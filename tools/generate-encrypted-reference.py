#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Generate the US-315 encrypted-document reference files with LibreOffice.

Writes `tests/encrypted-reference/`: `encrypted.{docx,doc,xlsx,xls}` (saved
with the test password `test`), their unencrypted `plain.*` twins, and two
workbooks with protection but no opening password (`doc-protected.xls`:
workbook structure protected; `sheet-protected.xls`: one sheet protected).
Microsoft Office is not involved: LibreOffice is the only producer.

The files are written by `storeToURL` through UNO, using the Python bundled
with LibreOffice (next to `soffice`); the `--convert-to` JSON filter options do
not encrypt. This script itself uses only the standard library.

The bytes LibreOffice writes are not reproducible (timestamps, random salts):
regenerating changes every encrypted file. `SHA256SUMS` freezes the stored
files. `--check` verifies them against `SHA256SUMS` and does not need
LibreOffice or regenerate anything.

    py tools/generate-encrypted-reference.py [--soffice PATH]   # regenerate
    py tools/generate-encrypted-reference.py --check
"""

from __future__ import annotations

import argparse
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIXTURE_DIR = ROOT / "tests" / "encrypted-reference"
SUMS = FIXTURE_DIR / "SHA256SUMS"
PASSWORD = "test"
PORT = 2083

TEXT = "Bonjour le monde\nDeuxieme ligne\n"
CSV = "a,b\n1,2\n3,4\n"

# Runs under the Python bundled with LibreOffice (needs `uno`).
WORKER = r'''
import os, sys, time, uno
from com.sun.star.beans import PropertyValue

def pv(n, v):
    p = PropertyValue(); p.Name = n; p.Value = v; return p

work, out, password, port = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
ctx = uno.getComponentContext()
resolver = ctx.ServiceManager.createInstanceWithContext(
    "com.sun.star.bridge.UnoUrlResolver", ctx)
conn = None
for _ in range(90):
    try:
        conn = resolver.resolve(
            "uno:socket,host=localhost,port=%s;urp;StarOffice.ComponentContext" % port)
        break
    except Exception:
        time.sleep(1)
if conn is None:
    sys.exit("cannot connect to LibreOffice")
desktop = conn.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", conn)

def save(src, name, filt, protect=None, secret=None):
    doc = desktop.loadComponentFromURL(
        uno.systemPathToFileUrl(os.path.join(work, src)), "_blank", 0,
        (pv("Hidden", True),))
    if protect == "structure":
        doc.protect("")
    elif protect == "sheet":
        doc.Sheets.getByIndex(0).protect("")
    props = [pv("FilterName", filt)]
    if secret:
        props.append(pv("Password", secret))
    doc.storeToURL(uno.systemPathToFileUrl(os.path.join(out, name)), tuple(props))
    doc.close(True)

FILTERS = [("docx", "src.txt", "MS Word 2007 XML"), ("doc", "src.txt", "MS Word 97"),
           ("xlsx", "src.csv", "Calc MS Excel 2007 XML"), ("xls", "src.csv", "MS Excel 97")]
for ext, src, filt in FILTERS:
    save(src, "plain." + ext, filt)
    save(src, "encrypted." + ext, filt, secret=password)
save("src.csv", "doc-protected.xls", "MS Excel 97", protect="structure")
save("src.csv", "sheet-protected.xls", "MS Excel 97", protect="sheet")
try:
    desktop.terminate()
except Exception:
    pass
'''


def find_soffice(given: str | None) -> Path:
    candidates = [given or "", os.environ.get("SOFFICE", ""), shutil.which("soffice") or "",
                  r"C:\Program Files\LibreOffice\program\soffice.exe",
                  "/usr/bin/soffice", "/Applications/LibreOffice.app/Contents/MacOS/soffice"]
    for c in candidates:
        if c and Path(c).exists():
            return Path(c)
    raise FileNotFoundError("LibreOffice (soffice) not found; set SOFFICE or pass --soffice")


def lo_python(soffice: Path) -> Path:
    program = soffice.resolve().parent
    for name in ("python.exe", "python", "python3"):
        if (program / name).exists():
            return program / name
    raise FileNotFoundError(f"no Python bundled with LibreOffice in {program}")


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def stored_files() -> list[Path]:
    return sorted(p for p in FIXTURE_DIR.iterdir()
                  if p.suffix in (".doc", ".docx", ".xls", ".xlsx"))


def write_sums() -> None:
    lines = [f"{sha256(p)} *{p.name}\n" for p in stored_files()]
    SUMS.write_text("".join(lines), encoding="utf-8", newline="\n")


def check() -> int:
    expected = {}
    for line in SUMS.read_text(encoding="utf-8").splitlines():
        digest, _, name = line.partition(" *")
        expected[name] = digest
    bad = 0
    names = {p.name for p in stored_files()}
    for name in sorted(names | set(expected)):
        path = FIXTURE_DIR / name
        if name not in expected:
            print(f"missing from SHA256SUMS: {name}")
            bad += 1
        elif not path.exists():
            print(f"missing file: {name}")
            bad += 1
        elif sha256(path) != expected[name]:
            print(f"checksum mismatch: {name}")
            bad += 1
    print("encrypted reference: OK" if not bad else f"encrypted reference: {bad} problem(s)")
    return 1 if bad else 0


def generate(soffice: Path) -> None:
    python = lo_python(soffice)
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        (work / "src.txt").write_text(TEXT, encoding="utf-8", newline="\n")
        (work / "src.csv").write_text(CSV, encoding="utf-8", newline="\n")
        (work / "worker.py").write_text(WORKER, encoding="utf-8")
        out = work / "out"
        out.mkdir()
        profile = (work / "profile").resolve().as_posix()
        server = subprocess.Popen(
            [str(soffice), "--headless", "--norestore", f"-env:UserInstallation=file:///{profile}",
             f"--accept=socket,host=localhost,port={PORT};urp;"],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            done = subprocess.run(
                [str(python), str(work / "worker.py"), str(work), str(out), PASSWORD, str(PORT)],
                timeout=300)
            if done.returncode != 0:
                raise RuntimeError("LibreOffice worker failed")
        finally:
            time.sleep(1)
            server.terminate()
            try:
                server.wait(timeout=20)
            except subprocess.TimeoutExpired:
                server.kill()
        FIXTURE_DIR.mkdir(parents=True, exist_ok=True)
        for produced in sorted(out.iterdir()):
            shutil.copyfile(produced, FIXTURE_DIR / produced.name)
    write_sums()
    print(f"wrote {len(stored_files())} files to {FIXTURE_DIR}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--check", action="store_true",
                        help="verify the stored files against SHA256SUMS; no LibreOffice needed")
    parser.add_argument("--soffice", help="path to soffice (default: SOFFICE, PATH, usual places)")
    args = parser.parse_args()
    if args.check:
        return check()
    generate(find_soffice(args.soffice))
    return 0


if __name__ == "__main__":
    sys.exit(main())
