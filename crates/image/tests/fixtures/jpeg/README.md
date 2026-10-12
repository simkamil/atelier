# JPEG reference corpus

The seventeen encoded inputs and their raw top-down RGBA8 references were generated
with Pillow 12.3.0 (Python package `Pillow`) using `generate.py`. Pillow's JPEG
decoder supplies the independent reference pixels. The standard source is a
deterministic 19×17 RGB gradient, which exercises partial MCU edges. Baseline
and progressive sampling cases use quality 95 and cover grayscale, 4:4:4,
4:2:2, and 4:2:0; the corpus also includes progressive grayscale and restart
markers, including a progressive restart-marker stream. Three deterministic RGB noise cases use seed 42042: baseline 4:2:0 at
quality 20, baseline 4:4:4 at quality 98, and progressive 4:2:2 at quality 90.
The `size-1x1` and `size-16x16` inputs are gradient crops encoded at quality 95
with 4:2:0 sampling. Separate cases cover Adobe CMYK and Adobe YCCK; the YCCK
input is assembled from four baseline grayscale component scans by the
generator. References are frozen in this directory; regenerating them requires
Pillow and is not part of product builds or tests. The additional
`progressive-restart-eobrun-crosses-restart.jpg` is a malformed negative case:
the generator changes the first progressive AC Huffman symbol so its EOB run
crosses the first restart marker; it has no pixel reference.
