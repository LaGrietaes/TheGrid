#!/usr/bin/env python3
import json
import sys


def emit_error(msg: str) -> None:
    # Non-zero exit lets Rust fallback to CPU analysis.
    print(msg, file=sys.stderr)
    sys.exit(2)


def main() -> None:
    if len(sys.argv) < 2:
        emit_error("usage: cuda_media_analyze.py <image_path>")

    img_path = sys.argv[1]

    try:
        from PIL import Image
    except Exception as e:
        emit_error(f"Pillow missing: {e}")

    try:
        import torch
        import torch.nn.functional as F
    except Exception as e:
        emit_error(f"torch missing: {e}")

    if not torch.cuda.is_available():
        emit_error("CUDA not available")

    try:
        img = Image.open(img_path).convert("RGB")
    except Exception as e:
        emit_error(f"failed to open image: {e}")

    w, h = img.size
    mp = (w * h) / 1_000_000.0

    # Resize to a bounded tensor for quick GPU metrics.
    max_edge = 1280
    scale = min(max_edge / max(w, h), 1.0)
    tw = max(1, int(w * scale))
    th = max(1, int(h * scale))
    if (tw, th) != (w, h):
        img = img.resize((tw, th), Image.Resampling.BILINEAR)

    x = torch.from_numpy(__import__("numpy").array(img)).to(torch.float32)
    x = x.permute(2, 0, 1).unsqueeze(0) / 255.0
    x = x.to("cuda", non_blocking=True)

    # Grayscale
    gray = 0.299 * x[:, 0:1] + 0.587 * x[:, 1:2] + 0.114 * x[:, 2:3]

    brightness = float(gray.mean().item())
    contrast = float(gray.std(unbiased=False).item())

    # Laplacian variance for focus estimate.
    lap_kernel = torch.tensor(
        [[0.0, -1.0, 0.0], [-1.0, 4.0, -1.0], [0.0, -1.0, 0.0]],
        device="cuda",
        dtype=torch.float32,
    ).view(1, 1, 3, 3)
    lap = F.conv2d(gray, lap_kernel, padding=1)
    focus_raw = float(lap.var(unbiased=False).item())
    focus_score = max(0.0, min(1.0, focus_raw / 1200.0))

    # Simple noise estimate: mean absolute neighboring differences.
    dx = torch.abs(gray[:, :, :, 1:] - gray[:, :, :, :-1]).mean()
    dy = torch.abs(gray[:, :, 1:, :] - gray[:, :, :-1, :]).mean()
    noise = float(((dx + dy) * 0.5).item())
    noise_score = max(0.0, min(1.0, noise))

    in_focus = focus_score >= 0.30

    quality_score = (
        (focus_score * 0.50)
        + (contrast * 0.20)
        + (max(0.0, min(1.0, 1.0 - abs(brightness - 0.50) * 2.0)) * 0.20)
        + (max(0.0, min(1.0, 1.0 - noise_score)) * 0.10)
    )
    quality_score = max(0.0, min(1.0, quality_score))

    payload = {
        "description": f"{w}x{h} ({mp:.1f}MP), focus {focus_score:.2f}, quality {quality_score:.2f}, mode dedicated_gpu(cuda)",
        "tags": ["cuda", "gpu", "image"],
        "dominant_colors": [],
        "face_count": 0,
        "ocr_text": None,
        "width": w,
        "height": h,
        "megapixels": mp,
        "focus_score": focus_score,
        "quality_score": quality_score,
        "brightness": brightness,
        "contrast": contrast,
        "in_focus": in_focus,
        "camera_make": None,
        "camera_model": None,
        "lens_model": None,
        "iso": None,
        "aperture": None,
        "shutter_seconds": None,
        "focal_length_mm": None,
        "captured_at": None,
        "gps_lat": None,
        "gps_lon": None,
    }

    print(json.dumps(payload, ensure_ascii=True))


if __name__ == "__main__":
    main()
