"""Export student-model candidates to CoreML .mlpackage (random weights).

S1: torchvision LR-ASPP on MobileNetV3-Large backbone, 8 classes.
S2: timm mobilenetv4_conv_small features_only + handwritten LR-ASPP head, 8 classes.
H1: timm mobilenetv4_conv_small classifier, 6 classes, 128x128.
H2: timm mobilenetv3_small_100 classifier, 6 classes, 128x128.

Each model is jit-traced, converted to mlprogram FP16 with an RGB image
input (scale 1/255), minimum deployment target macOS 15.
"""

import sys
from pathlib import Path

import coremltools as ct
import timm
import torch
import torch.nn as nn
import torch.nn.functional as F
import torchvision

OUT = Path(__file__).resolve().parent / "models"
OUT.mkdir(exist_ok=True)


class SegOutOnly(nn.Module):
    """Unwrap torchvision's OrderedDict output to a single tensor."""

    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, x):
        return self.model(x)["out"]


class ConvBNReLU(nn.Sequential):
    def __init__(self, cin, cout, k=1):
        super().__init__(
            nn.Conv2d(cin, cout, k, bias=False),
            nn.BatchNorm2d(cout),
            nn.ReLU(inplace=True),
        )


class LRASPPHead(nn.Module):
    """Same structure as torchvision's LRASPPHead."""

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


class TimmLRASPP(nn.Module):
    """timm features_only backbone + LR-ASPP head, upsampled to input size."""

    def __init__(self, backbone_name, num_classes):
        super().__init__()
        self.backbone = timm.create_model(
            backbone_name, pretrained=False, features_only=True
        )
        info = self.backbone.feature_info
        reductions = info.reduction()
        channels = info.channels()
        idx_low = reductions.index(8)
        idx_high = reductions.index(32)
        self.idx_low = idx_low
        self.idx_high = idx_high
        self.head = LRASPPHead(
            low_ch=channels[idx_low],
            high_ch=channels[idx_high],
            num_classes=num_classes,
        )

    def forward(self, x):
        feats = self.backbone(x)
        out = self.head(feats[self.idx_low], feats[self.idx_high])
        return F.interpolate(
            out, size=x.shape[-2:], mode="bilinear", align_corners=False
        )


def build_candidates():
    s1 = SegOutOnly(
        torchvision.models.segmentation.lraspp_mobilenet_v3_large(
            weights=None, weights_backbone=None, num_classes=8
        )
    )
    s2 = TimmLRASPP("mobilenetv4_conv_small", num_classes=8)
    h1 = timm.create_model(
        "mobilenetv4_conv_small", pretrained=False, num_classes=6
    )
    h2 = timm.create_model(
        "mobilenetv3_small_100", pretrained=False, num_classes=6
    )
    return {
        "S1": (s1, [256, 320]),
        "S2": (s2, [256, 320]),
        "H1": (h1, [128]),
        "H2": (h2, [128]),
    }


def export(name, model, size):
    model.eval()
    example = torch.rand(1, 3, size, size)
    with torch.no_grad():
        traced = torch.jit.trace(model, example)
        traced = torch.jit.freeze(traced)
    inputs = [
        ct.ImageType(
            name="image",
            shape=example.shape,
            color_layout=ct.colorlayout.RGB,
            scale=1.0 / 255.0,
        )
    ]
    mlmodel = ct.convert(
        traced,
        inputs=inputs,
        convert_to="mlprogram",
        compute_precision=ct.precision.FLOAT16,
        minimum_deployment_target=ct.target.macOS15,
    )
    path = OUT / f"{name}_{size}.mlpackage"
    mlmodel.save(str(path))
    return path


def main():
    print(f"torch {torch.__version__} timm {timm.__version__} ct {ct.__version__}")
    only = sys.argv[1] if len(sys.argv) > 1 else None
    for name, (model, sizes) in build_candidates().items():
        if only and name != only:
            continue
        n_params = sum(p.numel() for p in model.parameters())
        print(f"{name} params {n_params} ({n_params / 1e6:.2f}M)", flush=True)
        for size in sizes:
            path = export(name, model, size)
            mb = sum(
                f.stat().st_size for f in path.rglob("*") if f.is_file()
            ) / 2**20
            print(f"exported {path.name} size_mib {mb:.2f}", flush=True)


if __name__ == "__main__":
    main()
