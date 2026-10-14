#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""US-307: make LibreOffice open every file the DOCX writer produces.

1. run the Rust example `write_docx_samples`, which writes (under --samples):
   the round trip of the corpus and of built documents, 120 documents created
   from scratch, and the same kinds of documents after random edits;
2. convert every file with LibreOffice (headless) to text, and to PDF so that
   the whole document is loaded and laid out;
3. compare the text with the one the model holds (new and edited documents) or
   with the text LibreOffice reads from the source file (round trip).

LibreOffice is a tolerant reader: that it opens a file does not prove that
Word accepts it. Word is not available on the machines of the project.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BATCH = 25


def soffice() -> str:
    for candidate in (
        os.environ.get("ATELIER_SOFFICE"),
        shutil.which("soffice"),
        r"C:\Program Files\LibreOffice\program\soffice.exe",
    ):
        if candidate and Path(candidate).exists():
            return candidate
    sys.exit("LibreOffice (soffice) not found; set ATELIER_SOFFICE")


def convert(exe: str, files: list[Path], fmt: str, outdir: Path, profile: Path) -> None:
    """Converts files in batches with one LibreOffice process per batch."""
    outdir.mkdir(parents=True, exist_ok=True)
    for i in range(0, len(files), BATCH):
        batch = files[i : i + BATCH]
        cmd = [
            exe,
            f"-env:UserInstallation={profile.as_uri()}",
            "--headless",
            "--convert-to",
            fmt,
            "--outdir",
            str(outdir),
            *map(str, batch),
        ]
        try:
            subprocess.run(cmd, check=False, capture_output=True, timeout=600)
        except subprocess.TimeoutExpired:
            print(f"  LibreOffice timed out on a batch of {len(batch)} files", file=sys.stderr)


def words(text: str) -> list[str]:
    text = text.replace("\ufeff", "").replace("\u00ad", "").replace("\u2011", "-")
    return [w for w in text.split() if w != "\u2022"]


def read_words(path: Path) -> list[str]:
    return words(path.read_text(encoding="utf-8", errors="replace"))


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--samples", type=Path, default=ROOT / "target" / "tmp" / "docx-writer-samples")
    ap.add_argument("--no-build", action="store_true", help="reuse the files already written")
    args = ap.parse_args()
    samples = args.samples.resolve()
    if not args.no_build:
        if samples.exists():
            shutil.rmtree(samples)
        samples.mkdir(parents=True)
        subprocess.run(
            ["cargo", "run", "-q", "-p", "atelier-fmt-ooxml", "--example", "write_docx_samples",
             "--locked", "--offline", "--", str(samples)],
            cwd=ROOT, check=True,
        )
    exe = soffice()
    entries = []
    for line in (samples / "manifest.tsv").read_text(encoding="utf-8").splitlines():
        kind, file, expected = line.split("\t")
        entries.append((kind, samples / file, samples / expected))

    with tempfile.TemporaryDirectory() as tmp:
        tmp = Path(tmp)
        profile = tmp / "profile"
        txt = tmp / "txt"
        pdf = tmp / "pdf"
        files = [e[1] for e in entries]
        sources = sorted({e[2] for e in entries if e[0] == "roundtrip"})
        convert(exe, files, "txt:Text (encoded):UTF8", txt, profile)
        convert(exe, files, "pdf", pdf, profile)
        txt_src = tmp / "txt-src"
        convert(exe, sources, "txt:Text (encoded):UTF8", txt_src, profile)

        opened = {"roundtrip": [0, 0], "new": [0, 0], "modified": [0, 0]}
        loaded = {"roundtrip": 0, "new": 0, "modified": 0}
        failures: list[str] = []
        mismatches: list[str] = []
        for kind, file, expected in entries:
            opened[kind][1] += 1
            out_txt = txt / (file.stem + ".txt")
            out_pdf = pdf / (file.stem + ".pdf")
            if not out_txt.exists():
                failures.append(f"{kind} {file.name}: no text export")
                continue
            opened[kind][0] += 1
            if out_pdf.exists() and out_pdf.stat().st_size > 0:
                loaded[kind] += 1
            else:
                failures.append(f"{kind} {file.name}: no PDF export")
            got = read_words(out_txt)
            if kind == "roundtrip":
                src_txt = txt_src / (expected.stem + ".txt")
                if not src_txt.exists():
                    failures.append(f"{kind} {expected.name}: source not readable by LibreOffice")
                    continue
                want = read_words(src_txt)
            else:
                want = words(expected.read_text(encoding="utf-8"))
            if got != want:
                diff = next((i for i, (a, b) in enumerate(zip(got, want)) if a != b), min(len(got), len(want)))
                mismatches.append(
                    f"{kind} {file.name}: {len(got)} words read, {len(want)} expected; first difference at {diff}: "
                    f"{got[diff:diff + 3]} vs {want[diff:diff + 3]}"
                )

    total = sum(v[1] for v in opened.values())
    ok = sum(v[0] for v in opened.values())
    print(f"LibreOffice opened {ok} of {total} files (text export), PDF produced for {sum(loaded.values())}")
    for kind, (o, t) in opened.items():
        print(f"  {kind}: {o}/{t} opened, {loaded[kind]} laid out to PDF")
    print(f"text differences: {len(mismatches)}")
    for line in mismatches[:40]:
        print("  " + line)
    for line in failures[:40]:
        print("  FAILED " + line)
    return 0 if ok == total and not mismatches and not failures else 1


if __name__ == "__main__":
    sys.exit(main())
