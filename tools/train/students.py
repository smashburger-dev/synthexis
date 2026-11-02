"""Student models, losses and a plain tar shard reader (train.setup, D10-D12).

seg:  MobileNetV4-Conv-Small (timm, ImageNet weights) with a 4th input
      channel for the previous frame's mask (D11), LR-ASPP head on the
      stride-8 and stride-32 features, 7 classes at 384x216.
hand: MobileNetV4-Conv-Small classifier on a 128 px crop -> 21 keypoints
      (x, y in crop coordinates, sigmoid) + 5 finger-state logits. Keypoint
      targets come from MediaPipe Hand Landmarker (Apache-2.0); crops without
      a matched keypoint hand have conf 0 and drop out of the keypoint loss.
      Apple Vision output must never be a target (DPLA 3.2).

Both models take RGB in 0..1 and normalise inside, so the CoreML export can
use an ImageType with scale 1/255. The previous-mask channel carries the
class id / 6 (one channel, as measured in probes/coreml-student-latency).
"""

import io
import json
import tarfile
from collections import defaultdict

import numpy as np
import timm
import torch
import torch.nn as nn
import torch.nn.functional as F
from PIL import Image

BACKBONE = "mobilenetv4_conv_small.e2400_r224_in1k"
NUM_CLASSES = 7
NUM_JOINTS = 21
MEAN = (0.485, 0.456, 0.406)
STD = (0.229, 0.224, 0.225)
KP_MIN_CONF = 0.3
TEMPORAL_WEIGHT = 0.1  # placeholder until the first pilot run tunes it


class Normalize(nn.Module):
    def __init__(self):
        super().__init__()
        self.register_buffer("mean", torch.tensor(MEAN).view(1, 3, 1, 1))
        self.register_buffer("std", torch.tensor(STD).view(1, 3, 1, 1))

    def forward(self, x):
        return (x - self.mean) / self.std


class LRASPP(nn.Module):
    def __init__(self, low_ch, high_ch, num_classes, inter_ch=128):
        super().__init__()
        self.cbr = nn.Sequential(nn.Conv2d(high_ch, inter_ch, 1, bias=False),
                                 nn.BatchNorm2d(inter_ch), nn.ReLU(inplace=True))
        self.scale = nn.Sequential(nn.AdaptiveAvgPool2d(1), nn.Conv2d(high_ch, inter_ch, 1), nn.Sigmoid())
        self.low_classifier = nn.Conv2d(low_ch, num_classes, 1)
        self.high_classifier = nn.Conv2d(inter_ch, num_classes, 1)

    def forward(self, low, high):
        x = self.cbr(high) * self.scale(high)
        x = F.interpolate(x, size=low.shape[-2:], mode="bilinear", align_corners=False)
        return self.low_classifier(low) + self.high_classifier(x)


class SegStudent(nn.Module):
    def __init__(self, pretrained=True, prev_mask=True):
        super().__init__()
        self.prev_mask = prev_mask
        self.norm = Normalize()
        self.backbone = timm.create_model(BACKBONE, pretrained=pretrained, features_only=True,
                                          in_chans=4 if prev_mask else 3)
        red, ch = self.backbone.feature_info.reduction(), self.backbone.feature_info.channels()
        self.i_low, self.i_high = red.index(8), red.index(32)
        self.head = LRASPP(ch[self.i_low], ch[self.i_high], NUM_CLASSES)

    def forward(self, image, prev_mask=None):
        x = self.norm(image)
        if self.prev_mask:
            x = torch.cat([x, prev_mask], dim=1)
        feats = self.backbone(x)
        out = self.head(feats[self.i_low], feats[self.i_high])
        return F.interpolate(out, size=image.shape[-2:], mode="bilinear", align_corners=False)


class HandStudent(nn.Module):
    def __init__(self, pretrained=True):
        super().__init__()
        self.norm = Normalize()
        self.net = timm.create_model(BACKBONE, pretrained=pretrained, num_classes=NUM_JOINTS * 2 + 5)

    def forward(self, image):
        out = self.net(self.norm(image))
        keypoints = torch.sigmoid(out[:, :NUM_JOINTS * 2]).view(-1, NUM_JOINTS, 2)
        return keypoints, out[:, NUM_JOINTS * 2:]


def seg_loss(model, batch):
    """Cross-entropy on SAM masks + temporal consistency with the previous frame.

    The temporal term compares class probabilities of frame t and t-1 only on
    pixels where both SAM masks agree (no motion there per the teacher).
    """
    logits = model(batch["image"], batch["prev_mask_in"])
    ce = F.cross_entropy(logits, batch["mask"])
    with torch.no_grad():
        prev_logits = model(batch["prev_image"], torch.zeros_like(batch["prev_mask_in"]))
    stable = (batch["mask"] == batch["prev_mask"]).float().unsqueeze(1)
    diff = (logits.softmax(1) - prev_logits.softmax(1)).abs().sum(1, keepdim=True)
    temporal = (diff * stable).sum() / stable.sum().clamp(min=1)
    return ce + TEMPORAL_WEIGHT * temporal, {"ce": ce.item(), "temporal": temporal.item()}


def hand_loss(model, batch):
    keypoints, finger_logits = model(batch["image"])
    fm = batch["finger_mask"]
    finger = (F.binary_cross_entropy_with_logits(finger_logits, batch["fingers"], reduction="none")
              .mean(1) * fm).sum() / fm.sum().clamp(min=1)
    w = (batch["kp_conf"] > KP_MIN_CONF).float()
    kp = ((keypoints - batch["kp"]).abs().sum(-1) * w).sum() / w.sum().clamp(min=1)
    return finger + kp, {"finger_bce": finger.item(), "kp_l1": kp.item()}


def read_samples(path, n=None, keep=None):
    """Samples of one tar shard as {ext: bytes} dicts, in shard order."""
    samples = defaultdict(dict)
    with tarfile.open(path) as tar:
        for member in tar:
            key, ext = member.name.split(".", 1)
            samples[key][ext] = tar.extractfile(member).read()
    out = []
    for key, s in samples.items():
        s["key"], s["meta"] = key, json.loads(s["json"])
        if keep is None or keep(s):
            out.append(s)
        if n and len(out) == n:
            break
    return out


def _rgb(data):
    return torch.from_numpy(np.asarray(Image.open(io.BytesIO(data)).convert("RGB")).copy()).permute(2, 0, 1).float() / 255


def _label(data):
    return torch.from_numpy(np.asarray(Image.open(io.BytesIO(data))).astype(np.int64))


def seg_batch(samples):
    mask = torch.stack([_label(s["mask.png"]) for s in samples])
    prev_mask = torch.stack([_label(s["prev_mask.png"]) for s in samples])
    return {
        "image": torch.stack([_rgb(s["jpg"]) for s in samples]),
        "mask": mask,
        "prev_image": torch.stack([_rgb(s["prev.jpg"]) for s in samples]),
        "prev_mask": prev_mask,
        "prev_mask_in": prev_mask.unsqueeze(1).float() / (NUM_CLASSES - 1),
    }


def hand_batch(samples):
    fingers, finger_mask, kp, kp_conf = [], [], [], []
    for s in samples:
        m = s["meta"]
        fingers.append(m["fingers"] or [0] * 5)
        finger_mask.append(1.0 if m["fingers"] else 0.0)
        pts = m.get("keypoints") or [[0, 0, 0]] * NUM_JOINTS
        kp.append([p[:2] for p in pts])
        kp_conf.append([p[2] for p in pts])
    return {
        "image": torch.stack([_rgb(s["jpg"]) for s in samples]),
        "fingers": torch.tensor(fingers, dtype=torch.float32),
        "finger_mask": torch.tensor(finger_mask),
        "kp": torch.tensor(kp, dtype=torch.float32),
        "kp_conf": torch.tensor(kp_conf, dtype=torch.float32),
    }
