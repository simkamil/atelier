#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Create and COM-verify small native MS-CFB reference files on Windows."""

from __future__ import annotations

import argparse
import ctypes
import hashlib
import json
import tempfile
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "tests" / "cfb-reference"
STGM_READWRITE = 0x00000002
STGM_SHARE_EXCLUSIVE = 0x00000010
STGM_CREATE = 0x00001000
STGFMT_DOCFILE = 5
def guid(raw: bytes) -> ctypes.Array:
    return (ctypes.c_byte * 16).from_buffer_copy(raw)


IID_STORAGE = guid(uuid.UUID("0000000b-0000-0000-c000-000000000046").bytes_le)


class StgOptions(ctypes.Structure):
    _fields_ = [
        ("usVersion", ctypes.c_ushort),
        ("reserved", ctypes.c_ushort),
        ("ulSectorSize", ctypes.c_uint),
    ]


def method(ptr: int, slot: int, result, *args):
    table = ctypes.cast(ptr, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p))).contents
    return ctypes.WINFUNCTYPE(result, ctypes.c_void_p, *args)(table[slot])


def check_hresult(hr: int, what: str) -> None:
    if hr & 0x80000000:
        raise OSError(f"{what} failed: HRESULT 0x{hr & 0xffffffff:08x}")


def release(ptr: int) -> None:
    if ptr:
        method(ptr, 2, ctypes.c_ulong)(ptr)


def write_stream(storage: int, name: str, payload: bytes) -> None:
    stream = ctypes.c_void_p()
    create = method(
        storage,
        3,
        ctypes.c_long,
        ctypes.c_wchar_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.POINTER(ctypes.c_void_p),
    )
    check_hresult(create(storage, name, STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                 0, 0, ctypes.byref(stream)), f"CreateStream({name})")
    try:
        if payload:
            buf = ctypes.create_string_buffer(payload)
            written = ctypes.c_ulong()
            write = method(stream.value, 4, ctypes.c_long, ctypes.c_void_p,
                           ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong))
            check_hresult(write(stream.value, buf, len(payload), ctypes.byref(written)),
                  f"IStream::Write({name})")
            if written.value != len(payload):
                raise OSError(f"short write for {name}: {written.value}/{len(payload)}")
        commit = method(stream.value, 8, ctypes.c_long, ctypes.c_uint)
        check_hresult(commit(stream.value, 0), f"IStream::Commit({name})")
    finally:
        release(stream.value)


def create_storage(parent: int, name: str) -> int:
    child = ctypes.c_void_p()
    create = method(
        parent,
        5,
        ctypes.c_long,
        ctypes.c_wchar_p,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.c_uint,
        ctypes.POINTER(ctypes.c_void_p),
    )
    check_hresult(create(parent, name, STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                 0, 0, ctypes.byref(child)), f"CreateStorage({name})")
    return child.value


def pattern(tag: int, length: int) -> bytes:
    return bytes((i * 31 + tag * 17) % 251 for i in range(length))


def expected_streams() -> dict[str, bytes]:
    return {
        "empty": b"",
        "mini": pattern(1, 300),
        "boundary4095": pattern(2, 4095),
        "boundary4096": pattern(3, 4096),
        "multi8193": pattern(4, 8193),
        "Unicode/Δοκιμή/東京": pattern(5, 73),
    }


def create_file(path: Path, sector_size: int) -> None:
    ole32 = ctypes.WinDLL("ole32", use_last_error=True)
    check_hresult(ole32.CoInitialize(None), "CoInitialize")
    try:
        opts = StgOptions(1, 0, sector_size)
        root = ctypes.c_void_p()
        create = ole32.StgCreateStorageEx
        create.restype = ctypes.c_long
        create.argtypes = [ctypes.c_wchar_p, ctypes.c_uint, ctypes.c_uint,
                           ctypes.c_uint, ctypes.POINTER(StgOptions), ctypes.c_void_p,
                           ctypes.POINTER(ctypes.c_byte), ctypes.POINTER(ctypes.c_void_p)]
        check_hresult(create(str(path), STGM_CREATE | STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                     STGFMT_DOCFILE, 0, ctypes.byref(opts), None,
                     ctypes.cast(IID_STORAGE, ctypes.POINTER(ctypes.c_byte)),
                     ctypes.byref(root)), "StgCreateStorageEx")
        try:
            streams = expected_streams()
            for name in ("empty", "mini", "boundary4095", "boundary4096", "multi8193"):
                write_stream(root.value, name, streams[name])
            unicode_storage = create_storage(root.value, "Unicode")
            try:
                nested = create_storage(unicode_storage, "Δοκιμή")
                try:
                    write_stream(nested, "東京", streams["Unicode/Δοκιμή/東京"])
                    commit = method(nested, 9, ctypes.c_long, ctypes.c_uint)
                    check_hresult(commit(nested, 0), "nested IStorage::Commit")
                finally:
                    release(nested)
                commit = method(unicode_storage, 9, ctypes.c_long, ctypes.c_uint)
                check_hresult(commit(unicode_storage, 0), "Unicode IStorage::Commit")
            finally:
                release(unicode_storage)
            commit = method(root.value, 9, ctypes.c_long, ctypes.c_uint)
            check_hresult(commit(root.value, 0), "root IStorage::Commit")
        finally:
            release(root.value)
    finally:
        ole32.CoUninitialize()


def validate_header(data: bytes, expected_sector_size: int, label: str) -> tuple[int, int]:
    if len(data) < 512:
        raise AssertionError(f"{label}: truncated CFB header")
    major = int.from_bytes(data[26:28], "little")
    shift = int.from_bytes(data[30:32], "little")
    sector_size = 1 << shift if shift < 32 else 0
    expected_major = 3 if expected_sector_size == 512 else 4
    if major != expected_major:
        raise AssertionError(f"{label}: CFB major version {major}, expected {expected_major}")
    if sector_size != expected_sector_size:
        raise AssertionError(
            f"{label}: header sector size {sector_size}, expected {expected_sector_size}"
        )
    if len(data) < sector_size:
        raise AssertionError(f"{label}: file is shorter than its declared sector size")
    return major, sector_size


def validate_manifest_entry(entry: dict, actual: dict, filename: str) -> None:
    for field in ("major_version", "sector_size"):
        if entry.get(field) != actual[field]:
            raise AssertionError(
                f"{filename}: manifest {field} {entry.get(field)!r} "
                f"does not match header {actual[field]!r}"
            )


def verify_file(path: Path, sector_size: int) -> dict:
    data = path.read_bytes()
    major, actual_sector_size = validate_header(data, sector_size, str(path))
    # Reopen through Structured Storage and compare bytes yielded by IStream::Read.
    ole32 = ctypes.WinDLL("ole32", use_last_error=True)
    root = ctypes.c_void_p()
    open_storage = ole32.StgOpenStorageEx
    open_storage.restype = ctypes.c_long
    open_storage.argtypes = [ctypes.c_wchar_p, ctypes.c_uint, ctypes.c_uint,
                             ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p,
                             ctypes.POINTER(ctypes.c_byte), ctypes.POINTER(ctypes.c_void_p)]
    check_hresult(open_storage(str(path), STGM_READWRITE | STGM_SHARE_EXCLUSIVE,
                       STGFMT_DOCFILE, 0, None, None,
                       ctypes.cast(IID_STORAGE, ctypes.POINTER(ctypes.c_byte)),
                       ctypes.byref(root)), "StgOpenStorageEx")
    try:
        read_expected = expected_streams()
        hashes = {}
        for path_name, expected in read_expected.items():
            pieces = path_name.split("/")
            current = root.value
            owned = []
            try:
                for storage_name in pieces[:-1]:
                    child = ctypes.c_void_p()
                    open_child = method(current, 6, ctypes.c_long, ctypes.c_wchar_p,
                                        ctypes.c_void_p, ctypes.c_uint, ctypes.c_void_p,
                                        ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p))
                    check_hresult(open_child(current, storage_name, None, STGM_READWRITE |
                                     STGM_SHARE_EXCLUSIVE, None, 0, ctypes.byref(child)),
                          f"OpenStorage({storage_name})")
                    current = child.value
                    owned.append(current)
                stream = ctypes.c_void_p()
                open_stream = method(current, 4, ctypes.c_long, ctypes.c_wchar_p,
                                     ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint,
                                     ctypes.POINTER(ctypes.c_void_p))
                check_hresult(open_stream(current, pieces[-1], None, STGM_READWRITE |
                                  STGM_SHARE_EXCLUSIVE, 0, ctypes.byref(stream)),
                      f"OpenStream({path_name})")
                try:
                    got = bytearray()
                    while True:
                        buf = ctypes.create_string_buffer(4096)
                        count = ctypes.c_ulong()
                        read = method(stream.value, 3, ctypes.c_long, ctypes.c_void_p,
                                      ctypes.c_ulong, ctypes.POINTER(ctypes.c_ulong))
                        check_hresult(read(stream.value, buf, len(buf), ctypes.byref(count)),
                              f"IStream::Read({path_name})")
                        if not count.value:
                            break
                        got.extend(buf.raw[:count.value])
                    if bytes(got) != expected:
                        raise AssertionError(f"COM read-back mismatch for {path_name}")
                    hashes[path_name] = hashlib.sha256(got).hexdigest()
                finally:
                    release(stream.value)
            finally:
                for ptr in reversed(owned):
                    release(ptr)
        return {"major_version": major, "sector_size": actual_sector_size,
                "file_sha256": hashlib.sha256(data).hexdigest(),
                "streams": {p: {"size": len(b), "sha256": hashes[p]}
                            for p, b in read_expected.items()}}
    finally:
        release(root.value)


def generate(output: Path) -> dict:
    output.mkdir(parents=True, exist_ok=True)
    files = {}
    for version, sector_size in ((3, 512), (4, 4096)):
        name = f"native-v{version}.cfb"
        path = output / name
        create_file(path, sector_size)
        files[name] = verify_file(path, sector_size)
    manifest = {
        "schema_version": 1,
        "generator": "tools/generate-cfb-reference.py",
        "provenance": "Microsoft Structured Storage (ole32 StgCreateStorageEx/IStorage/IStream)",
        "verification": "fixtures reopened by StgOpenStorageEx and stream bytes/hash checked",
        "files": files,
    }
    (output / "expected.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
                                           encoding="utf-8")
    return manifest


def check(output: Path) -> None:
    with tempfile.TemporaryDirectory(prefix="atelier-cfb-check-") as temp:
        generated = generate(Path(temp))
        checked = json.loads((output / "expected.json").read_text(encoding="utf-8"))
        for filename, entry in generated["files"].items():
            stored = verify_file(output / filename, entry["sector_size"])
            if stored["major_version"] != entry["major_version"] or stored["sector_size"] != entry["sector_size"]:
                raise AssertionError(f"{filename}: regenerated header mismatch")
            validate_manifest_entry(checked["files"][filename], stored, filename)
            if stored["streams"] != entry["streams"]:
                raise AssertionError(f"{filename}: stream reference mismatch")
            if checked["files"][filename]["streams"] != stored["streams"]:
                raise AssertionError(f"{filename}: checked-in expected.json mismatch")
    print("native CFB reference fixtures pass COM regeneration/read-back checks")


def self_check() -> None:
    fixture = (OUT / "native-v3.cfb").read_bytes()
    validate_header(fixture, 512, "self-check fixture")
    broken_header = bytearray(fixture)
    broken_header[30:32] = (12).to_bytes(2, "little")
    try:
        validate_header(bytes(broken_header), 512, "self-check altered header")
    except AssertionError:
        pass
    else:
        raise AssertionError("header sector-shift self-check did not reject corruption")
    actual = {"major_version": 3, "sector_size": 512}
    try:
        validate_manifest_entry({"major_version": 4, "sector_size": 4096}, actual, "self-check")
    except AssertionError:
        pass
    else:
        raise AssertionError("manifest version/sector-size self-check did not reject mismatch")
    print("CFB reference metadata self-check passed")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true", help="regenerate temporarily and COM-verify checked-in files")
    parser.add_argument("--self-check", action="store_true", help="exercise header and manifest mismatch rejection")
    parser.add_argument("--output", type=Path, default=OUT)
    args = parser.parse_args()
    if args.self_check:
        self_check()
    elif args.check:
        check(args.output)
    else:
        manifest = generate(args.output)
        print(json.dumps({"files": list(manifest["files"]), "output": str(args.output)}, indent=2))


if __name__ == "__main__":
    main()
