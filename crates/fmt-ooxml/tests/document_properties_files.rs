// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-328: what the package writes (no generated `nsN:` prefixes anywhere),
//! operations that touch each other, and a seeded property test of the
//! in-place writer.
use atelier_fmt_ooxml::{
    Package, PartUri, PropertyChanges, PropertyNotice, SaveContext, TargetMode, W3cDateTime,
};
use atelier_zip::{Archive, Limits as ZipLimits, Method, WriteOptions, Writer};

const MAIN: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const CORE_CT: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_CT: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const CORE_NS_DECLS: &str = "xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"";
const APP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

fn core_xml(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties {CORE_NS_DECLS}>{body}</cp:coreProperties>"
    )
}

fn base() -> Package {
    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        b"<w:document xmlns:w=\"urn:word\"/>".to_vec(),
    )
    .unwrap();
    p.add_relationship(
        None,
        "rId1",
        MAIN,
        "word/document.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p
}

fn with_parts(core: Option<&str>, app: Option<&str>) -> Package {
    let mut p = base();
    if let Some(c) = core {
        p.insert_part(
            PartUri::parse("docProps/core.xml").unwrap(),
            CORE_CT,
            c.as_bytes().to_vec(),
        )
        .unwrap();
        p.add_relationship(
            None,
            "rId2",
            "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties",
            "docProps/core.xml",
            TargetMode::Internal,
        )
        .unwrap();
    }
    if let Some(a) = app {
        p.insert_part(
            PartUri::parse("docProps/app.xml").unwrap(),
            APP_CT,
            a.as_bytes().to_vec(),
        )
        .unwrap();
        p.add_relationship(
            None,
            "rId3",
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties",
            "docProps/app.xml",
            TargetMode::Internal,
        )
        .unwrap();
    }
    Package::read(&p.write().unwrap()).unwrap()
}

fn member(zip: &[u8], name: &str) -> String {
    let archive = Archive::open(zip, ZipLimits::default()).unwrap();
    let entry = archive
        .entries()
        .iter()
        .find(|e| e.name() == name)
        .unwrap_or_else(|| panic!("missing member {name}"));
    String::from_utf8(entry.read_all().unwrap()).unwrap()
}

fn part(p: &Package, path: &str) -> String {
    String::from_utf8(
        p.part(&PartUri::parse(path).unwrap())
            .unwrap()
            .bytes
            .clone(),
    )
    .unwrap()
}

/// True when the text contains a generated prefix such as `ns0:` or `ns12:`.
fn has_generated_prefix(text: &str) -> bool {
    let b = text.as_bytes();
    (0..b.len().saturating_sub(3)).any(|i| {
        b[i] == b'n'
            && b[i + 1] == b's'
            && b[i + 2].is_ascii_digit()
            && !(i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_'))
            && {
                let mut j = i + 2;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                j < b.len() && (b[j] == b':' || b[j] == b'=')
            }
    })
}

fn repack(zip: &[u8], f: impl Fn(&str, Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let archive = Archive::open(zip, ZipLimits::default()).unwrap();
    let mut w = Writer::new(WriteOptions::default());
    for e in archive.entries() {
        let data = f(e.name(), e.read_all().unwrap());
        w.add(e.name(), &data, Method::Store).unwrap();
    }
    w.finish().unwrap()
}

// ---- Point 1: default namespace in the OPC parts ---------------------------

#[test]
fn opc_parts_use_the_default_namespace_declared_once() {
    let mut p = base();
    p.add_relationship(
        None,
        "rId9",
        "http://example.org/link",
        "https://example.org/a&b?c=d",
        TargetMode::External,
    )
    .unwrap();
    // Property parts created from scratch, as a new document would.
    p.apply_property_changes(&PropertyChanges {
        title: Some("Titre".into()),
        company: Some("ACME".into()),
        ..Default::default()
    })
    .unwrap();
    p.update_on_save(&SaveContext {
        now: W3cDateTime::utc(2026, 1, 2, 3, 4, 5).unwrap(),
        user: Some("U".into()),
        application: Some("Atelier".into()),
        app_version: Some("1".into()),
        statistics: None,
    });
    let bytes = p.write().unwrap();
    assert_eq!(bytes, p.write().unwrap(), "deterministic");

    let ct = member(&bytes, "[Content_Types].xml");
    assert!(ct.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default "));
    assert!(ct.ends_with("</Types>"));
    assert!(ct.contains("<Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>"));
    assert_eq!(ct.matches("xmlns").count(), 1, "{ct}");

    let rels = member(&bytes, "_rels/.rels");
    assert!(rels.contains("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship "));
    assert_eq!(rels.matches("xmlns").count(), 1, "{rels}");
    assert!(rels.contains("Target=\"https://example.org/a&amp;b?c=d\" TargetMode=\"External\"/>"));
    assert!(
        rels.contains("Target=\"docProps/core.xml\"")
            && rels.contains("Target=\"docProps/app.xml\"")
    );

    let core = member(&bytes, "docProps/core.xml");
    assert!(
        core.contains(&format!("<cp:coreProperties {CORE_NS_DECLS}>")),
        "{core}"
    );
    assert!(core.contains("<dc:title>Titre</dc:title>"));
    assert!(core.contains(
        "<dcterms:modified xsi:type=\"dcterms:W3CDTF\">2026-01-02T03:04:05Z</dcterms:modified>"
    ));
    let app = member(&bytes, "docProps/app.xml");
    assert!(
        app.contains(&format!("<Properties xmlns=\"{APP_NS}\" ")),
        "{app}"
    );
    assert!(app.contains("<Company>ACME</Company>"));
    assert_eq!(app.matches("xmlns=").count(), 1);

    for (name, text) in [("ct", &ct), ("rels", &rels), ("core", &core), ("app", &app)] {
        assert!(!has_generated_prefix(text), "{name}: {text}");
    }
    assert!(!member(&bytes, "word/document.xml").is_empty());
    let back = Package::read(&bytes).unwrap();
    assert_eq!(back.properties_snapshot().editable.title, "Titre");
}

#[test]
fn the_reader_still_accepts_prefixed_opc_parts() {
    let p = with_parts(Some(&core_xml("<dc:title>T</dc:title>")), None);
    let bytes = p.write().unwrap();
    // The form the project used to write: prefixed, declaration repeated.
    let old = repack(&bytes, |name, data| {
        let text = String::from_utf8_lossy(&data).into_owned();
        if name == "[Content_Types].xml" {
            let ns = "http://schemas.openxmlformats.org/package/2006/content-types";
            text.replace("<Types xmlns=", "<ns0:Types xmlns:ns0=")
                .replace("</Types>", "</ns0:Types>")
                .replace("<Default ", &format!("<ns0:Default xmlns:ns0=\"{ns}\" "))
                .replace("<Override ", &format!("<ns0:Override xmlns:ns0=\"{ns}\" "))
                .into_bytes()
        } else if name.ends_with(".rels") {
            let ns = "http://schemas.openxmlformats.org/package/2006/relationships";
            text.replace("<Relationships xmlns=", "<ns0:Relationships xmlns:ns0=")
                .replace("</Relationships>", "</ns0:Relationships>")
                .replace(
                    "<Relationship ",
                    &format!("<ns0:Relationship xmlns:ns0=\"{ns}\" "),
                )
                .into_bytes()
        } else {
            data
        }
    });
    assert!(member(&old, "[Content_Types].xml").contains("ns0:Types"));
    let q = Package::read(&old).unwrap();
    assert_eq!(q.properties_snapshot().editable.title, "T");
    // Rewriting normalises it.
    let out = q.write().unwrap();
    assert!(!has_generated_prefix(&member(&out, "[Content_Types].xml")));
    assert!(!has_generated_prefix(&member(&out, "_rels/.rels")));
}

#[test]
fn an_unscannable_part_is_reported_and_regenerated_without_generated_prefixes() {
    // UTF-16 core.xml: the XML reader accepts it, the in-place editor does not.
    let text = core_xml("<dc:title>T&amp;\u{e9}</dc:title><dc:creator>A</dc:creator>")
        .replace("UTF-8", "UTF-16");
    let mut bytes = vec![0xFF, 0xFE];
    for u in text.encode_utf16() {
        bytes.extend(u.to_le_bytes());
    }
    let mut p = Package::read(&{
        let mut q = base();
        q.insert_part(PartUri::parse("docProps/core.xml").unwrap(), CORE_CT, bytes)
            .unwrap();
        q.add_relationship(
            None,
            "rId2",
            "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties",
            "docProps/core.xml",
            TargetMode::Internal,
        )
        .unwrap();
        q.write().unwrap()
    })
    .unwrap();
    let s = p.properties_snapshot();
    assert_eq!(s.editable.title, "T&\u{e9}");
    assert!(s.notices.contains(&PropertyNotice::PartRegenerated {
        part: "docProps/core.xml"
    }));
    p.apply_property_changes(&PropertyChanges {
        subject: Some("S".into()),
        ..Default::default()
    })
    .unwrap();
    let out = Package::read(&p.write().unwrap()).unwrap();
    let core = part(&out, "docProps/core.xml");
    assert!(!has_generated_prefix(&core), "{core}");
    let e = out.properties_snapshot().editable;
    assert_eq!(
        (e.title.as_str(), e.author.as_str(), e.subject.as_str()),
        ("T&\u{e9}", "A", "S")
    );
}

// ---- Point 2: operations that touch each other -------------------------------

#[test]
fn removal_and_insertion_that_touch_go_through_the_in_place_writer() {
    let c = core_xml("<dc:creator/><dc:title/>");
    let mut p = with_parts(Some(&c), None);
    p.apply_property_changes(&PropertyChanges {
        title: Some(String::new()),
        keywords: Some("k".into()),
        ..Default::default()
    })
    .unwrap();
    let out = Package::read(&p.write().unwrap()).unwrap();
    assert_eq!(
        part(&out, "docProps/core.xml"),
        core_xml("<dc:creator/><cp:keywords>k</cp:keywords>")
    );
    // Same with indentation, and a removal first in the file.
    let c = "<?xml version=\"1.0\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n  <dc:title>T</dc:title>\n  <dc:creator>A</dc:creator>\n</cp:coreProperties>";
    let mut p = with_parts(Some(c), None);
    p.apply_property_changes(&PropertyChanges {
        title: Some(String::new()),
        subject: Some("S".into()),
        ..Default::default()
    })
    .unwrap();
    let out = Package::read(&p.write().unwrap()).unwrap();
    assert_eq!(
        part(&out, "docProps/core.xml"),
        "<?xml version=\"1.0\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n  <dc:subject>S</dc:subject>\n  <dc:creator>A</dc:creator>\n</cp:coreProperties>"
    );
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const VALUES: [&str; 6] = [
    "alpha",
    "b & c",
    "\u{e9}t\u{e9}",
    "x<y>z",
    "a]]>b",
    "\"q\" 's'",
];

/// Field index -> (element name in the part, key of `EditableProperties` order).
const CORE_ELEMS: [&str; 8] = [
    "dc:title",
    "dc:subject",
    "dc:creator",
    "cp:keywords",
    "dc:description",
    "cp:category",
    "cp:contentStatus",
    "dc:language",
];
const APP_ELEMS: [&str; 3] = ["Manager", "Company", "HyperlinkBase"];

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn shuffle<T>(rng: &mut Rng, v: &mut [T]) {
    for i in (1..v.len()).rev() {
        v.swap(i, rng.below(i + 1));
    }
}

fn changes_for(vals: &[Option<Option<String>>; 11]) -> PropertyChanges {
    let g = |i: usize| vals[i].clone().map(|v| v.unwrap_or_default());
    PropertyChanges {
        title: g(0),
        subject: g(1),
        author: g(2),
        keywords: g(3),
        comments: g(4),
        category: g(5),
        content_status: g(6),
        language: g(7),
        manager: g(8),
        company: g(9),
        hyperlink_base: g(10),
    }
}

#[test]
fn random_orders_and_batches_always_give_clean_well_formed_parts() {
    let mut rng = Rng(0x5DEE_CE66_D1CE_4E5B);
    for round in 0..400 {
        let pretty = rng.below(2) == 0;
        let sep = if pretty { "\n  " } else { "" };
        // Initial content: a random subset in random order, plus unknown noise.
        let mut model: [String; 11] = Default::default();
        let mut core_items: Vec<String> = Vec::new();
        for (i, name) in CORE_ELEMS.iter().enumerate() {
            if rng.below(2) == 0 {
                let v = VALUES[rng.below(VALUES.len())];
                model[i] = v.to_owned();
                core_items.push(format!("<{name}>{}</{name}>", esc(v)));
            } else if rng.below(6) == 0 {
                core_items.push(format!("<{name}/>"));
            }
        }
        core_items.push("<cp:revision>7</cp:revision>".into());
        core_items.push("<x:unknown k=\"v\"><x:in>1</x:in></x:unknown>".into());
        shuffle(&mut rng, &mut core_items);
        let core = format!(
            "<?xml version=\"1.0\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:x=\"urn:x\">{}{}{}</cp:coreProperties>",
            if pretty { "\n  " } else { "" },
            core_items.join(sep),
            if pretty { "\n" } else { "" }
        );
        let mut app_items: Vec<String> = Vec::new();
        for (i, name) in APP_ELEMS.iter().enumerate() {
            if rng.below(2) == 0 {
                let v = VALUES[rng.below(VALUES.len())];
                model[8 + i] = v.to_owned();
                app_items.push(format!("<{name}>{}</{name}>", esc(v)));
            }
        }
        app_items.push("<Words>12</Words>".into());
        app_items.push("<TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>t</vt:lpstr></vt:vector></TitlesOfParts>".into());
        shuffle(&mut rng, &mut app_items);
        let app = format!(
            "<?xml version=\"1.0\"?>\n<Properties xmlns=\"{APP_NS}\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">{}{}{}</Properties>",
            if pretty { "\n  " } else { "" },
            app_items.join(sep),
            if pretty { "\n" } else { "" }
        );
        let mut p = with_parts(Some(&core), Some(&app));
        assert_eq!(
            p.properties_snapshot().editable.title,
            model[0],
            "round {round}"
        );

        // A random batch of changes: set, remove, or leave alone.
        let mut vals: [Option<Option<String>>; 11] = Default::default();
        for (i, slot) in vals.iter_mut().enumerate() {
            match rng.below(3) {
                0 => {}
                1 => {
                    *slot = Some(None);
                    model[i].clear();
                }
                _ => {
                    let v = VALUES[rng.below(VALUES.len())].to_owned();
                    model[i] = v.clone();
                    *slot = Some(Some(v));
                }
            }
        }
        // `Some(None)` means "empty string" in the batch.
        p.apply_property_changes(&changes_for(&vals)).unwrap();
        let bytes = p.write().unwrap();
        let out = Package::read(&bytes).unwrap_or_else(|e| panic!("round {round}: {e:?}"));
        let e = out.properties_snapshot().editable;
        let got = [
            &e.title,
            &e.subject,
            &e.author,
            &e.keywords,
            &e.comments,
            &e.category,
            &e.content_status,
            &e.language,
            &e.manager,
            &e.company,
            &e.hyperlink_base,
        ];
        for i in 0..11 {
            assert_eq!(got[i], &model[i], "round {round}, field {i}\n{core}\n{app}");
        }
        for path in ["docProps/core.xml", "docProps/app.xml"] {
            let text = part(&out, path);
            assert!(!has_generated_prefix(&text), "round {round}: {text}");
            assert!(text.starts_with("<?xml"));
        }
        let core_out = part(&out, "docProps/core.xml");
        assert!(
            core_out.contains("<cp:revision>7</cp:revision>"),
            "round {round}"
        );
        assert!(core_out.contains("<x:unknown k=\"v\"><x:in>1</x:in></x:unknown>"));
        let app_out = part(&out, "docProps/app.xml");
        assert!(app_out.contains("<Words>12</Words>"));
        assert!(app_out.contains("<TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>t</vt:lpstr></vt:vector></TitlesOfParts>"));
        // A second write of what was read is stable.
        assert_eq!(
            part(
                &Package::read(&out.write().unwrap()).unwrap(),
                "docProps/core.xml"
            ),
            core_out
        );
    }
}

// ---- Point 4: ]]> in a value -------------------------------------------------

#[test]
fn a_value_containing_the_cdata_terminator_survives_a_round_trip() {
    let mut p = with_parts(None, None);
    p.apply_property_changes(&PropertyChanges {
        title: Some("a]]>b".into()),
        comments: Some("x]]>y ]] > ]]]>".into()),
        company: Some("]]>".into()),
        ..Default::default()
    })
    .unwrap();
    let out = Package::read(&p.write().unwrap()).unwrap();
    let e = out.properties_snapshot().editable;
    assert_eq!(e.title, "a]]>b");
    assert_eq!(e.comments, "x]]>y ]] > ]]]>");
    assert_eq!(e.company, "]]>");
    let core = part(&out, "docProps/core.xml");
    assert!(core.contains("<dc:title>a]]&gt;b</dc:title>"), "{core}");
    assert!(!core.contains("]]>b"));
    // Editing another field leaves the value alone.
    let mut q = out;
    q.apply_property_changes(&PropertyChanges {
        subject: Some("S".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        Package::read(&q.write().unwrap())
            .unwrap()
            .properties_snapshot()
            .editable
            .title,
        "a]]>b"
    );
}
