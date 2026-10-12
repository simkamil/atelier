# CFB reference files from Microsoft Structured Storage

`native-v3.cfb` and `native-v4.cfb` were created on Windows by
`tools/generate-cfb-reference.py` through `ole32!StgCreateStorageEx`,
`IStorage::CreateStream`/`CreateStorage`, and `IStream::Write`. The generator
reopens each file with `StgOpenStorageEx` and reads every stream back through
`IStream::Read` before recording its SHA-256 and size in `expected.json`.

Each file contains empty, 300-byte, 4095-byte, 4096-byte, and 8193-byte
streams, plus a 73-byte stream nested under `Unicode/Δοκιμή/東京`. Stream bytes
come from the deterministic pattern encoded in the generator and are checked
byte-for-byte by `crates/cfb/tests/reference.rs`. V3 uses 512-byte sectors;
V4 uses 4096-byte sectors.

On Windows, regenerate with `py tools/generate-cfb-reference.py`. Run
`py tools/generate-cfb-reference.py --check` to create fresh temporary
Structured Storage files, verify their COM read-back, and compare their stream
hashes with the checked-in fixtures. Whole-file hashes are recorded for
provenance but are intentionally not compared because Structured Storage can
write variable metadata such as timestamps.

These are small local conformance references, not a broad real-world CFB
corpus. The Rust test validates container parsing, paths, and stream bytes; the
COM generator provides the independent creation and read-back check.
