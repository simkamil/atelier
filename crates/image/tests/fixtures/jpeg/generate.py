# SPDX-License-Identifier: AGPL-3.0-or-later
"""Regenerate the tiny JPEG decoder corpus and its independent Pillow RGBA oracles."""

from pathlib import Path
from io import BytesIO
import random
from PIL import Image, __version__

ROOT = Path(__file__).parent
WIDTH, HEIGHT = 19, 17

rgb = Image.new("RGB", (WIDTH, HEIGHT))
rgb.putdata([
    ((x * 8 + y * 2) % 256, (y * 9 + x * 2) % 256, ((x + y) * 6) % 256)
    for y in range(HEIGHT)
    for x in range(WIDTH)
])

def noise_image():
    rng = random.Random(42042)
    image = Image.new("RGB", (WIDTH, HEIGHT))
    image.putdata([tuple(rng.randrange(256) for _ in range(3)) for _ in range(WIDTH * HEIGHT)])
    return image

cases = {
    "baseline-gray": (rgb.convert("L"), {"quality": 95}),
    "baseline-444": (rgb, {"quality": 95, "subsampling": 0}),
    "baseline-422": (rgb, {"quality": 95, "subsampling": 1}),
    "baseline-420": (rgb, {"quality": 95, "subsampling": 2}),
    "progressive-444": (rgb, {"quality": 95, "subsampling": 0, "progressive": True}),
    "progressive-422": (rgb, {"quality": 95, "subsampling": 1, "progressive": True}),
    "progressive-420": (rgb, {"quality": 95, "subsampling": 2, "progressive": True}),
    "baseline-cmyk": (rgb.convert("CMYK"), {"quality": 95}),
    "restart-420": (rgb, {"quality": 95, "subsampling": 2, "restart_marker_blocks": 1}),
    "baseline-noise-low": (noise_image(), {"quality": 20, "subsampling": 2}),
    "baseline-noise-high": (noise_image(), {"quality": 98, "subsampling": 0}),
    "progressive-noise": (noise_image(), {"quality": 90, "subsampling": 1, "progressive": True}),
    "progressive-gray": (rgb.convert("L"), {"quality": 90, "progressive": True}),
    "progressive-restart": (rgb, {"quality": 90, "subsampling": 2, "progressive": True, "restart_marker_blocks": 1}),
    "size-1x1": (rgb.crop((0, 0, 1, 1)), {"quality": 95, "subsampling": 2}),
    "size-16x16": (rgb.crop((0, 0, 16, 16)), {"quality": 95, "subsampling": 2}),
}

for name, (source, options) in cases.items():
    stream = BytesIO()
    source.save(stream, format="JPEG", **options)
    encoded = stream.getvalue()
    (ROOT / f"{name}.jpg").write_bytes(encoded)
    rgba = Image.open(BytesIO(encoded)).convert("RGBA")
    (ROOT / f"{name}.rgba").write_bytes(rgba.tobytes())

# Negative scan fixture: change the first symbol of the first progressive AC
# table into EOBRUN(1), leaving its bits to span the following restart marker.
invalid_restart = bytearray((ROOT / "progressive-restart.jpg").read_bytes())
at = 2
while True:
    marker_at = invalid_restart.index(b"\xff\xc4", at)
    length = int.from_bytes(invalid_restart[marker_at + 2:marker_at + 4], "big")
    payload = marker_at + 4
    end = marker_at + 2 + length
    while payload < end:
        info = invalid_restart[payload]
        counts = invalid_restart[payload + 1:payload + 17]
        symbols = sum(counts)
        if info >> 4 == 1 and symbols:
            invalid_restart[payload + 17] = 0x10
            (ROOT / "progressive-restart-eobrun-crosses-restart.jpg").write_bytes(invalid_restart)
            break
        payload += 17 + symbols
    if payload < end:
        break
    at = end

# Build a small real YCCK stream from four grayscale scans.  Separate scans are
# legal baseline JPEG; the APP14 marker selects the Adobe YCCK transform.
def jpeg_parts(encoded):
    pos = 2
    parts = []
    entropy = None
    while pos < len(encoded):
        assert encoded[pos] == 0xFF
        while encoded[pos] == 0xFF:
            pos += 1
        marker = encoded[pos]
        pos += 1
        if marker == 0xD9:
            break
        length = int.from_bytes(encoded[pos:pos + 2], "big")
        end = pos + length
        if marker == 0xDA:
            scan = end
            # Generated grayscale scans contain no restart markers.  Account
            # for byte stuffing while finding EOI.
            at = scan
            while at + 1 < len(encoded):
                if encoded[at] == 0xFF:
                    if encoded[at + 1] == 0:
                        at += 2
                        continue
                    break
                at += 1
            entropy = encoded[scan:at]
            break
        parts.append((marker, encoded[pos + 2:end]))
        pos = end
    return parts, entropy

def segment(marker, payload):
    return b"\xff" + bytes([marker]) + (len(payload) + 2).to_bytes(2, "big") + payload

source_cmyk = rgb.convert("CMYK")
y_plane, cb_plane, cr_plane, k_plane = [], [], [], []
for c, m, y, k in source_cmyk.getdata():
    r, g, b = 255 - c, 255 - m, 255 - y
    yy = round(0.299 * r + 0.587 * g + 0.114 * b)
    cb = round(128 - 0.168736 * r - 0.331264 * g + 0.5 * b)
    cr = round(128 + 0.5 * r - 0.418688 * g - 0.081312 * b)
    y_plane.append(max(0, min(255, yy)))
    cb_plane.append(max(0, min(255, cb)))
    cr_plane.append(max(0, min(255, cr)))
    k_plane.append(k)

planes = []
for values in (y_plane, cb_plane, cr_plane, k_plane):
    gray = Image.new("L", (WIDTH, HEIGHT))
    gray.putdata(values)
    stream = BytesIO()
    gray.save(stream, format="JPEG", quality=95)
    planes.append(stream.getvalue())
table_parts, _ = jpeg_parts(planes[0])
tables = b"".join(segment(marker, payload) for marker, payload in table_parts if marker in (0xDB, 0xC4))
components = bytes([8]) + HEIGHT.to_bytes(2, "big") + WIDTH.to_bytes(2, "big") + bytes([4])
components += b"".join(bytes([i, 0x11, 0]) for i in range(1, 5))
adobe = b"Adobe" + (100).to_bytes(2, "big") + b"\0\0\0\0\x02"
ycck = b"\xff\xd8" + segment(0xEE, adobe) + tables + segment(0xC0, components)
for index, encoded in enumerate(planes, 1):
    _, entropy = jpeg_parts(encoded)
    assert entropy is not None
    sos = bytes([1, index, 0, 0, 63, 0])
    ycck += segment(0xDA, sos) + entropy
ycck += b"\xff\xd9"
(ROOT / "adobe-ycck.jpg").write_bytes(ycck)
opened = Image.open(BytesIO(ycck)).convert("RGBA")
(ROOT / "adobe-ycck.rgba").write_bytes(opened.tobytes())

print(f"Pillow {__version__}: regenerated {len(cases) + 1} JPEG/RGBA pairs")
