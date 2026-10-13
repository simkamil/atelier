// SPDX-License-Identifier: AGPL-3.0-or-later
//! US-328: document properties. Expected values are computed by hand.
use atelier_fmt_ooxml::{
    DatePrecision, DocumentStatistics, EditableProperties, Package, PartUri, PdfDateFields,
    PropertyChanges, PropertyDate, PropertyError, PropertyNotice, SaveContext, TargetMode,
    W3cDateTime,
};

const MAIN: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const CORE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const APP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const CORE_CT: &str = "application/vnd.openxmlformats-package.core-properties+xml";
const APP_CT: &str = "application/vnd.openxmlformats-officedocument.extended-properties+xml";
const NS: &str = "xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:dcmitype=\"http://purl.org/dc/dcmitype/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"";
const APP_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/extended-properties";

fn core_xml(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<cp:coreProperties {NS}>{body}</cp:coreProperties>"
    )
}
fn app_xml(body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Properties xmlns=\"{APP_NS}\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\">{body}</Properties>"
    )
}

/// A package with a main part and optional property parts (linked or not).
fn package(core: Option<&str>, app: Option<&str>, linked: bool) -> Vec<u8> {
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
    if let Some(c) = core {
        p.insert_part(
            PartUri::parse("docProps/core.xml").unwrap(),
            CORE_CT,
            c.as_bytes().to_vec(),
        )
        .unwrap();
        if linked {
            p.add_relationship(
                None,
                "rId2",
                CORE,
                "docProps/core.xml",
                TargetMode::Internal,
            )
            .unwrap();
        }
    }
    if let Some(a) = app {
        p.insert_part(
            PartUri::parse("docProps/app.xml").unwrap(),
            APP_CT,
            a.as_bytes().to_vec(),
        )
        .unwrap();
        if linked {
            p.add_relationship(None, "rId3", APP, "docProps/app.xml", TargetMode::Internal)
                .unwrap();
        }
    }
    p.write().unwrap()
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
fn core_of(p: &Package) -> String {
    part(p, "docProps/core.xml")
}
fn app_of(p: &Package) -> String {
    part(p, "docProps/app.xml")
}
fn reread(p: &Package) -> Package {
    Package::read(&p.write().unwrap()).unwrap()
}

const WORD_CORE_BODY: &str = "<dc:title>Rapport</dc:title><dc:subject>Sujet</dc:subject><dc:creator>Alice</dc:creator><cp:keywords>a; b</cp:keywords><dc:description>Note</dc:description><cp:lastModifiedBy>Bob</cp:lastModifiedBy><cp:revision>3</cp:revision><cp:lastPrinted>2024-03-04T05:06:07Z</cp:lastPrinted><dcterms:created xsi:type=\"dcterms:W3CDTF\">2024-01-02T03:04:05Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2024-02-03T04:05:06Z</dcterms:modified><cp:category>Cat</cp:category><cp:contentStatus>Draft</cp:contentStatus><dc:identifier>ID-1</dc:identifier><dc:language>fr-FR</dc:language><cp:version>1.2</cp:version>";
const WORD_APP_BODY: &str = "<Template>Normal.dotm</Template><TotalTime>12</TotalTime><Pages>3</Pages><Words>100</Words><Characters>500</Characters><Application>Microsoft Office Word</Application><DocSecurity>0</DocSecurity><Lines>10</Lines><Paragraphs>4</Paragraphs><Company>ACME</Company><CharactersWithSpaces>600</CharactersWithSpaces><HeadingPairs><vt:vector size=\"2\" baseType=\"variant\"><vt:variant><vt:lpstr>Title</vt:lpstr></vt:variant><vt:variant><vt:i4>1</vt:i4></vt:variant></vt:vector></HeadingPairs><TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>Rapport</vt:lpstr></vt:vector></TitlesOfParts><Manager>Carol</Manager><HyperlinkBase>http://example.org/</HyperlinkBase><AppVersion>16.0000</AppVersion>";

fn word_package() -> Package {
    Package::read(&package(
        Some(&core_xml(WORD_CORE_BODY)),
        Some(&app_xml(WORD_APP_BODY)),
        true,
    ))
    .unwrap()
}

fn dt(s: &str) -> W3cDateTime {
    W3cDateTime::parse(s).unwrap()
}

// ---- Dates ----------------------------------------------------------------

#[test]
fn parses_and_writes_every_w3cdtf_form() {
    for (text, p) in [
        ("2024", DatePrecision::Year),
        ("2024-05", DatePrecision::Month),
        ("2024-05-17", DatePrecision::Day),
        ("2024-05-17T10:20Z", DatePrecision::Minute),
        ("2024-05-17T10:20:30Z", DatePrecision::Second),
        ("2024-05-17T10:20:30+02:00", DatePrecision::Second),
        ("2024-05-17T10:20:30-05:30", DatePrecision::Second),
        ("2024-05-17T10:20:30.5Z", DatePrecision::Fraction(1)),
        (
            "2024-05-17T10:20:30.123456+01:00",
            DatePrecision::Fraction(6),
        ),
        ("2024-05-17T10:20:30", DatePrecision::Second),
    ] {
        let d = dt(text);
        assert_eq!(d.precision, p, "{text}");
        assert_eq!(d.to_w3cdtf(), text, "{text}");
    }
    let d = dt("2024-05-17T10:20:30.5Z");
    assert_eq!((d.second, d.nanosecond), (30, 500_000_000));
    assert_eq!(dt("2024-05-17T10:20:30").offset_minutes, None);
    assert_eq!(dt("2024-05-17T10:20:30-05:30").offset_minutes, Some(-330));
    // Compact offset and surrounding spaces are tolerated; extra fraction digits are cut.
    assert_eq!(dt(" 2024-05-17T10:20:30+0200 ").offset_minutes, Some(120));
    assert_eq!(
        dt("2024-05-17T10:20:30.1234567891Z").nanosecond,
        123_456_789
    );
}

#[test]
fn rejects_invalid_dates() {
    for t in [
        "",
        "x",
        "24",
        "2024-13",
        "2024-02-30",
        "2023-02-29",
        "2024-05-17T24:00",
        "2024-05-17T10:60",
        "2024-05-17T10:20:60Z",
        "2024-05-17T10",
        "2024-05-17 10:20",
        "2024-05-17T10:20:30.Z",
        "2024-05-17T10:20:30+25:00",
        "2024-05-17T10:30:15+15:00",
        "2024-05-17T10:30:15-14:01",
        "2024-05-17T10:30:15+14:60",
        "2024-05-17T10:20:30+02:99",
        "2024-05-17T10:20:30Zjunk",
        "0000-01-01",
        "2024-5-17",
    ] {
        assert!(W3cDateTime::parse(t).is_err(), "{t:?}");
    }
    assert!(W3cDateTime::parse(&"1".repeat(10_000)).is_err());
    assert!(W3cDateTime::parse("2024-02-29").is_ok()); // leap year
    // The offset bound of XML Schema dateTime is inclusive.
    assert_eq!(dt("2024-05-17T10:30:15+14:00").offset_minutes, Some(840));
    assert_eq!(dt("2024-05-17T10:30:15-14:00").offset_minutes, Some(-840));
}

#[test]
fn unix_time_conversions_match_hand_values() {
    assert_eq!(dt("1970-01-01T00:00:00Z").unix_seconds(), 0);
    assert_eq!(dt("2000-01-01T00:00:00Z").unix_seconds(), 946_684_800);
    assert_eq!(dt("2000-01-01T01:00:00+01:00").unix_seconds(), 946_684_800);
    assert_eq!(
        W3cDateTime::from_unix_seconds(946_684_800)
            .unwrap()
            .to_w3cdtf(),
        "2000-01-01T00:00:00Z"
    );
    // 2024-02-29T23:30:00-02:00 is 2024-03-01T01:30:00Z.
    let u = dt("2024-02-29T23:30:00-02:00").to_utc().unwrap();
    assert_eq!(u.to_w3cdtf(), "2024-03-01T01:30:00Z");
    assert_eq!(
        W3cDateTime::from_unix_seconds(-86_400).unwrap().to_w3cdtf(),
        "1969-12-31T00:00:00Z"
    );
    assert!(W3cDateTime::utc(2024, 2, 30, 0, 0, 0).is_none());
}

// ---- Reading ----------------------------------------------------------------

#[test]
fn reads_every_property_of_a_word_style_file() {
    let s = word_package().properties_snapshot();
    let e = &s.editable;
    assert_eq!(e.title, "Rapport");
    assert_eq!(e.subject, "Sujet");
    assert_eq!(e.author, "Alice");
    assert_eq!(e.keywords, "a; b");
    assert_eq!(e.comments, "Note");
    assert_eq!(e.category, "Cat");
    assert_eq!(e.content_status, "Draft");
    assert_eq!(e.language, "fr-FR");
    assert_eq!(e.manager, "Carol");
    assert_eq!(e.company, "ACME");
    assert_eq!(e.hyperlink_base, "http://example.org/");
    let r = &s.read_only;
    assert_eq!(r.last_modified_by.as_deref(), Some("Bob"));
    assert_eq!(r.revision.as_deref(), Some("3"));
    assert_eq!(r.identifier.as_deref(), Some("ID-1"));
    assert_eq!(r.version.as_deref(), Some("1.2"));
    assert_eq!(
        r.created,
        Some(PropertyDate::Valid(dt("2024-01-02T03:04:05Z")))
    );
    assert_eq!(
        r.modified,
        Some(PropertyDate::Valid(dt("2024-02-03T04:05:06Z")))
    );
    assert_eq!(
        r.last_printed,
        Some(PropertyDate::Valid(dt("2024-03-04T05:06:07Z")))
    );
    assert_eq!(r.application.as_deref(), Some("Microsoft Office Word"));
    assert_eq!(r.app_version.as_deref(), Some("16.0000"));
    assert_eq!(r.template.as_deref(), Some("Normal.dotm"));
    assert_eq!(r.total_time, Some(12));
    assert_eq!(r.doc_security, Some(0));
    assert_eq!(
        r.statistics,
        DocumentStatistics {
            pages: Some(3),
            words: Some(100),
            characters: Some(500),
            characters_with_spaces: Some(600),
            lines: Some(10),
            paragraphs: Some(4),
        }
    );
    assert!(s.notices.is_empty(), "{:?}", s.notices);
}

#[test]
fn reads_prefixes_cdata_and_entities() {
    let xml = "<?xml version=\"1.0\"?><c:coreProperties xmlns:c=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:d=\"http://purl.org/dc/elements/1.1/\"><d:title><![CDATA[A <b> & c]]></d:title><d:description>x &amp; y &#233; &#x41;</d:description></c:coreProperties>";
    let p = Package::read(&package(Some(xml), None, true)).unwrap();
    let e = p.properties_snapshot().editable;
    assert_eq!(e.title, "A <b> & c");
    assert_eq!(e.comments, "x & y \u{e9} A");
}

#[test]
fn absent_parts_give_an_empty_snapshot() {
    let p = Package::read(&package(None, None, true)).unwrap();
    let s = p.properties_snapshot();
    assert_eq!(s.editable, EditableProperties::default());
    assert_eq!(s.read_only.created, None);
    assert!(s.notices.is_empty());
}

#[test]
fn invalid_date_is_kept_as_text_and_reported() {
    let xml = core_xml(
        "<dcterms:created xsi:type=\"dcterms:W3CDTF\">hier soir</dcterms:created><dc:title>T</dc:title>",
    );
    let mut p = Package::read(&package(Some(&xml), None, true)).unwrap();
    let s = p.properties_snapshot();
    assert_eq!(
        s.read_only.created,
        Some(PropertyDate::Invalid("hier soir".into()))
    );
    assert_eq!(
        s.notices,
        vec![PropertyNotice::InvalidDate {
            field: "created",
            text: "hier soir".into()
        }]
    );
    // Editing another field keeps the bad text byte for byte.
    p.apply_property_changes(&PropertyChanges {
        title: Some("U".into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    assert!(core_of(&out).contains(">hier soir</dcterms:created>"));
    // A save does not "repair" it either: created is present.
    let mut p = Package::read(&package(Some(&xml), None, true)).unwrap();
    let r = p.update_on_save(&ctx());
    assert!(!r.created_set);
    assert!(core_of(&reread(&p)).contains(">hier soir</dcterms:created>"));
}

#[test]
fn non_numeric_counters_are_reported() {
    let a = app_xml("<Words>beaucoup</Words><Pages>2</Pages>");
    let c = core_xml("<cp:revision>r2</cp:revision>");
    let p = Package::read(&package(Some(&c), Some(&a), true)).unwrap();
    let s = p.properties_snapshot();
    assert_eq!(s.read_only.statistics.words, None);
    assert_eq!(s.read_only.statistics.pages, Some(2));
    assert!(s.notices.contains(&PropertyNotice::NonNumeric {
        field: "Words",
        text: "beaucoup".into()
    }));
    assert!(s.notices.contains(&PropertyNotice::NonNumeric {
        field: "revision",
        text: "r2".into()
    }));
}

#[test]
fn duplicate_elements_last_one_wins_and_is_reported() {
    let c =
        core_xml("<dc:title>first</dc:title><dc:creator>A</dc:creator><dc:title>second</dc:title>");
    let mut p = Package::read(&package(Some(&c), None, true)).unwrap();
    let s = p.properties_snapshot();
    assert_eq!(s.editable.title, "second");
    assert!(s.notices.contains(&PropertyNotice::DuplicateElement {
        part: "docProps/core.xml",
        element: "title",
        count: 2
    }));
    // Editing changes the winning (last) element only.
    p.apply_property_changes(&PropertyChanges {
        title: Some("new".into()),
        ..Default::default()
    })
    .unwrap();
    let out = core_of(&reread(&p));
    assert!(
        out.contains(
            "<dc:title>first</dc:title><dc:creator>A</dc:creator><dc:title>new</dc:title>"
        )
    );
    // Clearing removes every copy.
    p.apply_property_changes(&PropertyChanges {
        title: Some(String::new()),
        ..Default::default()
    })
    .unwrap();
    let out = core_of(&reread(&p));
    assert!(!out.contains("title"));
    assert!(out.contains("<dc:creator>A</dc:creator>"));
}

// ---- Writing ----------------------------------------------------------------

#[test]
fn roundtrip_without_change_is_byte_identical() {
    let c = core_xml(WORD_CORE_BODY);
    let a = app_xml(WORD_APP_BODY);
    let mut p = Package::read(&package(Some(&c), Some(&a), true)).unwrap();
    // Merely asking for mutable access marks the parts, but nothing differs.
    let _ = p.core_properties_mut();
    let _ = p.app_properties_mut();
    let out = reread(&p);
    assert_eq!(core_of(&out), c);
    assert_eq!(app_of(&out), a);
    // Applying values equal to the stored ones changes nothing either.
    let before = p.properties_snapshot().editable;
    let r = p
        .apply_property_changes(&PropertyChanges::between(
            &EditableProperties::default(),
            &before,
        ))
        .unwrap();
    assert!(r.changed.is_empty());
    let out = reread(&p);
    assert_eq!(core_of(&out), c);
    assert_eq!(app_of(&out), a);
}

#[test]
fn changing_one_field_changes_only_that_element() {
    let c = core_xml(WORD_CORE_BODY);
    let a = app_xml(WORD_APP_BODY);
    let mut p = Package::read(&package(Some(&c), Some(&a), true)).unwrap();
    let r = p
        .apply_property_changes(&PropertyChanges {
            title: Some("Nouveau".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(r.changed, vec!["title"]);
    let out = reread(&p);
    assert_eq!(
        core_of(&out),
        c.replace(
            "<dc:title>Rapport</dc:title>",
            "<dc:title>Nouveau</dc:title>"
        )
    );
    assert_eq!(app_of(&out), a);
    // Same for an app.xml field, including the structured elements kept as is.
    let mut p = Package::read(&package(Some(&c), Some(&a), true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        company: Some("Initech".into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    assert_eq!(
        app_of(&out),
        a.replace("<Company>ACME</Company>", "<Company>Initech</Company>")
    );
    assert_eq!(core_of(&out), c);
}

#[test]
fn empty_string_removes_the_element_and_its_indentation() {
    let c = "<?xml version=\"1.0\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n  <dc:title>T</dc:title>\n  <dc:subject>S</dc:subject>\n  <cp:category>C</cp:category>\n</cp:coreProperties>";
    let mut p = Package::read(&package(Some(c), None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        subject: Some(String::new()),
        ..Default::default()
    })
    .unwrap();
    let out = core_of(&reread(&p));
    assert_eq!(out, c.replace("\n  <dc:subject>S</dc:subject>", ""));
    assert_eq!(p.properties_snapshot().editable.subject, "");
    // Setting a field that is only an empty element: `<dc:title/>` expands.
    let c2 = core_xml("<dc:title/><cp:category>C</cp:category>");
    let mut p = Package::read(&package(Some(&c2), None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        title: Some("X".into()),
        ..Default::default()
    })
    .unwrap();
    assert!(core_of(&reread(&p)).contains("<dc:title>X</dc:title><cp:category>C</cp:category>"));
}

#[test]
fn new_elements_go_in_canonical_order_and_keep_indentation() {
    let c = "<?xml version=\"1.0\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n  <dc:title>T</dc:title>\n  <cp:category>C</cp:category>\n</cp:coreProperties>";
    let mut p = Package::read(&package(Some(c), None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        subject: Some("S".into()),
        author: Some("A".into()),
        language: Some("fr".into()),
        ..Default::default()
    })
    .unwrap();
    let out = core_of(&reread(&p));
    assert_eq!(
        out,
        "<?xml version=\"1.0\"?>\n<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n  <dc:title>T</dc:title>\n  <dc:subject>S</dc:subject>\n  <dc:creator>A</dc:creator>\n  <cp:category>C</cp:category>\n  <dc:language>fr</dc:language>\n</cp:coreProperties>"
    );
}

#[test]
fn undeclared_namespaces_are_declared_on_the_new_element() {
    // The root declares only cp; dc and dcterms/xsi must be declared locally.
    let c = "<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"><cp:category>C</cp:category></cp:coreProperties>";
    let mut p = Package::read(&package(Some(c), None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        title: Some("T".into()),
        ..Default::default()
    })
    .unwrap();
    p.core_properties_mut().created = Some("2024-01-01T00:00:00Z".into());
    let out = reread(&p);
    let text = core_of(&out);
    assert!(
        text.contains("<dc:title xmlns:dc=\"http://purl.org/dc/elements/1.1/\">T</dc:title>"),
        "{text}"
    );
    assert!(text.contains("<dcterms:created xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"dcterms:W3CDTF\">2024-01-01T00:00:00Z</dcterms:created>"), "{text}");
    let s = out.properties_snapshot();
    assert_eq!(s.editable.title, "T");
    assert_eq!(s.editable.category, "C");
    assert!(s.read_only.created.is_some());
}

#[test]
fn default_namespace_and_self_closing_root_are_handled() {
    let c = "<coreProperties xmlns=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"/>";
    let mut p = Package::read(&package(Some(c), None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        category: Some("K".into()),
        ..Default::default()
    })
    .unwrap();
    let text = core_of(&reread(&p));
    assert_eq!(
        text,
        "<coreProperties xmlns=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\"><category>K</category></coreProperties>"
    );
}

#[test]
fn unknown_elements_attributes_and_structured_parts_are_preserved() {
    let c = "<?xml version=\"1.0\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:x=\"urn:vendor\" x:flag=\"root\"><x:custom k=\"v\"><x:in>1</x:in></x:custom><dc:title>T</dc:title><dc:rights>R</dc:rights><!-- note --></cp:coreProperties>";
    let a = app_xml(WORD_APP_BODY);
    let mut p = Package::read(&package(Some(c), Some(&a), true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        title: Some("T2".into()),
        company: Some("Z".into()),
        subject: Some("S".into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    let core = core_of(&out);
    assert!(core.contains("x:flag=\"root\""));
    assert!(core.contains("<x:custom k=\"v\"><x:in>1</x:in></x:custom>"));
    assert!(core.contains("<dc:rights>R</dc:rights><!-- note -->"));
    assert!(core.contains("<dc:title>T2</dc:title><dc:subject>S</dc:subject>"));
    let app = app_of(&out);
    for kept in [
        "<HeadingPairs><vt:vector size=\"2\" baseType=\"variant\"><vt:variant><vt:lpstr>Title</vt:lpstr></vt:variant><vt:variant><vt:i4>1</vt:i4></vt:variant></vt:vector></HeadingPairs>",
        "<TitlesOfParts><vt:vector size=\"1\" baseType=\"lpstr\"><vt:lpstr>Rapport</vt:lpstr></vt:vector></TitlesOfParts>",
        "<Company>Z</Company>",
    ] {
        assert!(app.contains(kept), "{kept}");
    }
}

#[test]
fn absent_parts_are_created_with_relationship_and_content_type() {
    let mut p = Package::read(&package(None, None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        title: Some("Titre".into()),
        company: Some("ACME".into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    let core_part = out
        .part(&PartUri::parse("docProps/core.xml").unwrap())
        .unwrap();
    assert_eq!(core_part.content_type, CORE_CT);
    let app_part = out
        .part(&PartUri::parse("docProps/app.xml").unwrap())
        .unwrap();
    assert_eq!(app_part.content_type, APP_CT);
    let rels = out.relationships(None);
    assert!(
        rels.iter()
            .any(|r| r.rel_type == CORE && r.target == "docProps/core.xml")
    );
    assert!(
        rels.iter()
            .any(|r| r.rel_type == APP && r.target == "docProps/app.xml")
    );
    let text = core_of(&out);
    assert!(text.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>"));
    assert!(text.contains("<dc:title>Titre</dc:title>"));
    assert_eq!(out.properties_snapshot().editable.title, "Titre");
    assert_eq!(out.properties_snapshot().editable.company, "ACME");
    // Only the part that was touched is created.
    let mut p = Package::read(&package(None, None, true)).unwrap();
    p.apply_property_changes(&PropertyChanges {
        title: Some("T".into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    assert!(
        out.part(&PartUri::parse("docProps/app.xml").unwrap())
            .is_none()
    );
}

#[test]
fn a_part_without_relationship_is_read_and_then_linked() {
    let c = core_xml("<dc:title>Orphelin</dc:title>");
    let mut p = Package::read(&package(Some(&c), None, false)).unwrap();
    let s = p.properties_snapshot();
    assert_eq!(s.editable.title, "Orphelin");
    assert_eq!(
        s.notices,
        vec![PropertyNotice::UnreferencedPart {
            part: "docProps/core.xml"
        }]
    );
    // Not touched: written back as it came, still without a relationship.
    assert!(
        reread(&p)
            .relationships(None)
            .iter()
            .all(|r| r.rel_type != CORE)
    );
    p.apply_property_changes(&PropertyChanges {
        subject: Some("S".into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    assert!(out.relationships(None).iter().any(|r| r.rel_type == CORE));
    assert_eq!(out.properties_snapshot().editable.title, "Orphelin");
    assert_eq!(out.properties_snapshot().editable.subject, "S");
    assert!(out.properties_snapshot().notices.is_empty());
}

#[test]
fn values_are_escaped_and_survive() {
    let mut p = Package::read(&package(None, None, true)).unwrap();
    let comments = "ligne 1\nligne 2 & <b>\"q\"</b> 'x' \u{e9}\u{e8}\u{4e2d}\u{1f600}";
    p.apply_property_changes(&PropertyChanges {
        title: Some("A & B < C > \"D\" 'E' \u{e9}t\u{e9}".into()),
        comments: Some(comments.into()),
        ..Default::default()
    })
    .unwrap();
    let out = reread(&p);
    let text = core_of(&out);
    assert!(
        text.contains("<dc:title>A &amp; B &lt; C &gt; \"D\" 'E' \u{e9}t\u{e9}</dc:title>"),
        "{text}"
    );
    assert!(text.contains("<dc:description>ligne 1\nligne 2 &amp; &lt;b&gt;\"q\"&lt;/b&gt; 'x' \u{e9}\u{e8}\u{4e2d}\u{1f600}</dc:description>"));
    let e = out.properties_snapshot().editable;
    assert_eq!(e.title, "A & B < C > \"D\" 'E' \u{e9}t\u{e9}");
    assert_eq!(e.comments, comments);
}

#[test]
fn text_rules_clean_control_characters_and_line_breaks() {
    let mut p = Package::read(&package(None, None, true)).unwrap();
    let r = p
        .apply_property_changes(&PropertyChanges {
            title: Some("a\u{0}b\u{1}c\td\r\ne\nf".into()),
            comments: Some("x\r\ny\rz\u{b}".into()),
            keywords: Some("un; deux".into()),
            ..Default::default()
        })
        .unwrap();
    let e = p.properties_snapshot().editable;
    assert_eq!(e.title, "abc d  e f"); // NUL and SOH dropped; tab, CR, LF -> space
    assert_eq!(e.comments, "x\ny\nz"); // CRLF and CR -> LF; VT dropped
    assert_eq!(e.keywords, "un; deux");
    assert_eq!(r.sanitized, vec!["title", "comments"]);
    // A value that becomes empty after cleaning removes the element.
    p.apply_property_changes(&PropertyChanges {
        subject: Some("\u{1}".into()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(p.properties_snapshot().editable.subject, "");
}

#[test]
fn lengths_are_bounded_and_nothing_is_applied_on_error() {
    let mut p = Package::read(&package(None, None, true)).unwrap();
    let ok = "x".repeat(255);
    let long = "\u{e9}".repeat(256); // characters, not bytes
    let err = p
        .apply_property_changes(&PropertyChanges {
            subject: Some("fine".into()),
            title: Some(long),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(
        err,
        PropertyError::TooLong {
            field: "title",
            max: 255,
            len: 256
        }
    );
    assert_eq!(p.properties_snapshot().editable.subject, "");
    p.apply_property_changes(&PropertyChanges {
        title: Some(ok.clone()),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(p.properties_snapshot().editable.title, ok);
    assert!(
        p.apply_property_changes(&PropertyChanges {
            keywords: Some("k".repeat(1025)),
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        p.apply_property_changes(&PropertyChanges {
            comments: Some("c".repeat(4097)),
            ..Default::default()
        })
        .is_err()
    );
    assert!(
        p.apply_property_changes(&PropertyChanges {
            comments: Some("c".repeat(4096)),
            ..Default::default()
        })
        .is_ok()
    );
}

#[test]
fn between_lists_only_the_fields_the_dialog_changed() {
    let old = EditableProperties {
        title: "a".into(),
        subject: "b".into(),
        ..Default::default()
    };
    let new = EditableProperties {
        title: "a".into(),
        subject: String::new(),
        author: "z".into(),
        ..Default::default()
    };
    let ch = PropertyChanges::between(&old, &new);
    assert_eq!(ch.title, None);
    assert_eq!(ch.subject, Some(String::new()));
    assert_eq!(ch.author, Some("z".into()));
}

// ---- Save-time update ----------------------------------------------------------

fn ctx() -> SaveContext {
    SaveContext {
        now: W3cDateTime::utc(2025, 6, 7, 8, 9, 10).unwrap(),
        user: Some("Dana".into()),
        application: Some("Atelier".into()),
        app_version: Some("1.0".into()),
        statistics: Some(DocumentStatistics {
            pages: Some(2),
            words: Some(30),
            characters: Some(150),
            characters_with_spaces: Some(180),
            lines: Some(9),
            paragraphs: Some(3),
        }),
    }
}

#[test]
fn opening_never_changes_anything_only_saving_does() {
    let c = core_xml(WORD_CORE_BODY);
    let a = app_xml(WORD_APP_BODY);
    let p = Package::read(&package(Some(&c), Some(&a), true)).unwrap();
    let out = reread(&p);
    assert_eq!(core_of(&out), c);
    assert_eq!(app_of(&out), a);
}

#[test]
fn save_update_on_an_existing_file() {
    let mut p = word_package();
    let r = p.update_on_save(&ctx());
    assert_eq!(r.revision, "4");
    assert!(!r.revision_restarted && !r.created_set && !r.creator_set);
    let out = reread(&p);
    let c = core_of(&out);
    // Only modified, lastModifiedBy and revision differ.
    let expected = core_xml(
        &WORD_CORE_BODY
            .replace("<cp:lastModifiedBy>Bob<", "<cp:lastModifiedBy>Dana<")
            .replace("<cp:revision>3<", "<cp:revision>4<")
            .replace("2024-02-03T04:05:06Z", "2025-06-07T08:09:10Z"),
    );
    assert_eq!(c, expected);
    let a = app_of(&out);
    let expected_app = app_xml(
        &WORD_APP_BODY
            .replace("<Pages>3<", "<Pages>2<")
            .replace("<Words>100<", "<Words>30<")
            .replace("<Characters>500<", "<Characters>150<")
            .replace("<CharactersWithSpaces>600<", "<CharactersWithSpaces>180<")
            .replace("<Lines>10<", "<Lines>9<")
            .replace("<Paragraphs>4<", "<Paragraphs>3<")
            .replace("Microsoft Office Word", "Atelier")
            .replace("16.0000", "1.0"),
    );
    assert_eq!(a, expected_app);
}

#[test]
fn save_update_on_a_new_document_creates_everything() {
    let mut p = Package::read(&package(None, None, true)).unwrap();
    let r = p.update_on_save(&ctx());
    assert!(r.created_set && r.creator_set && r.revision_restarted);
    assert_eq!(r.revision, "1");
    let out = reread(&p);
    let s = out.properties_snapshot();
    assert_eq!(s.editable.author, "Dana");
    assert_eq!(s.read_only.last_modified_by.as_deref(), Some("Dana"));
    assert_eq!(s.read_only.revision.as_deref(), Some("1"));
    assert_eq!(
        s.read_only.created,
        Some(PropertyDate::Valid(dt("2025-06-07T08:09:10Z")))
    );
    assert_eq!(s.read_only.modified, s.read_only.created);
    assert_eq!(s.read_only.application.as_deref(), Some("Atelier"));
    assert_eq!(s.read_only.app_version.as_deref(), Some("1.0"));
    assert_eq!(s.read_only.statistics.words, Some(30));
    assert_eq!(s.read_only.statistics.characters_with_spaces, Some(180));
    let c = core_of(&out);
    assert!(c.contains(
        "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2025-06-07T08:09:10Z</dcterms:created>"
    ));
}

#[test]
fn save_update_keeps_existing_creator_and_created_and_normalises_now_to_utc() {
    let mut p = word_package();
    let mut c = ctx();
    c.now = dt("2025-06-07T23:30:00.75-02:00");
    c.user = None;
    c.statistics = None;
    p.update_on_save(&c);
    let s = reread(&p).properties_snapshot();
    assert_eq!(s.editable.author, "Alice");
    assert_eq!(s.read_only.last_modified_by.as_deref(), Some("Bob")); // no user: unchanged
    assert_eq!(
        s.read_only.created,
        Some(PropertyDate::Valid(dt("2024-01-02T03:04:05Z")))
    );
    assert_eq!(
        s.read_only.modified,
        Some(PropertyDate::Valid(dt("2025-06-08T01:30:00Z")))
    );
    assert_eq!(s.read_only.statistics.words, Some(100)); // None leaves the old value
}

#[test]
fn revision_rules() {
    for (before, after, restarted) in [
        (Some("0"), "1", false),
        (Some(" 9 "), "10", false),
        (Some("18446744073709551615"), "18446744073709551615", false),
        (Some("abc"), "1", true),
        (Some(""), "1", true),
        (Some("-3"), "1", true),
        (Some("2.5"), "1", true),
        (None, "1", true),
    ] {
        let body = before.map_or(String::new(), |b| format!("<cp:revision>{b}</cp:revision>"));
        let mut p = Package::read(&package(Some(&core_xml(&body)), None, true)).unwrap();
        let r = p.update_on_save(&ctx());
        assert_eq!(
            (r.revision.as_str(), r.revision_restarted),
            (after, restarted),
            "{before:?}"
        );
        assert_eq!(
            reread(&p)
                .properties_snapshot()
                .read_only
                .revision
                .as_deref(),
            Some(after)
        );
    }
}

#[test]
fn save_context_text_is_cleaned_and_truncated() {
    let mut p = Package::read(&package(None, None, true)).unwrap();
    let mut c = ctx();
    c.user = Some(format!("{}\u{1}\n", "u".repeat(300)));
    p.update_on_save(&c);
    let s = p.properties_snapshot();
    assert_eq!(s.editable.author.chars().count(), 255);
    assert!(!s.editable.author.contains('\u{1}'));
}

// ---- PDF -------------------------------------------------------------------------

#[test]
fn pdf_metadata_maps_field_by_field() {
    let m = word_package().pdf_metadata();
    assert_eq!(m.title.as_deref(), Some("Rapport"));
    assert_eq!(m.author.as_deref(), Some("Alice"));
    assert_eq!(m.subject.as_deref(), Some("Sujet"));
    assert_eq!(m.keywords.as_deref(), Some("a; b"));
    assert_eq!(
        m.creation_date,
        Some(PdfDateFields {
            year: 2024,
            month: 1,
            day: 2,
            hour: 3,
            minute: 4,
            second: 5,
            utc_offset_minutes: 0
        })
    );
    assert_eq!(
        m.modification_date,
        Some(PdfDateFields {
            year: 2024,
            month: 2,
            day: 3,
            hour: 4,
            minute: 5,
            second: 6,
            utc_offset_minutes: 0
        })
    );
    // Offsets, date-only values, huge offsets and invalid dates.
    let c = core_xml(
        "<dcterms:created xsi:type=\"dcterms:W3CDTF\">2024-05-17T10:20:30+05:30</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2024-05-17</dcterms:modified>",
    );
    let m = Package::read(&package(Some(&c), None, true))
        .unwrap()
        .pdf_metadata();
    assert_eq!(m.creation_date.unwrap().utc_offset_minutes, 330);
    assert_eq!(
        m.modification_date,
        Some(PdfDateFields {
            year: 2024,
            month: 5,
            day: 17,
            hour: 0,
            minute: 0,
            second: 0,
            utc_offset_minutes: 0
        })
    );
    // An offset beyond +-14:00 is not a valid dateTime: no PDF date, text kept.
    let c = core_xml(
        "<dcterms:created>2024-05-17T10:20:30+14:01</dcterms:created><dcterms:modified>nope</dcterms:modified>",
    );
    let p = Package::read(&package(Some(&c), None, true)).unwrap();
    assert_eq!(
        p.properties_snapshot().read_only.created,
        Some(PropertyDate::Invalid("2024-05-17T10:20:30+14:01".into()))
    );
    let m = p.pdf_metadata();
    assert_eq!(m.creation_date, None);
    assert_eq!(m.modification_date, None);
    assert_eq!(
        Package::read(&package(None, None, true))
            .unwrap()
            .pdf_metadata(),
        Default::default()
    );
}

// ---- Robustness ------------------------------------------------------------------

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

fn exercise(core: &[u8], app: &[u8]) {
    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/xml",
        b"<d/>".to_vec(),
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
    p.insert_part(
        PartUri::parse("docProps/core.xml").unwrap(),
        CORE_CT,
        core.to_vec(),
    )
    .unwrap();
    p.insert_part(
        PartUri::parse("docProps/app.xml").unwrap(),
        APP_CT,
        app.to_vec(),
    )
    .unwrap();
    p.add_relationship(
        None,
        "rId2",
        CORE,
        "docProps/core.xml",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(None, "rId3", APP, "docProps/app.xml", TargetMode::Internal)
        .unwrap();
    let Ok(bytes) = p.write() else { return };
    let Ok(mut q) = Package::read(&bytes) else {
        return;
    };
    let _ = q.properties_snapshot();
    let _ = q.pdf_metadata();
    let _ = q.apply_property_changes(&PropertyChanges {
        title: Some("t".into()),
        subject: Some(String::new()),
        company: Some("c".into()),
        hyperlink_base: Some("h".into()),
        ..Default::default()
    });
    let _ = q.update_on_save(&ctx());
    if let Ok(out) = q.write() {
        let _ = Package::read(&out).map(|r| r.properties_snapshot());
    }
}

#[test]
fn mutated_and_truncated_parts_never_panic() {
    let core = core_xml(WORD_CORE_BODY).into_bytes();
    let app = app_xml(WORD_APP_BODY).into_bytes();
    let mut rng = Rng(0x2D35_8DCC_AA6C_78A5);
    let alphabet: &[u8] = b"<>/&;\"'=:! -\xff\x00a\xc3";
    exercise(&core, &app);
    for round in 0..600 {
        let (mut c, mut a) = (core.clone(), app.clone());
        for part in [&mut c, &mut a] {
            match rng.below(4) {
                0 => {
                    let n = rng.below(part.len());
                    part.truncate(n);
                }
                1 => {
                    for _ in 0..1 + rng.below(4) {
                        let i = rng.below(part.len());
                        part[i] = alphabet[rng.below(alphabet.len())];
                    }
                }
                2 => {
                    let i = rng.below(part.len());
                    let j = (i + rng.below(40)).min(part.len());
                    part.drain(i..j);
                }
                _ => {
                    let i = rng.below(part.len());
                    let ins = part[rng.below(part.len())..]
                        .iter()
                        .take(30)
                        .copied()
                        .collect::<Vec<_>>();
                    part.splice(i..i, ins);
                }
            }
        }
        exercise(&c, &a);
        let _ = round;
    }
}

#[test]
fn huge_values_are_handled_without_panic() {
    // Pseudo-random letters keep the compression ratio below the ZIP bomb limit.
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let big: String = (0..400_000)
        .map(|_| (b'a' + rng.below(26) as u8) as char)
        .collect();
    let c = core_xml(&format!(
        "<dc:title>{big}</dc:title><dcterms:created>{big}</dcterms:created>"
    ));
    let p = Package::read(&package(Some(&c), None, true)).unwrap();
    let s = p.properties_snapshot();
    assert_eq!(s.editable.title.len(), big.len());
    assert!(matches!(
        s.read_only.created,
        Some(PropertyDate::Invalid(_))
    ));
    let deep = format!("{}{}", "<x>".repeat(5000), "</x>".repeat(5000));
    let c = core_xml(&format!("<dc:title>T</dc:title>{deep}"));
    if let Ok(p) = Package::read(&package(Some(&c), None, true)) {
        let _ = p.properties_snapshot();
    }
}
