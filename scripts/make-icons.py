"""Builds Catlify icons from design/icon-source.png (a tile on a transparent, glowing background).

The tile's background gradient is fitted with a smooth polynomial, so the icon can be extended
edge to edge (no transparent corners or glow) and padded for Android's adaptive icon safe zone.
"""
import sys
import numpy as np
from PIL import Image, ImageFilter

src = Image.open(sys.argv[1]).convert("RGBA")
out_dir = sys.argv[2]
a = np.asarray(src).astype(np.float64)
H, W = a.shape[:2]
X0, X1, Y0, Y1 = 154, 1084, 176, 1085          # tile bounds in the source
yy, xx = np.mgrid[0:H, 0:W]
u = (xx - X0) / (X1 - X0)                         # tile coordinates, 0..1
v = (yy - Y0) / (Y1 - Y0)

# Background samples: opaque tile pixels away from the edge and away from the cat/headphones.
opaque = a[..., 3] > 250
edge = (u > 0.06) & (u < 0.94) & (v > 0.06) & (v < 0.94)
content = (xx > 255) & (xx < 980) & (yy > 295) & (yy < 945)
sample = opaque & edge & ~content
su, sv = u[sample][::7], v[sample][::7]

def basis(u, v, deg=3):
    return np.stack([u**i * v**j for i in range(deg + 1) for j in range(deg + 1 - i)], axis=-1)

B = basis(su, sv)
coef = [np.linalg.lstsq(B, a[..., c][sample][::7], rcond=None)[0] for c in range(3)]
def model(u, v):
    # Clamp so the polynomial doesn't run away outside the tile.
    u, v = np.clip(u, -0.02, 1.02), np.clip(v, -0.02, 1.02)
    Bm = basis(u, v)
    return np.stack([Bm @ coef[c] for c in range(3)], axis=-1)

resid = np.abs(a[..., :3][sample][::7] - B @ np.stack(coef, axis=1)).mean()
print(f"background fit: mean error {resid:.1f}/255")

# Where to trust the original: inside the tile, eroded and feathered.
tile = Image.fromarray(((a[..., 3] > 250) * 255).astype(np.uint8))
tile = tile.filter(ImageFilter.MinFilter(25)).filter(ImageFilter.GaussianBlur(6))
keep = np.asarray(tile).astype(np.float64)[..., None] / 255.0
srcrgb = a[..., :3]

def render(size, scale):
    """Square image: the tile drawn at `scale` of the canvas, gradient everywhere else."""
    gy, gx = np.mgrid[0:size, 0:size] + 0.5
    tu = (gx / size - 0.5) / scale + 0.5
    tv = (gy / size - 0.5) / scale + 0.5
    bg = model(tu, tv)
    sx = X0 + tu * (X1 - X0)
    sy = Y0 + tv * (Y1 - Y0)
    inside = (sx >= 0) & (sx < W - 1) & (sy >= 0) & (sy < H - 1)
    # Bilinear sample of the source (and of the trust mask).
    x0 = np.clip(np.floor(sx).astype(int), 0, W - 2); y0 = np.clip(np.floor(sy).astype(int), 0, H - 2)
    fx = (sx - x0)[..., None]; fy = (sy - y0)[..., None]
    def bil(img):
        return (img[y0, x0] * (1 - fx) * (1 - fy) + img[y0, x0 + 1] * fx * (1 - fy)
                + img[y0 + 1, x0] * (1 - fx) * fy + img[y0 + 1, x0 + 1] * fx * fy)
    orig = bil(srcrgb)
    k = bil(keep) * inside[..., None]
    rgb = orig * k + bg * (1 - k)
    return Image.fromarray(np.clip(rgb + 0.5, 0, 255).astype(np.uint8), "RGB")

# Full bleed: the tile fills the canvas (iOS masks it; Android legacy; base for the rest).
full = render(1024, 1.0)
full.save(f"{out_dir}/icon-full.png")
# Android adaptive icon layer (108dp, only the middle 72dp shows): shrink so the headphones
# stay inside the safe zone; the gradient continues to the edges.
render(1080, 0.80).save(f"{out_dir}/icon-adaptive.png")
# Desktop: the full-bleed icon with rounded corners (macOS/Windows don't mask icons).
mask = Image.new("L", (4096, 4096), 0)
from PIL import ImageDraw
ImageDraw.Draw(mask).rounded_rectangle((0, 0, 4095, 4095), radius=int(4096 * 0.225), fill=255)
mask = mask.resize((1024, 1024), Image.LANCZOS)
rounded = full.convert("RGBA"); rounded.putalpha(mask)
rounded.save(f"{out_dir}/icon-rounded.png")
# macOS: Apple's grid puts the 824 px tile on a 1024 px canvas with a soft drop shadow, so the
# icon sits at the same size as other apps in the Dock (src-tauri/icons/icon.icns via iconutil).
from PIL import ImageFilter
mac = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
tile = rounded.resize((824, 824), Image.LANCZOS)
shadow = Image.new("RGBA", (1024, 1024), (0, 0, 0, 0))
shadow.paste((0, 0, 0, 90), (100, 112), tile.split()[3])
mac = Image.alpha_composite(mac, shadow.filter(ImageFilter.GaussianBlur(14)))
mac.alpha_composite(tile, (100, 100))
mac.save(f"{out_dir}/icon-macos.png")
print("ok")
