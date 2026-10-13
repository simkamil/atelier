// SPDX-License-Identifier: AGPL-3.0-or-later
//! Protection detection (US-315): containers built in the test, real files
//! written by LibreOffice (`tests/encrypted-reference`), size independence and
//! a seeded mutation campaign.

#[path = "../../cfb/tests/common/mod.rs"]
mod cfb;

use atelier_fmt_legacy::detect::{Detection, Encryption, Family, Protection, detect_protection};
use atelier_fmt_legacy::doc::{DocError, import_doc};
use cfb::{Builder, Node, Rng, storage, stream};
use std::path::PathBuf;
use std::time::Instant;

fn build(children: Vec<Node>) -> Vec<u8> {
    Builder::new(3, children).build().bytes
}

fn d(family: Family, protection: Protection) -> Detection {
    Detection { family, protection }
}

fn version(major: u16, minor: u16) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&major.to_le_bytes());
    v.extend_from_slice(&minor.to_le_bytes());
    v.extend_from_slice(&[0x40, 0, 0, 0, 1, 2, 3, 4]);
    v
}

fn ooxml(info: &[u8]) -> Vec<u8> {
    build(vec![
        stream("EncryptionInfo", info),
        stream("EncryptedPackage", &cfb::pattern(1, 5000)),
    ])
}

fn fib(flags: u16) -> Vec<u8> {
    let mut w = vec![0u8; 64];
    w[0..2].copy_from_slice(&0xA5ECu16.to_le_bytes());
    w[2..4].copy_from_slice(&0x00C1u16.to_le_bytes());
    w[10..12].copy_from_slice(&flags.to_le_bytes());
    w
}

fn record(id: u16, body: &[u8]) -> Vec<u8> {
    let mut r = id.to_le_bytes().to_vec();
    r.extend_from_slice(&(body.len() as u16).to_le_bytes());
    r.extend_from_slice(body);
    r
}

fn biff(filepass: Option<&[u8]>) -> Vec<u8> {
    let mut b = record(0x0809, &[0, 6, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    if let Some(body) = filepass {
        b.extend(record(0x002F, body));
    }
    b.extend(record(0x00E1, &[0xB0, 4]));
    b.extend(record(0x0042, &[0xB0, 4]));
    b.extend(record(0x0031, &[0; 14]));
    b
}

fn ppt(token: u32) -> Vec<u8> {
    let mut cu = vec![0u8; 8];
    cu[2..4].copy_from_slice(&0x0FF6u16.to_le_bytes());
    cu[4..8].copy_from_slice(&20u32.to_le_bytes());
    cu.extend_from_slice(&20u32.to_le_bytes());
    cu.extend_from_slice(&token.to_le_bytes());
    cu.extend_from_slice(&[0; 12]);
    build(vec![
        stream("Current User", &cu),
        stream("PowerPoint Document", &cfb::pattern(2, 600)),
    ])
}

const OOXML: Family = Family::EncryptedOoxmlPackage;

#[test]
fn ooxml_versions() {
    let cases = [
        ((2, 2), Encryption::Standard),
        ((3, 2), Encryption::Standard),
        ((4, 2), Encryption::Standard),
        ((4, 4), Encryption::Agile),
        ((3, 3), Encryption::Extensible),
        ((4, 3), Encryption::Extensible),
        ((1, 1), Encryption::Unknown),
        ((4, 5), Encryption::Unknown),
        ((0xFFFF, 0xFFFF), Encryption::Unknown),
    ];
    for ((maj, min), enc) in cases {
        let got = detect_protection(&ooxml(&version(maj, min)));
        assert_eq!(got, d(OOXML, Protection::Encrypted(enc)), "{maj}.{min}");
        assert!(got.is_protected());
    }
}

#[test]
fn ooxml_empty_or_short_info_and_case() {
    for info in [&b""[..], &b"\x04"[..], &b"\x04\x00\x04"[..]] {
        assert_eq!(
            detect_protection(&ooxml(info)),
            d(OOXML, Protection::Encrypted(Encryption::Unknown))
        );
    }
    // Empty EncryptedPackage still counts; names compare case-insensitively.
    let bytes = build(vec![
        stream("encryptioninfo", &version(4, 4)),
        stream("ENCRYPTEDPACKAGE", b""),
    ]);
    assert_eq!(
        detect_protection(&bytes),
        d(OOXML, Protection::Encrypted(Encryption::Agile))
    );
}

#[test]
fn only_one_ooxml_stream_is_not_enough() {
    for nodes in [
        vec![stream("EncryptionInfo", &version(4, 4))],
        vec![stream("EncryptedPackage", &[1, 2, 3])],
        // A storage with the right name is not a stream.
        vec![
            storage("EncryptionInfo", vec![]),
            stream("EncryptedPackage", &[1]),
        ],
    ] {
        assert_eq!(
            detect_protection(&build(nodes)),
            d(Family::OtherCompound, Protection::Undetermined)
        );
    }
}

#[test]
fn word_flags() {
    let word = |flags| detect_protection(&build(vec![stream("WordDocument", &fib(flags))]));
    assert_eq!(word(0), d(Family::Word97, Protection::None));
    assert_eq!(word(0x0200), d(Family::Word97, Protection::None));
    assert_eq!(
        word(0x0100),
        d(Family::Word97, Protection::Encrypted(Encryption::Unknown))
    );
    assert_eq!(word(0x8100), d(Family::Word97, Protection::Obfuscated));
    // fObfuscated alone, without fEncrypted, is not a protection.
    assert_eq!(word(0x8000), d(Family::Word97, Protection::None));
    // Case of the stream name.
    let b = build(vec![stream("worddocument", &fib(0x0100))]);
    assert!(detect_protection(&b).is_protected());
}

#[test]
fn word_empty_truncated_or_wrong_identifier() {
    for data in [vec![], fib(0)[..20].to_vec(), vec![0u8; 64]] {
        assert_eq!(
            detect_protection(&build(vec![stream("WordDocument", &data)])),
            d(Family::Word97, Protection::Undetermined)
        );
    }
    // 32 bytes are enough to read the flags.
    let b = build(vec![stream("WordDocument", &fib(0x0100)[..32])]);
    assert!(detect_protection(&b).is_protected());
}

#[test]
fn word_agrees_with_import_doc() {
    for flags in [0u16, 0x0100, 0x8100] {
        let bytes = build(vec![stream("WordDocument", &fib(flags))]);
        let imported = import_doc(&bytes);
        let detected = detect_protection(&bytes);
        assert_eq!(
            matches!(imported, Err(DocError::PasswordProtected { .. })),
            detected.is_protected()
        );
    }
}

#[test]
fn excel_filepass() {
    let x = |b: &[u8]| detect_protection(&build(vec![stream("Workbook", b)]));
    assert_eq!(x(&biff(None)), d(Family::Excel97, Protection::None));
    assert_eq!(
        x(&biff(Some(&[0, 0, 1, 2, 3, 4]))),
        d(Family::Excel97, Protection::Obfuscated)
    );
    assert_eq!(
        x(&biff(Some(&[1, 0, 1, 0, 1, 0]))),
        d(Family::Excel97, Protection::Encrypted(Encryption::Rc4))
    );
    for major in [2u8, 3, 4] {
        assert_eq!(
            x(&biff(Some(&[1, 0, major, 0, 2, 0]))),
            d(
                Family::Excel97,
                Protection::Encrypted(Encryption::Rc4CryptoApi)
            )
        );
    }
    assert_eq!(
        x(&biff(Some(&[1, 0, 9, 0]))),
        d(Family::Excel97, Protection::Encrypted(Encryption::Unknown))
    );
    assert_eq!(
        x(&biff(Some(&[7, 0]))),
        d(Family::Excel97, Protection::EncryptedUnknown)
    );
    // Book (BIFF5) stream name, mixed case.
    let b = build(vec![stream("book", &biff(Some(&[1, 0, 1, 0, 1, 0])))]);
    assert!(detect_protection(&b).is_protected());
    // Empty, truncated or not starting with a BOF.
    assert_eq!(x(b""), d(Family::Excel97, Protection::Undetermined));
    assert_eq!(x(&[9, 8, 16]), d(Family::Excel97, Protection::Undetermined));
    assert_eq!(
        x(&record(0x0042, &[0, 0])),
        d(Family::Excel97, Protection::Undetermined)
    );
    let cut = biff(Some(&[0, 0, 1, 2, 3, 4]));
    assert_eq!(x(&cut[..22]), d(Family::Excel97, Protection::Undetermined));
}

#[test]
fn powerpoint_current_user() {
    assert_eq!(
        detect_protection(&ppt(0xE391_C05F)),
        d(Family::PowerPoint97, Protection::None)
    );
    assert_eq!(
        detect_protection(&ppt(0xF3D1_C4DF)),
        d(
            Family::PowerPoint97,
            Protection::Encrypted(Encryption::Unknown)
        )
    );
    assert_eq!(
        detect_protection(&ppt(7)),
        d(Family::PowerPoint97, Protection::Undetermined)
    );
    let no_user = build(vec![stream("PowerPoint Document", &[1; 100])]);
    assert_eq!(
        detect_protection(&no_user),
        d(Family::PowerPoint97, Protection::Undetermined)
    );
}

#[test]
fn rights_management() {
    let drm = build(vec![
        storage(
            "\u{6}DataSpaces",
            vec![storage(
                "TransformInfo",
                vec![storage("\u{9}DRMEncryptedTransform", vec![])],
            )],
        ),
        stream("\u{9}DRMContent", &[1, 2, 3]),
    ]);
    assert_eq!(
        detect_protection(&drm),
        d(Family::OtherCompound, Protection::RightsManaged)
    );
    // A password-encrypted package also has DataSpaces, but with the strong
    // encryption transform: it is not rights managed.
    let pw = build(vec![
        storage(
            "\u{6}DataSpaces",
            vec![storage(
                "TransformInfo",
                vec![storage("StrongEncryptionTransform", vec![])],
            )],
        ),
        stream("EncryptionInfo", &version(4, 4)),
        stream("EncryptedPackage", &[0; 8]),
    ]);
    assert_eq!(
        detect_protection(&pw),
        d(OOXML, Protection::Encrypted(Encryption::Agile))
    );
    // DataSpaces alone proves nothing.
    let alone = build(vec![storage("\u{6}DataSpaces", vec![])]);
    assert_eq!(
        detect_protection(&alone),
        d(Family::OtherCompound, Protection::Undetermined)
    );
}

#[test]
fn everything_else_is_unknown() {
    let unknown = d(Family::Unknown, Protection::Undetermined);
    assert_eq!(detect_protection(b""), unknown);
    assert_eq!(detect_protection(b"Bonjour"), unknown);
    assert_eq!(detect_protection(b"PK\x03\x04\x14\0\0\0\x08\0"), unknown);
    assert_eq!(detect_protection(&[0xD0, 0xCF, 0x11, 0xE0]), unknown);
    let cfb = ooxml(&version(4, 4));
    assert_eq!(detect_protection(&cfb[..400]), unknown);
    assert_eq!(detect_protection(&cfb[..512]), unknown);
    assert!(!unknown.is_protected());
    // An ordinary container without any mark.
    let other = build(vec![stream("Contents", &[1, 2, 3])]);
    assert_eq!(
        detect_protection(&other),
        d(Family::OtherCompound, Protection::Undetermined)
    );
}

#[test]
fn cost_does_not_depend_on_stream_size() {
    let small = ooxml(&version(4, 4));
    let big = build(vec![
        stream("EncryptionInfo", &version(4, 4)),
        stream("EncryptedPackage", &cfb::pattern(3, 96 * 1024 * 1024)),
    ]);
    assert!(big.len() > 96 * 1024 * 1024);
    let time = |data: &[u8]| {
        let t = Instant::now();
        for _ in 0..20 {
            assert!(detect_protection(data).is_protected());
        }
        t.elapsed() / 20
    };
    let (ts, tb) = (time(&small), time(&big));
    eprintln!("detect: small {ts:?}, {} MiB {tb:?}", big.len() >> 20);
    assert!(tb.as_millis() < 250, "{tb:?}");
}

// ----- real files ----------------------------------------------------------

fn reference(name: &str) -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/encrypted-reference");
    std::fs::read(p.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn libreoffice_encrypted_files() {
    // LibreOffice writes standard encryption (EncryptionInfo 3.2 or 4.2).
    let std = Protection::Encrypted(Encryption::Standard);
    for name in ["encrypted.docx", "encrypted.xlsx"] {
        assert_eq!(detect_protection(&reference(name)), d(OOXML, std), "{name}");
    }
    let doc = detect_protection(&reference("encrypted.doc"));
    assert_eq!(
        doc,
        d(Family::Word97, Protection::Encrypted(Encryption::Unknown))
    );
    let xls = detect_protection(&reference("encrypted.xls"));
    assert_eq!(
        xls,
        d(Family::Excel97, Protection::Encrypted(Encryption::Rc4))
    );
    assert!(xls.may_be_default_password());
    assert!(!doc.may_be_default_password());
    // The existing importer agrees on the real .doc.
    assert!(matches!(
        import_doc(&reference("encrypted.doc")),
        Err(DocError::PasswordProtected { .. })
    ));
}

#[test]
fn libreoffice_plain_files_are_not_flagged() {
    let doc = detect_protection(&reference("plain.doc"));
    assert_eq!(doc, d(Family::Word97, Protection::None));
    assert!(import_doc(&reference("plain.doc")).is_ok());
    let xls = detect_protection(&reference("plain.xls"));
    assert_eq!(xls, d(Family::Excel97, Protection::None));
    for name in ["plain.docx", "plain.xlsx"] {
        let got = detect_protection(&reference(name));
        assert_eq!(got, d(Family::Unknown, Protection::Undetermined), "{name}");
        assert!(!got.is_protected());
    }
}

// ----- mutation campaign ---------------------------------------------------

#[test]
fn seeded_mutations_and_truncations_never_panic() {
    let mut corpus: Vec<Vec<u8>> = vec![
        ooxml(&version(4, 4)),
        build(vec![stream("WordDocument", &fib(0x8100))]),
        build(vec![stream("Workbook", &biff(Some(&[1, 0, 2, 0, 2, 0])))]),
        ppt(0xF3D1_C4DF),
        build(vec![
            storage(
                "\u{6}DataSpaces",
                vec![storage(
                    "TransformInfo",
                    vec![storage("\u{9}DRMTransform", vec![])],
                )],
            ),
            stream("\u{9}DRMContent", &[1; 700]),
        ]),
    ];
    for name in [
        "encrypted.doc",
        "encrypted.docx",
        "encrypted.xls",
        "encrypted.xlsx",
        "plain.doc",
        "plain.docx",
    ] {
        corpus.push(reference(name));
    }
    let mut rng = Rng(0x5EED_0315);
    let mut classified = 0usize;
    for base in &corpus {
        for _ in 0..600 {
            let mut data = base.clone();
            match rng.below(3) {
                0 => {
                    let cut = rng.below(data.len() + 1);
                    data.truncate(cut);
                }
                1 => {
                    for _ in 0..1 + rng.below(8) {
                        let i = rng.below(data.len());
                        data[i] = rng.next() as u8;
                    }
                }
                _ => {
                    // Aim at the header and the directory area.
                    for _ in 0..1 + rng.below(4) {
                        let i = rng.below(data.len().min(1536));
                        data[i] ^= 1 << rng.below(8);
                    }
                }
            }
            if detect_protection(&data).family != Family::Unknown {
                classified += 1;
            }
        }
    }
    assert!(
        classified > 1000,
        "the campaign must keep reaching the parsers"
    );
    // Random bytes behind a valid signature.
    for _ in 0..500 {
        let mut data = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        for _ in 0..rng.below(1200) {
            data.push(rng.next() as u8);
        }
        let _ = detect_protection(&data);
    }
}

#[test]
fn excel_filepass_after_interfacehdr_or_writeprot() {
    let x = |records: Vec<Vec<u8>>| {
        detect_protection(&build(vec![stream("Workbook", &records.concat())]))
    };
    let bof = record(0x0809, &[0, 6, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    let pass = record(0x002F, &[1, 0, 1, 0, 1, 0]);
    let rc4 = d(Family::Excel97, Protection::Encrypted(Encryption::Rc4));
    let orders = [
        vec![bof.clone(), record(0x0086, &[0, 0]), pass.clone()],
        vec![bof.clone(), record(0x00E1, &[0xB0, 4]), pass.clone()],
        vec![
            bof.clone(),
            record(0x00E1, &[0xB0, 4]),
            record(0x00C1, &[0, 0]),
            record(0x00E2, &[]),
            record(0x005C, &[0; 8]),
            record(0x0042, &[0xB0, 4]),
            pass.clone(),
        ],
    ];
    for records in orders {
        let got = x(records);
        assert_eq!(got, rc4);
        assert!(got.is_protected());
    }
    // Header records and then the body without FILEPASS: not protected.
    let plain = x(vec![
        bof.clone(),
        record(0x00E1, &[0xB0, 4]),
        record(0x0042, &[0xB0, 4]),
        record(0x0085, &[0; 8]),
        pass,
    ]);
    assert_eq!(plain, d(Family::Excel97, Protection::None));
    // A header cut short before any marker cannot be classified.
    let cut = x(vec![bof, record(0x00E1, &[0xB0, 4])]);
    assert_eq!(cut, d(Family::Excel97, Protection::Undetermined));
}

#[test]
fn default_password_hint() {
    let rc4 = d(Family::Excel97, Protection::Encrypted(Encryption::Rc4));
    assert!(rc4.is_protected() && rc4.may_be_default_password());
    let word = d(Family::Word97, Protection::Encrypted(Encryption::Unknown));
    assert!(word.is_protected() && !word.may_be_default_password());
    let none = d(Family::Excel97, Protection::None);
    assert!(!none.may_be_default_password());
}

#[test]
fn libreoffice_protected_structure_workbooks() {
    // Workbook structure protected without opening password: LibreOffice
    // writes a FILEPASS record (default-password encryption), so the file is
    // reported protected but flagged as possibly openable without a password.
    let doc = detect_protection(&reference("doc-protected.xls"));
    assert_eq!(
        doc,
        d(Family::Excel97, Protection::Encrypted(Encryption::Rc4))
    );
    assert!(doc.is_protected() && doc.may_be_default_password());
    // Sheet protection alone writes no FILEPASS.
    let sheet = detect_protection(&reference("sheet-protected.xls"));
    assert_eq!(sheet, d(Family::Excel97, Protection::None));
}
