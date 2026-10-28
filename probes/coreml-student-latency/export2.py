"""Export D10-D12 student candidates to CoreML .mlpackage (random weights).

Segmenters (D10/D11): timm features_only backbone, take the stride-8 and
stride-32 feature maps via feature_info reductions, generic LR-ASPP-style
head (low/high classifier + bilinear upsample to the input size), 7 classes.
Sizes W x H = 384x216 and 320x180; each with (suffix _pm) and without a
previous-mask input channel.

Face head F1: LR-ASPP on mobilenetv4_conv_small, 128x128, 3 classes.

Hand head H3: mobilenetv3_small_100 classifier, 128x128, 47 outputs
(21x2 keypoints + 5 finger logits). Exported as ImageType batch 1 and as
TensorType fp16 batch 1 and batch 2.
"""

import json
import sys
from pathlib import Path

import coremltools as ct
import timm
import torch
import torch.nn as nn
import torch.nn.functional as F

OUT = Path(__file__).resolve().parent / "models2"
OUT.mkdir(exist_ok=True)

NUM_SEG_CLASSES = 7
BACKBONES = [
    "mobilenetv4_conv_small",
    "mobilenetv4_conv_small_050",
    "repvit_m0_9",
    "efficientvit_b0",
]
SHORT = {
    "mobilenetv4_conv_small": "mnv4s",
    "mobilenetv4_conv_small_050": "mnv4s50",
    "repvit_m0_9": "repvit09",
    "efficientvit_b0": "effvitb0",
}


class ConvBNReLU(nn.Sequential):
    def __init__(self, cin, cout, k=1):
        super().__init__(
            nn.Conv2d(cin, cout, k, bias=False),
            nn.BatchNorm2d(cout),
            nn.ReLU(inplace=True),
        )


class LRASPPHead(nn.Module):
    def __init__(self, low_ch, high_ch, num_classes, inter_ch=128):
        super().__init__()
        self.cbr = ConvBNReLU(high_ch, inter_ch, 1)
        self.scale = nn.Sequential(
            nn.AdaptiveAvgPool2d(1),
            nn.Conv2d(high_ch, inter_ch, 1),
            nn.Sigmoid(),
        )
        self.low_classifier = nn.Conv2d(low_ch, num_classes, 1)
        self.high_classifier = nn.Conv2d(inter_ch, num_classes, 1)

    def forward(self, low, high):
        x = self.cbr(high)
        s = self.scale(high)
        x = x * s
        x = F.interpolate(
            x, size=low.shape[-2:], mode="bilinear", align_corners=False
        )
        return self.low_classifier(low) + self.high_classifier(x)


class TimmSeg(nn.Module):
    """timm features_only backbone + generic low/high-resolution head."""

    def __init__(self, backbone_name, num_classes, in_chans=3):
        super().__init__()
        self.backbone = timm.create_model(
            backbone_name,
            pretrained=False,
            features_only=True,
            in_chans=in_chans,
        )
        info = self.backbone.feature_info
        reductions = info.reduction()
        channels = info.channels()
        # stride-8 and stride-32 features
        self.idx_low = reductions.index(8)
        self.idx_high = reductions.index(32)
        self.head = LRASPPHead(
            low_ch=channels[self.idx_low],
            high_ch=channels[self.idx_high],
            num_classes=num_classes,
        )

    def forward(self, *xs):
        x = xs[0] if len(xs) == 1 else torch.cat(xs, dim=1)
        feats = self.backbone(x)
        out = self.head(feats[self.idx_low], feats[self.idx_high])
        return F.interpolate(
            out, size=x.shape[-2:], mode="bilinear", align_corners=False
        )


def export(name, model, h, w, prev_mask=False, batch=1, as_image=True):
    model.eval()
    ch = 4 if prev_mask else 3
    inputs_py = [torch.rand(batch, 3, h, w)]
    if prev_mask:
        inputs_py.append(torch.rand(batch, 1, h, w))
    with torch.no_grad():
        traced = torch.jit.trace(model, inputs_py)
        traced = torch.jit.freeze(traced)
    if as_image:
        inputs = [
            ct.ImageType(
                name="image",
                shape=(batch, 3, h, w),
                color_layout=ct.colorlayout.RGB,
                scale=1.0 / 255.0,
            )
        ]
    else:
        inputs = [
            ct.TensorType(
                name="image",
                shape=(batch, 3, h, w),
                dtype=ct.converters.mil.mil.types.fp16,
            )
        ]
    if prev_mask:
        inputs.append(
            ct.TensorType(
                name="prev_mask",
                shape=(batch, 1, h, w),
                dtype=ct.converters.mil.mil.types.fp16,
            )
        )
    mlmodel = ct.convert(
        traced,
        inputs=inputs,
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16,
        minimum_deployment_target=ct.target.macOS15,
    )
    path = OUT / f"{name}.mlpackage"
    mlmodel.save(str(path))
    return path


def param_count(model):
    return sum(p.numel() for p in model.parameters())


def main():
    print(f"torch {torch.__version__} timm {timm.__version__} ct {ct.__version__}")
    which = sys.argv[1] if len(sys.argv) > 1 else None
    manifest = {}

    # --- segmenters: 4 backbones x 2 sizes x {plain, _pm}
    for bb in BACKBONES:
        if which and which != "seg":
            break
        for (w, h) in [(384, 216), (320, 180)]:
            for pm in [False, True]:
                tag = SHORT[bb]
                name = f"seg_{tag}_{w}x{h}" + ("_pm" if pm else "")
                try:
                    model = TimmSeg(
                        bb, NUM_SEG_CLASSES, in_chans=4 if pm else 3
                    )
                    path = export(name, model, h, w, prev_mask=pm)
                    mb = sum(
                        f.stat().st_size
                        for f in path.rglob("*")
                        if f.is_file()
                    ) / 2**20
                    np = param_count(model)
                    manifest[name] = {"params": np, "size_mib": round(mb, 2)}
                    print(f"exported {name} params={np} size_mib={mb:.2f}", flush=True)
                except Exception as e:
                    manifest[name] = {"error": repr(e)}
                    print(f"FAILED {name}: {e!r}", flush=True)

    # --- face head F1: LR-ASPP mnv4s 128x128 3 classes
    if which in (None, "f1"):
        try:
            model = TimmSeg("mobilenetv4_conv_small", 3)
            path = export("face_f1_mnv4s_128", model, 128, 128)
            mb = sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / 2**20
            manifest["face_f1_mnv4s_128"] = {
                "params": param_count(model),
                "size_mib": round(mb, 2),
            }
            print(f"exported face_f1 params={param_count(model)} size_mib={mb:.2f}")
        except Exception as e:
            manifest["face_f1_mnv4s_128"] = {"error": repr(e)}
            print(f"FAILED face_f1: {e!r}", flush=True)

    # --- hand head H3: mobilenetv3_small_100 classifier, 47 outputs
    if which in (None, "h3"):
        try:
            model = timm.create_model(
                "mobilenetv3_small_100", pretrained=False, num_classes=47
            )
            model.eval()
            np_ = param_count(model)
            for name, batch, as_image in [
                ("hand_h3_mnv3_128_img_b1", 1, True),
                ("hand_h3_mnv3_128_t_b1", 1, False),
                ("hand_h3_mnv3_128_t_b2", 2, False),
            ]:
                path = export(name, model, 128, 128, batch=batch, as_image=as_image)
                mb = sum(f.stat().st_size for f in path.rglob("*") if f.is_file()) / 2**20
                manifest[name] = {"params": np_, "size_mib": round(mb, 2)}
                print(f"exported {name} params={np_} size_mib={mb:.2f}", flush=True)
        except Exception as e:
            manifest["hand_h3"] = {"error": repr(e)}
            print(f"FAILED hand_h3: {e!r}", flush=True)

    (OUT / "manifest.json").write_text(json.dumps(manifest, indent=1) + "\n")
    print("manifest ->", OUT / "manifest.json")


if __name__ == "__main__":
    main()
