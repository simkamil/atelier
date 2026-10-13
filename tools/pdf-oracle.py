# SPDX-License-Identifier: AGPL-3.0-or-later
"""External oracles for PDF files written by atelier-fmt-pdf (US-331).

Local checks only, never run by `cargo test`. Python standard library, plus
whatever is present on the machine and used when found (nothing is installed):

  * PyMuPDF (`import pymupdf`, or the old name `fitz`): opens the file and
    reports every MuPDF warning, extracts the text, renders a page to PNG;
  * `pdftotext` (poppler): second text extraction;
  * fontTools: re-reads every embedded font program;
  * LibreOffice (`soffice`): imports the PDF headless and converts it to PNG.

    python tools/pdf-oracle.py file.pdf [--expect-text expected.txt]
        [--png page.png] [--page N] [--dpi 96] [--libreoffice] [--keep DIR]

Exit status: 0 when every available oracle is satisfied, 1 when one failed,
2 on a usage error. A missing tool is reported as `skipped`, not as a failure.
"""

import argparse
import io
import os
import shutil
import subprocess
import sys
import tempfile
import unicodedata


def load_pymupdf():
    try:
        import pymupdf as module  # noqa: PLC0415
    except ImportError:
        try:
            import fitz as module  # noqa: PLC0415
        except ImportError:
            return None
    return module


def normalize(text):
    """Lines of `text` with layout differences removed: NFC, single spaces, no blank lines."""
    text = unicodedata.normalize("NFC", text.replace("\u00a0", " ").replace("\f", "\n"))
    lines = [" ".join(line.split()) for line in text.splitlines()]
    return [line for line in lines if line]


def words(text):
    return [w for line in normalize(text) for w in line.split(" ")]


def report(name, ok, detail=""):
    print(f"[{'ok' if ok else 'FAIL':>4}] {name}" + (f": {detail}" if detail else ""))
    return ok


def skipped(name, why):
    print(f"[skip] {name}: {why}")


def compare_text(name, got, expected):
    """Compares what the PDF yields with the source text.

    The check that decides is the multiset of characters without white space:
    it cannot hide a lost, wrong or invented character. Word splitting and
    reading order depend on the extractor's layout analysis (rotated text,
    columns), so they are reported but do not fail the check.
    """
    if expected is None:
        return report(name, True, f"{len(words(got))} words extracted")
    g, e = words(got), words(expected)

    def chars(ws):
        return sorted("".join(ws))

    notes = []
    if g == e:
        notes.append("words and order identical")
    else:
        if sorted(g) == sorted(e):
            notes.append("same words, other reading order")
        else:
            notes.append(f"{len(g)} words vs {len(e)} (extractor splits or joins words)")
        extracted = set(normalize(got))
        want = normalize(expected)
        found = sum(1 for line in want if line in extracted)
        notes.append(f"{found}/{len(want)} source lines extracted verbatim")
    ok = chars(g) == chars(e)
    if not ok:
        cg, ce = chars(g), chars(e)
        lost = [c for c in set(ce) if ce.count(c) > cg.count(c)]
        extra = [c for c in set(cg) if cg.count(c) > ce.count(c)]
        notes.append(f"lost {lost} unexpected {extra}")
    return report(name, ok,
                  f"{len(e)} source words, {len(chars(e))} characters; " + "; ".join(notes))


def run_pymupdf(mu, path, expected, png, page_number, dpi):
    ok = True
    mu.TOOLS.mupdf_warnings()  # clear
    try:
        doc = mu.open(path)
    except Exception as exc:  # noqa: BLE001
        return report("PyMuPDF open", False, str(exc)), None
    ok &= report("PyMuPDF open", True, f"{doc.page_count} page(s), {doc.metadata.get('format')}")
    text, links = "", []
    for page in doc:
        text += page.get_text("text") + "\f"
        page.get_pixmap(dpi=24)  # force the whole content stream through the renderer
        links += page.get_links()
    if png:
        page = doc[page_number]
        pix = page.get_pixmap(dpi=int(round(dpi)), alpha=False)
        pix.save(png)
        ok &= report("PyMuPDF render", True, f"{png} {pix.width}x{pix.height}")
    toc = doc.get_toc()
    kinds = sorted({link.get("kind") for link in links})
    report("PyMuPDF structure", True,
           f"{len(toc)} bookmark(s) {[t[1] for t in toc][:6]}, {len(links)} link(s) kinds {kinds}")
    warnings = mu.TOOLS.mupdf_warnings().strip()
    ok &= report("PyMuPDF warnings", not warnings, warnings.replace("\n", " | ") or "none")
    ok &= compare_text("PyMuPDF text", text, expected)
    return ok, doc


def run_pdftotext(path, expected):
    exe = shutil.which("pdftotext")
    if not exe:
        skipped("pdftotext", "not found")
        return True
    with tempfile.TemporaryDirectory() as tmp:
        out = os.path.join(tmp, "out.txt")
        run = subprocess.run(
            [exe, "-enc", "UTF-8", path, out], capture_output=True, text=True, check=False
        )
        ok = report("pdftotext messages", run.returncode == 0 and not run.stderr.strip(),
                    run.stderr.strip() or "none")
        if os.path.exists(out):
            with open(out, encoding="utf-8") as f:
                ok &= compare_text("pdftotext text", f.read(), expected)
    return ok


def run_fonttools(doc):
    try:
        from fontTools.cffLib import CFFFontSet  # noqa: PLC0415
        from fontTools.ttLib import TTFont  # noqa: PLC0415
    except ImportError:
        skipped("fontTools", "not installed")
        return True
    if doc is None:
        return True
    ok, seen = True, set()
    for page in doc:
        for font_info in page.get_fonts(full=True):
            xref, ext, kind, basefont = font_info[:4]
            if xref in seen:
                continue
            seen.add(xref)
            data = doc.extract_font(xref)[3]
            if not data:
                ok &= report(f"fontTools {basefont}", False, "no font program")
                continue
            try:
                if data[:4] in (b"\x00\x01\x00\x00", b"OTTO", b"true", b"ttcf"):
                    font = TTFont(io.BytesIO(data), lazy=False)
                    glyph_set = font.getGlyphSet()
                    order = font.getGlyphOrder()
                    for name in order:  # decompiles every glyph (glyf or CFF)
                        glyph_set[name]
                    tables = sorted(t for t in font.keys() if t != "GlyphOrder")
                    detail = f"{len(order)} glyphs, tables {','.join(tables)}"
                else:  # a bare CFF program (CIDFontType0C)
                    cff = CFFFontSet()
                    cff.decompile(io.BytesIO(data), None)
                    top = cff[cff.keys()[0]]
                    for name in top.charset:  # decompiles every charstring
                        top.CharStrings[name].calcBounds(None)
                    detail = (f"bare CFF, {len(top.charset)} glyphs, "
                              f"{'CID-keyed' if hasattr(top, 'ROS') else 'name-keyed'}")
                ok &= report(f"fontTools {basefont}", True, f"{ext}/{kind}, {detail}")
            except Exception as exc:  # noqa: BLE001
                ok &= report(f"fontTools {basefont}", False, str(exc))
    if not seen:
        report("fontTools", True, "no embedded font in the document")
    return ok


def run_libreoffice(path, keep):
    exe = shutil.which("soffice") or next(
        (p for p in (r"C:\Program Files\LibreOffice\program\soffice.exe",
                     "/usr/bin/soffice", "/Applications/LibreOffice.app/Contents/MacOS/soffice")
         if os.path.exists(p)), None)
    if not exe:
        skipped("LibreOffice", "not found")
        return True
    with tempfile.TemporaryDirectory() as tmp:
        profile = os.path.join(tmp, "profile").replace("\\", "/")
        out = os.path.join(tmp, "out")
        os.makedirs(out)
        cmd = [exe, f"-env:UserInstallation=file:///{profile.lstrip('/')}", "--headless",
               "--infilter=draw_pdf_import", "--convert-to", "png", "--outdir", out, path]
        try:
            run = subprocess.run(cmd, capture_output=True, text=True, timeout=180, check=False)
        except subprocess.TimeoutExpired:
            return report("LibreOffice import", False, "timeout after 180 s")
        pngs = [f for f in os.listdir(out) if f.endswith(".png")]
        message = " ".join((run.stdout + run.stderr).split())
        ok = report("LibreOffice import", run.returncode == 0 and bool(pngs),
                    message or "no message")
        if pngs and keep:
            shutil.copy(os.path.join(out, pngs[0]), os.path.join(keep, "libreoffice.png"))
    return ok


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("pdf")
    parser.add_argument("--expect-text", help="UTF-8 file with the text the PDF should yield")
    parser.add_argument("--png", help="render --page of the PDF to this PNG")
    parser.add_argument("--page", type=int, default=0, help="page to render (0-based)")
    parser.add_argument("--dpi", type=float, default=96.0)
    parser.add_argument("--libreoffice", action="store_true", help="also import with LibreOffice")
    parser.add_argument("--keep", help="directory for the LibreOffice PNG")
    args = parser.parse_args()
    if not os.path.isfile(args.pdf):
        print(f"no such file: {args.pdf}", file=sys.stderr)
        return 2
    expected = None
    if args.expect_text:
        with open(args.expect_text, encoding="utf-8") as f:
            expected = f.read()
    ok = True
    mu = load_pymupdf()
    doc = None
    if mu is None:
        skipped("PyMuPDF", "not installed")
    else:
        good, doc = run_pymupdf(mu, args.pdf, expected, args.png, args.page, args.dpi)
        ok &= bool(good)
    ok &= run_pdftotext(args.pdf, expected)
    if mu is not None:
        ok &= run_fonttools(doc)
    if args.libreoffice:
        ok &= run_libreoffice(args.pdf, args.keep)
    print("result:", "all available oracles agree" if ok else "FAILED")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
