# SPDX-License-Identifier: AGPL-3.0-or-later
"""Agent-only independent golden pixels: PyPNG and Pillow, never product dependencies."""
from pathlib import Path
import hashlib
import io
import json
import random
import shutil
import struct
import sys
import tarfile
import zlib

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'target/agent-python'))
import png
from PIL import Image

OUT = ROOT / 'tests/image-reference'
ARCHIVE = ROOT / 'target/images-fonts-oracle/PngSuite-2017jul19.tgz'
ARCHIVE_SHA = '0294b244c95a8342c01b00010cf34abdcabc7c6a34fd0fe1bd963917537bfdc8'
CASES = []


def record(name, kind, data, width=0, height=0, rgba=None):
    file = OUT / name
    file.parent.mkdir(parents=True, exist_ok=True)
    file.write_bytes(data)
    case = {'file': name, 'format': kind, 'sha256': hashlib.sha256(data).hexdigest(),
            'valid': rgba is not None, 'width': width, 'height': height}
    if rgba is not None:
        assert len(rgba) == width * height * 4
        ref = name + '.rgba'
        (OUT / ref).write_bytes(rgba)
        case.update(reference=ref, reference_sha256=hashlib.sha256(rgba).hexdigest())
    CASES.append(case)


def raw_png_samples(data):
    # PyPNG applies sBIT in asDirect. Remove rendering metadata to test the
    # declared raw-sample API, preserving only critical chunks and tRNS.
    out = data[:8]
    pos = 8
    while pos < len(data):
        size = int.from_bytes(data[pos:pos+4], 'big')
        kind = data[pos+4:pos+8]
        if kind in (b'IHDR', b'PLTE', b'tRNS', b'IDAT', b'IEND'):
            out += data[pos:pos+size+12]
        pos += size + 12
    w, h, rows, info = png.Reader(bytes=out).asRGBA()
    maximum = (1 << info['bitdepth']) - 1
    rgba = bytes((v * 255 + maximum // 2) // maximum for row in rows for v in row)
    return w, h, rgba


def pngsuite():
    assert hashlib.sha256(ARCHIVE.read_bytes()).hexdigest() == ARCHIVE_SHA
    with tarfile.open(ARCHIVE) as archive:
        for member in sorted(archive.getmembers(), key=lambda m: m.name):
            assert member.isfile() and Path(member.name).name == member.name
            data = archive.extractfile(member).read()
            if not member.name.endswith('.png'):
                path = OUT / 'pngsuite' / member.name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            elif member.name.startswith('x'):
                record('pngsuite/' + member.name, 'png', data)
            else:
                record('pngsuite/' + member.name, 'png', data, *raw_png_samples(data))


def gif_cases():
    rng = random.Random(43040)
    palette = [v for i in range(256) for v in (i, (i * 37) % 256, (i * 71) % 256)]
    for count, (w, h, transparent, interlace) in enumerate([
            (3, 2, False, False), (19, 23, False, True),
            (31, 25, True, True), (97, 67, False, False)]):
        image = Image.new('P', (w, h))
        image.putpalette(palette)
        image.putdata([rng.randrange(256) for _ in range(w*h)])
        options = dict(format='GIF', optimize=False, interlace=interlace)
        if transparent:
            options['transparency'] = 0
        output = io.BytesIO()
        image.save(output, **options)
        encoded = output.getvalue()
        reference = Image.open(io.BytesIO(encoded)).convert('RGBA')
        raw = bytearray(reference.tobytes())
        for i in range(0, len(raw), 4):
            if raw[i+3] == 0:
                raw[i:i+3] = b'\0\0\0'
        record(f'generated/gif-{count}.gif', 'gif', encoded, w, h, bytes(raw))
    first = Image.new('P', (5, 4), 1)
    first.putpalette(palette)
    second = Image.new('P', first.size, 2)
    second.putpalette(palette)
    output = io.BytesIO()
    first.save(output, format='GIF', save_all=True, append_images=[second], optimize=False)
    data = output.getvalue()
    record('generated/gif-multiple.gif', 'gif', data, 5, 4,
           Image.open(io.BytesIO(data)).convert('RGBA').tobytes())
    # Independently hand-encoded 1x1 frame placed in a 3x2 logical screen.
    for transparent in (False, True):
        data = b'GIF89a' + struct.pack('<HHBBB', 3, 2, 0x80, 0, 0)
        data += bytes([21, 32, 43, 210, 120, 30])
        if transparent:
            data += b'!\xf9\x04\x01\0\0\0\0'
        data += b',' + struct.pack('<HHHHB', 1, 1, 1, 1, 0)
        data += b'\x02\x02\x4c\x01\0;'
        bg = bytes([0, 0, 0, 0]) if transparent else bytes([21, 32, 43, 255])
        raw = bytearray(bg * 6)
        raw[16:20] = bytes([210, 120, 30, 255])
        pillow = Image.open(io.BytesIO(data)).convert('RGBA').tobytes()
        if not transparent:
            assert pillow == bytes(raw)
        record(f'generated/gif-offset-{int(transparent)}.gif', 'gif', data, 3, 2, bytes(raw))


def bmp(bits, w, h, rows, palette=(), compression=0, masks=(), dib_size=40):
    pal = b''.join(bytes((b, g, r, 0)) for r, g, b in palette)
    header = struct.pack('<IiiHHIIiiII', dib_size, w, h, 1, bits, compression,
                         len(rows), 0, 0, len(palette), 0)
    maskbytes = b''.join(struct.pack('<I', m) for m in masks)
    if dib_size > 40:
        header += maskbytes + bytes(dib_size - 40 - len(maskbytes))
        maskbytes = b''
    offset = 14 + len(header) + len(maskbytes) + len(pal)
    return b'BM' + struct.pack('<IHHI', offset + len(rows), 0, 0, offset) + header + maskbytes + pal + rows


def bmp_cases():
    palette = [(0, 0, 0), (255, 0, 0), (0, 255, 0), (0, 0, 255)]
    indices = [[0, 1, 1], [1, 0, 1]]
    for bits in (1, 4, 8):
        pal = palette[:2] if bits == 1 else palette
        for top in (False, True):
            rows = b''
            for row in (indices if top else indices[::-1]):
                packed = sum(v << ((len(row)-1-i)*bits) for i, v in enumerate(row))
                size = (3*bits+7)//8
                packed <<= size*8-3*bits
                raw = packed.to_bytes(size, 'big')
                rows += raw + bytes((-len(raw)) % 4)
            data = bmp(bits, 3, -2 if top else 2, rows, pal)
            rgba = bytes(c for row in indices for v in row for c in (*pal[v], 255))
            assert Image.open(io.BytesIO(data)).convert('RGBA').tobytes() == rgba
            record(f'generated/bmp-{bits}-{int(top)}.bmp', 'bmp', data, 3, 2, rgba)
    colors = [(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255), (0, 0, 0), (255, 0, 255)]
    for bits in (16, 24, 32):
        for top in (False, True):
            rows = b''
            for row in ([colors[:3], colors[3:]] if top else [colors[3:], colors[:3]]):
                raw = b''
                for r, g, b in row:
                    if bits == 16:
                        raw += struct.pack('<H', (r//8)<<10 | (g//8)<<5 | b//8)
                    elif bits == 24:
                        raw += bytes((b, g, r))
                    else:
                        raw += bytes((b, g, r, 7))  # reserved byte is not alpha
                rows += raw + bytes((-len(raw)) % 4)
            data = bmp(bits, 3, -2 if top else 2, rows)
            rgba = bytes(c for color in colors for c in (*color, 255))
            assert Image.open(io.BytesIO(data)).convert('RGBA').tobytes() == rgba
            record(f'generated/bmp-{bits}-{int(top)}.bmp', 'bmp', data, 3, 2, rgba)
    data = bmp(16, 3, 1, struct.pack('<HHHH', 0xf800, 0x07e0, 0x001f, 0),
               compression=3, masks=(0xf800, 0x07e0, 0x001f))
    record('generated/bmp-565.bmp', 'bmp', data, 3, 1,
           bytes([255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255]))
    data = bmp(32, 2, -1, bytes([7, 21, 43, 128, 210, 90, 12, 255]),
               compression=3, masks=(0xff0000, 0xff00, 0xff, 0xff000000), dib_size=108)
    record('generated/bmp-alpha.bmp', 'bmp', data, 2, 1, bytes([43, 21, 7, 128, 12, 90, 210, 255]))
    for bits, comp in ((4, 2), (8, 1)):
        # Bottom row absolute five colors + EOL, top row encoded run + EOB.
        stream = bytes([0, 5, 0x12, 0x30, 0x10, 0]) if bits == 4 else bytes([0, 5, 1, 2, 3, 0, 1, 0])
        stream += bytes([0, 0, 5, 0x22 if bits == 4 else 2, 0, 1])
        data = bmp(bits, 5, 2, stream, palette, comp)
        rgba = bytes(c for i in [2]*5+[1, 2, 3, 0, 1] for c in (*palette[i], 255))
        # Pillow 12.3.0 drops the last nibble of an odd RLE4 absolute run.
        # The RLE4 golden row is derived from the five literal indices above.
        if bits == 8:
            assert Image.open(io.BytesIO(data)).convert('RGBA').tobytes() == rgba
        record(f'generated/bmp-rle{bits}.bmp', 'bmp', data, 5, 2, rgba)
        # A delta leaves unpainted palette-zero pixels, not transparent pixels.
        stream = bytes([0, 2, 1, 0, 2, 0x11 if bits == 4 else 1, 0, 0, 5, 0x22 if bits == 4 else 2, 0, 1])
        data = bmp(bits, 5, 2, stream, palette, comp)
        rgba = bytes(c for i in [2]*5+[0, 1, 1, 0, 0] for c in (*palette[i], 255))
        record(f'generated/bmp-rle{bits}-delta.bmp', 'bmp', data, 5, 2, rgba)


def main():
    pngsuite()
    gif_cases()
    bmp_cases()
    manifest = {'pngsuite_archive_url': 'http://www.schaik.com/pngsuite/PngSuite-2017jul19.tgz',
                'pngsuite_archive_sha256': ARCHIVE_SHA,
                'oracle': 'PyPNG 0.20220715.0 raw samples (rendering metadata removed), nearest integer RGBA8; Pillow for GIF/BMP; hand-derived offset/bitfield/RLE cases',
                'cases': CASES}
    (OUT/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n', encoding='utf-8', newline='\n')
    (OUT/'cases.tsv').write_text(''.join(f"{c['format']}\t{c['file']}\t{c['width']}\t{c['height']}\t{c.get('reference','-')}\n" for c in CASES), encoding='utf-8', newline='\n')
    print(json.dumps({'cases': len(CASES), 'valid': sum(c['valid'] for c in CASES), 'pngsuite': sum(c['format']=='png' for c in CASES)}))


if __name__ == '__main__':
    main()
