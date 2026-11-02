"""Student models, losses and a plain tar shard reader (train.setup, D10-D12).

seg:  MobileNetV4-Conv-Small (timm, ImageNet weights) with a 4th input
      channel for the previous frame's mask (D11), LR-ASPP head on the
      stride-8 and stride-32 features, 7 classes at 384x216.
face: the same LR-ASPP on a 128 px face crop (D10), no prev-mask channel,
      low branch on the stride-4 features, 3 classes (other, eye, mouth).
hand: MobileNetV4-Conv-Small classifier on a 128 px crop -> 21 keypoints
      (x, y in crop coordinates, sigmoid) + 5 finger-state logits. Keypoint
      targets come from MediaPipe Hand Landmarker (Apache-2.0), or SAM 3D Body
      where MediaPipe missed the hand (its fingertips carry conf 0); crops
      without a teacher hand have conf 0 and drop out of the keypoint loss.
      Apple Vision output must never be a target (DPLA 3.2).

Both models take RGB in 0..1 and normalise inside, so the CoreML export can
use an ImageType with scale 1/255. The previous-mask channel carries the
class id / 6 (one channel, as measured in probes/coreml-student-latency).
"""

import io
import json
import tarfile
from collections import defaultdict
from contextlib import contextmanager

import numpy as np
import timm
import torch
import torch.nn as nn
import torch.nn.functional as F
from PIL import Image

BACKBONE = "mobilenetv4_conv_small.e2400_r224_in1k"
NUM_CLASSES = 7
NUM_FACE_CLASSES = 3  # other, eye, mouth
NUM_JOINTS = 21
IGNORE = 255  # label value that drops out of every loss term
DICE_WEIGHT = 0.5
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
    def __init__(self, pretrained=True, prev_mask=True, num_classes=NUM_CLASSES, low_stride=8, backbone=BACKBONE):
        super().__init__()
        self.prev_mask = prev_mask
        self.norm = Normalize()
        self.backbone = timm.create_model(backbone, pretrained=pretrained, features_only=True,
                                          in_chans=4 if prev_mask else 3)
        red, ch = self.backbone.feature_info.reduction(), self.backbone.feature_info.channels()
        self.i_low, self.i_high = red.index(low_stride), red.index(32)
        self.head = LRASPP(ch[self.i_low], ch[self.i_high], num_classes)

    def forward(self, image, prev_mask=None):
        x = self.norm(image)
        if self.prev_mask:
            x = torch.cat([x, prev_mask], dim=1)
        feats = self.backbone(x)
        out = self.head(feats[self.i_low], feats[self.i_high])
        return F.interpolate(out, size=image.shape[-2:], mode="bilinear", align_corners=False)


class FaceStudent(SegStudent):
    """Eye and mouth on the 128 px face crop (D10): [other, eye, mouth].

    An eye is about 10x5 px in the crop, so the LR-ASPP low branch reads the
    stride-4 features (32x32) instead of stride 8.
    """

    def __init__(self, pretrained=True):
        super().__init__(pretrained, prev_mask=False, num_classes=NUM_FACE_CLASSES, low_stride=4)

    def forward(self, image):
        return super().forward(image)


class HandStudent(nn.Module):
    def __init__(self, pretrained=True, backbone=BACKBONE):
        super().__init__()
        self.norm = Normalize()
        self.net = timm.create_model(backbone, pretrained=pretrained, num_classes=NUM_JOINTS * 2 + 5)

    def forward(self, image):
        out = self.net(self.norm(image))
        keypoints = torch.sigmoid(out[:, :NUM_JOINTS * 2]).view(-1, NUM_JOINTS, 2)
        return keypoints, out[:, NUM_JOINTS * 2:]


def soft_dice(logits, mask):
    """1 - soft Dice per foreground class over the whole batch, averaged.

    Eye and mouth cover 0.1 to 1 % of the pixels; cross-entropy alone
    under-segments them, Dice pulls toward the IoU the gate measures.
    """
    valid = (mask != IGNORE).unsqueeze(1).float()
    probs = logits.float().softmax(1) * valid
    onehot = F.one_hot(mask.clamp(max=logits.shape[1] - 1), logits.shape[1]).permute(0, 3, 1, 2).float() * valid
    inter = (probs * onehot).sum((0, 2, 3))[1:]
    denom = (probs.sum((0, 2, 3)) + onehot.sum((0, 2, 3)))[1:]
    return (1 - (2 * inter + 1) / (denom + 1)).mean()


@contextmanager
def frozen_bn_stats(model):
    """Batch statistics still normalise, running statistics stay as they are."""
    bns = [m for m in model.modules() if isinstance(m, nn.modules.batchnorm._BatchNorm)]
    saved = [m.momentum for m in bns]
    for m in bns:
        m.momentum = 0.0
    try:
        yield
    finally:
        for m, mom in zip(bns, saved):
            m.momentum = mom


def recalibrate_bn(model, batches):
    """Recompute BatchNorm running statistics as a plain average over batches
    (forward only, train mode). Repairs a model whose running stats drifted."""
    bns = [m for m in model.modules() if isinstance(m, nn.modules.batchnorm._BatchNorm)]
    for m in bns:
        m.reset_running_stats()
        m.momentum = None  # cumulative average
    model.train()
    with torch.no_grad():
        for args in batches:
            model(*args)
    for m in bns:
        m.momentum = 0.1
    return model.eval()


def seg_loss(model, batch, dice_weight=DICE_WEIGHT):
    """Cross-entropy + Dice on SAM masks + temporal consistency with the previous frame.

    The temporal term compares class probabilities of frame t and t-1 only on
    pixels where both SAM masks agree (no motion there per the teacher), and
    only for samples that have a previous frame (batch["has_prev"]).
    Pixels labelled IGNORE (S2 eye, glasses) drop out of every term.
    """
    # The gradient-free pass on frame t-1 has an all-zero prev channel. It must
    # not update the BatchNorm running statistics: in seg-a-s0 it did, half of
    # all updates saw "no prev", and in eval mode a real prev mask then turned
    # hands into "object" (probes/train-students/RESULT.md).
    # It also runs before the graded pass (self-prev needs its output) and
    # outside autocast: all round-2 seg runs that cast weights first under
    # no_grad trained at twice the cross-entropy (CUDA autocast weight cache,
    # probes/train-students/RESULT.md).
    dev_type = batch["image"].device.type
    with torch.no_grad(), frozen_bn_stats(model), torch.autocast(device_type=dev_type, enabled=False):
        prev_logits = model(batch["prev_image"], torch.zeros_like(batch["prev_mask_in"])).float()
    prev_in = batch["prev_mask_in"]
    if "self_prev" in batch:
        # live, the prev mask is the student's own last prediction, not SAM's:
        # feed that on the selected samples (scheduled sampling, one step)
        own = prev_logits.argmax(1, keepdim=True).float() / (NUM_CLASSES - 1)
        sel = batch["self_prev"].view(-1, 1, 1, 1)
        prev_in = sel * own + (1 - sel) * prev_in
    logits = model(batch["image"], prev_in)
    ce = F.cross_entropy(logits, batch["mask"], ignore_index=IGNORE)
    dice = soft_dice(logits, batch["mask"])
    has_prev = batch.get("has_prev", torch.ones(len(logits), device=logits.device)).view(-1, 1, 1)
    stable = ((batch["mask"] == batch["prev_mask"]) & (batch["mask"] != IGNORE)).float() * has_prev
    stable = stable.unsqueeze(1)
    diff = (logits.softmax(1) - prev_logits.softmax(1)).abs().sum(1, keepdim=True)
    temporal = (diff * stable).sum() / stable.sum().clamp(min=1)
    loss = ce + dice_weight * dice + TEMPORAL_WEIGHT * temporal
    return loss, {"ce": ce.item(), "dice": dice.item(), "temporal": temporal.item()}


def face_loss(model, batch, dice_weight=DICE_WEIGHT):
    logits = model(batch["image"])
    ce = F.cross_entropy(logits, batch["mask"], ignore_index=IGNORE)
    dice = soft_dice(logits, batch["mask"])
    return ce + dice_weight * dice, {"ce": ce.item(), "dice": dice.item()}


def hand_loss(model, batch):
    keypoints, finger_logits = model(batch["image"])
    fm = batch["finger_mask"]
    if fm.dim() == 1:  # one flag per sample -> per finger
        fm = fm.unsqueeze(1).expand(-1, 5)
    finger = (F.binary_cross_entropy_with_logits(finger_logits, batch["fingers"], reduction="none")
              * fm).sum() / fm.sum().clamp(min=1)
    w = (batch["kp_conf"] > KP_MIN_CONF).float()
    kp = ((keypoints - batch["kp"]).abs().sum(-1) * w).sum() / w.sum().clamp(min=1)
    return finger + kp, {"finger_bce": finger.item(), "kp_l1": kp.item()}


FINGER_CHAINS = [(1, 2, 3, 4), (5, 6, 7, 8), (9, 10, 11, 12), (13, 14, 15, 16), (17, 18, 19, 20)]


def finger_geometry(kp):
    """Per-finger extension features from 21 keypoints (B, 21, 2) -> (B, 5, 3).

    straightness: tip-to-mcp distance over the bone path; reach: tip minus pip
    distance from the wrist, over palm length; thumb: tip to index mcp and to
    little mcp, over palm length. A logistic regression on these, fitted on the
    script labels of the training crops, reads the teacher keypoints at 0.94
    (thumb) and 0.98 to 0.99 (other fingers) per finger on dev.
    """
    w = kp[:, 0]
    palm = (kp[:, 9] - w).norm(dim=-1).clamp(min=1e-6)
    out = []
    for f, (m, p, d, t) in enumerate(FINGER_CHAINS):
        path = (kp[:, p] - kp[:, m]).norm(dim=-1) + (kp[:, d] - kp[:, p]).norm(dim=-1) + (kp[:, t] - kp[:, d]).norm(dim=-1)
        straight = (kp[:, t] - kp[:, m]).norm(dim=-1) / path.clamp(min=1e-6)
        if f == 0:
            a = (kp[:, 4] - kp[:, 5]).norm(dim=-1) / palm
            b = (kp[:, 4] - kp[:, 17]).norm(dim=-1) / palm
        else:
            a = ((kp[:, t] - w).norm(dim=-1) - (kp[:, p] - w).norm(dim=-1)) / palm
            b = torch.zeros_like(a)
        out.append(torch.stack([straight, a, b], -1))
    return torch.stack(out, 1)


def fit_finger_geometry(feats, labels, steps=200):
    """Logistic regression per finger: feats (N, 5, 3), labels (N, 5) -> weights (5, 4)."""
    mu, sd = feats.mean(0), feats.std(0).clamp(min=1e-6)
    x = (feats - mu) / sd
    w = torch.zeros(5, 4, requires_grad=True)
    opt = torch.optim.LBFGS([w], max_iter=steps)

    def closure():
        opt.zero_grad()
        z = (x * w[:, :3]).sum(-1) + w[:, 3]
        loss = F.binary_cross_entropy_with_logits(z, labels) + 1e-3 * (w[:, :3] ** 2).sum()
        loss.backward()
        return loss
    opt.step(closure)
    return {"w": w.detach().tolist(), "mu": mu.tolist(), "sd": sd.tolist()}


def finger_geometry_prob(kp, geo):
    """P(extended) per finger from keypoints with fitted weights; kp (B, 21, 2)."""
    x = (finger_geometry(kp) - torch.tensor(geo["mu"], device=kp.device)) / torch.tensor(geo["sd"], device=kp.device)
    w = torch.tensor(geo["w"], device=kp.device)
    return torch.sigmoid((x * w[:, :3]).sum(-1) + w[:, 3])


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


def face_batch(samples):
    return {"image": torch.stack([_rgb(s["jpg"]) for s in samples]),
            "mask": torch.stack([_label(s["mask.png"]) for s in samples])}


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


class HandDeploy(nn.Module):
    """Hand student as shipped: same I/O as HandStudent (keypoints 1x21x2,
    finger logits 1x5), with the runtime levers baked in. tta: mean of the
    finger probabilities of the crop and its mirror (no output carries a side);
    geo: mean with the finger-geometry probability of the predicted keypoints;
    thresholds: dev-calibrated per finger, folded into the logit so that the
    plugin reads "extended" as logit > 0."""

    def __init__(self, model, tta=False, geo=None, thresholds=None):
        super().__init__()
        self.model, self.tta, self.use_geo = model, tta, geo is not None
        if geo is not None:
            self.register_buffer("g_w", torch.tensor(geo["w"]))
            self.register_buffer("g_mu", torch.tensor(geo["mu"]))
            self.register_buffer("g_sd", torch.tensor(geo["sd"]))
        t = torch.tensor(thresholds or [0.5] * 5).clamp(1e-3, 1 - 1e-3)
        self.register_buffer("bias", torch.log(t) - torch.log1p(-t))

    def forward(self, image):
        kp, logits = self.model(image)
        p = torch.sigmoid(logits)
        if self.tta:
            p = (p + torch.sigmoid(self.model(image.flip(-1))[1])) / 2
        if self.use_geo:
            x = (finger_geometry(kp) - self.g_mu) / self.g_sd
            p = (p + torch.sigmoid((x * self.g_w[:, :3]).sum(-1) + self.g_w[:, 3])) / 2
        p = p.clamp(1e-4, 1 - 1e-4)
        return kp, torch.log(p) - torch.log1p(-p) - self.bias


class FaceDeploy(nn.Module):
    """Face student as shipped: logits 1x3x128x128 with an eye bias (lower eye
    threshold) and an eye dilation (max-pool of the eye logit, odd kernel)."""

    def __init__(self, model, eye_bias=0.0, eye_dilate=1):
        super().__init__()
        self.model, self.eye_bias, self.k = model, float(eye_bias), int(eye_dilate)

    def forward(self, image):
        out = self.model(image)
        eye = out[:, 1:2] + self.eye_bias
        if self.k > 1:
            eye = F.max_pool2d(eye, self.k, stride=1, padding=self.k // 2)
        return torch.cat([out[:, :1], eye, out[:, 2:]], 1)
