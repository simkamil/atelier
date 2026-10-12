// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_fmt_ooxml::{
    AppProperties, CoreProperties, Limits, OpcError, Package, PartUri, TargetMode,
};
use atelier_zip::{Archive, Limits as ZipLimits, Method, WriteOptions, Writer};

const MAIN: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const CORE: &str =
    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties";
const APP: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties";
const IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const CONTENT_TYPES_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL_CONTENT_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";

fn sample() -> Package {
    let mut p = Package::new();
    p.insert_part(
        PartUri::parse("document.xml").unwrap(),
        "application/xml",
        b"<root/>".to_vec(),
    )
    .unwrap();
    p.insert_part(
        PartUri::parse("word/document.xml").unwrap(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
        b"<w:document xmlns:w=\"urn:word\"><w:body/></w:document>".to_vec(),
    )
    .unwrap();
    p.insert_part(PartUri::parse("custom/opaque.xml").unwrap(), "application/x-atlas-unknown+xml", b"<vendor:data xmlns:vendor=\"urn:vendor\"><vendor:item keep=\"yes\">opaque</vendor:item></vendor:data>".to_vec()).unwrap();
    p.insert_part(
        PartUri::parse("custom/blob.bin").unwrap(),
        "application/x-atlas-blob",
        vec![0, 1, 2, 0xff, 0x80],
    )
    .unwrap();
    p.insert_part(PartUri::parse("docProps/core.xml").unwrap(), "application/vnd.openxmlformats-package.core-properties+xml", b"<?xml version=\"1.0\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:x=\"urn:vendor\"><dc:title>First title</dc:title><x:custom flag=\"kept\"><x:v>42</x:v></x:custom></cp:coreProperties>".to_vec()).unwrap();
    p.insert_part(PartUri::parse("docProps/app.xml").unwrap(), "application/vnd.openxmlformats-officedocument.extended-properties+xml", b"<?xml version=\"1.0\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\"><Application>Atelier</Application><VendorField>value</VendorField></Properties>".to_vec()).unwrap();
    p.add_relationship(
        None,
        "rId1",
        MAIN,
        "/word/document.xml",
        TargetMode::Internal,
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
    p.add_relationship(
        None,
        "rId4",
        "urn:external",
        "https://example.invalid/resource",
        TargetMode::External,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("document.xml").unwrap()),
        "rIdRoot",
        IMAGE,
        "custom/blob.bin",
        TargetMode::Internal,
    )
    .unwrap();
    p.add_relationship(
        Some(PartUri::parse("word/document.xml").unwrap()),
        "rId9",
        IMAGE,
        "../custom/blob.bin",
        TargetMode::Internal,
    )
    .unwrap();
    p
}

fn relationship_tuples(
    package: &Package,
) -> Vec<(Option<String>, String, String, String, TargetMode)> {
    let sources = [None, Some("document.xml"), Some("word/document.xml")];
    let mut out = Vec::new();
    for source in sources {
        let uri = source.map(|s| PartUri::parse(s).unwrap());
        for rel in package.relationships(uri.as_ref()) {
            out.push((
                source.map(str::to_owned),
                rel.id.clone(),
                rel.rel_type.clone(),
                rel.target.clone(),
                rel.mode,
            ));
        }
    }
    out.sort_by(|a, b| (&a.0, &a.1).cmp(&(&b.0, &b.1)));
    out
}

fn replace_zip_member(bytes: &[u8], name: &str, replacement: &[u8]) -> Vec<u8> {
    let archive = Archive::open(bytes, ZipLimits::default()).unwrap();
    let mut writer = Writer::new(WriteOptions::default());
    for entry in archive.entries() {
        let content = if entry.name() == name {
            replacement.to_vec()
        } else {
            entry.read_all().unwrap()
        };
        writer.add(entry.name(), &content, Method::Store).unwrap();
    }
    writer.finish().unwrap()
}

fn rename_zip_member(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let archive = Archive::open(bytes, ZipLimits::default()).unwrap();
    let mut writer = Writer::new(WriteOptions::default());
    for entry in archive.entries() {
        let name = if entry.name() == from {
            to
        } else {
            entry.name()
        };
        writer
            .add(name, &entry.read_all().unwrap(), Method::Store)
            .unwrap();
    }
    writer.finish().unwrap()
}

#[test]
fn preserves_unknown_parts_relations_and_properties_through_deterministic_round_trip() {
    let expected = sample();
    let first = expected.write().unwrap();
    let read = Package::read(&first).unwrap();
    assert_eq!(read.parts(), expected.parts());
    assert_eq!(relationship_tuples(&read), relationship_tuples(&expected));
    let opaque = read
        .part(&PartUri::parse("custom/opaque.xml").unwrap())
        .unwrap();
    assert_eq!(opaque.content_type, "application/x-atlas-unknown+xml");
    assert!(opaque.bytes.ends_with(b"</vendor:data>"));
    assert_eq!(
        read.part(&PartUri::parse("custom/blob.bin").unwrap())
            .unwrap()
            .bytes,
        [0, 1, 2, 0xff, 0x80]
    );
    assert_eq!(read.core_properties().title.as_deref(), Some("First title"));
    assert_eq!(
        read.app_properties().application.as_deref(),
        Some("Atelier")
    );
    assert_eq!(
        read.relationships(Some(&PartUri::parse("word/document.xml").unwrap()))[0]
            .resolved_target
            .as_ref()
            .unwrap()
            .as_str(),
        "custom/blob.bin"
    );
    let root_rels = read.relationships(None);
    assert_eq!(
        root_rels.iter().find(|r| r.id == "rId4").unwrap().mode,
        TargetMode::External
    );
    assert!(
        root_rels
            .iter()
            .find(|r| r.id == "rId4")
            .unwrap()
            .resolved_target
            .is_none()
    );
    let second = read.write().unwrap();
    let third = Package::read(&second).unwrap().write().unwrap();
    assert_eq!(second, third);
    let mut edited = Package::read(&second).unwrap();
    edited.core_properties_mut().title = Some("Updated title".into());
    edited.app_properties_mut().company = Some("Atelier team".into());
    let edited = Package::read(&edited.write().unwrap()).unwrap();
    assert_eq!(
        edited.core_properties().title.as_deref(),
        Some("Updated title")
    );
    assert_eq!(
        edited.app_properties().company.as_deref(),
        Some("Atelier team")
    );
    let core = &edited
        .part(&PartUri::parse("docProps/core.xml").unwrap())
        .unwrap()
        .bytes;
    assert!(
        core.windows(b"urn:vendor".len())
            .any(|w| w == b"urn:vendor")
    );
    let app = &edited
        .part(&PartUri::parse("docProps/app.xml").unwrap())
        .unwrap()
        .bytes;
    assert!(
        app.windows(b"VendorField".len())
            .any(|w| w == b"VendorField")
    );
}

#[test]
fn rejects_invalid_part_uris_and_duplicate_relationship_ids() {
    assert!(matches!(
        PartUri::parse("word/../secret.xml"),
        Err(OpcError::InvalidUri(_))
    ));
    assert!(matches!(
        PartUri::parse("word/%2fsecret.xml"),
        Err(OpcError::InvalidUri(_))
    ));
    assert!(PartUri::parse("word/%62.xml").is_err());
    assert!(PartUri::parse("word/name.").is_err());
    assert!(PartUri::parse("word/%26.xml").is_ok());
    assert!(PartUri::parse("文書/概要.xml").is_ok());
    assert!(PartUri::parse("é/概要.xml").is_ok());
    assert!(PartUri::parse("word/%61%ff.xml").is_err());
    for invalid in ["a<b", "a>b", "a\"b", "a[b", "a]b"] {
        assert!(
            PartUri::parse(invalid).is_err(),
            "accepted segment: {invalid}"
        );
    }
    assert_eq!(
        PartUri::parse("A.xml").unwrap(),
        PartUri::parse("a.XML").unwrap()
    );
    let source = Some(PartUri::parse("word/document.xml").unwrap());
    for target in [
        "word//document.xml",
        "//word/document.xml",
        "../custom/blob.bin/",
    ] {
        assert!(matches!(
            Package::new().add_relationship(
                source.clone(),
                "bad",
                IMAGE,
                target,
                TargetMode::Internal
            ),
            Err(OpcError::InvalidUri(_))
        ));
    }
    assert!(matches!(
        Package::new().add_relationship(
            Some(PartUri::parse("document.xml").unwrap()),
            "bad",
            IMAGE,
            "../../escape.xml",
            TargetMode::Internal
        ),
        Err(OpcError::InvalidUri(_))
    ));
    let mut p = sample();
    assert!(matches!(
        p.add_relationship(
            None,
            "rId1",
            MAIN,
            "word/document.xml",
            TargetMode::Internal
        ),
        Err(OpcError::Duplicate(_))
    ));
}

#[test]
fn supports_relationship_metadata_overrides_and_case_insensitive_paths() {
    let bytes = sample().write().unwrap();
    let archive = Archive::open(&bytes, ZipLimits::default()).unwrap();
    let content_types = archive
        .entries()
        .iter()
        .find(|entry| entry.name() == "[Content_Types].xml")
        .unwrap()
        .read_all()
        .unwrap();
    let content_types = String::from_utf8(content_types).unwrap().replace(
        "</ns0:Types>",
        &format!(
            "<ns0:Override PartName=\"/_rels/.rels\" ContentType=\"{REL_CONTENT_TYPE}\"/></ns0:Types>"
        ),
    );
    let bytes = replace_zip_member(&bytes, "[Content_Types].xml", content_types.as_bytes());
    assert!(Package::read(&bytes).is_ok());

    let bytes = sample().write().unwrap();
    let bytes = rename_zip_member(
        &bytes,
        "word/_rels/document.xml.rels",
        "WORD/_RELS/DOCUMENT.XML.RELS",
    );
    let mut package = Package::read(&bytes).unwrap();
    let source = PartUri::parse("word/document.xml").unwrap();
    assert_eq!(package.relationships(Some(&source)).len(), 1);

    package
        .insert_part(
            PartUri::parse("_rels/foo.bin").unwrap(),
            "application/octet-stream",
            vec![1, 2, 3],
        )
        .unwrap();
    let round_trip = Package::read(&package.write().unwrap()).unwrap();
    assert!(
        round_trip
            .part(&PartUri::parse("_rels/foo.bin").unwrap())
            .is_some()
    );
}

#[test]
fn accepts_case_insensitive_relationships_media_type() {
    let bytes = sample().write().unwrap();
    let archive = Archive::open(&bytes, ZipLimits::default()).unwrap();
    let content_types = archive
        .entries()
        .iter()
        .find(|entry| entry.name() == "[Content_Types].xml")
        .unwrap()
        .read_all()
        .unwrap();
    let content_types = String::from_utf8(content_types)
        .unwrap()
        .replace(REL_CONTENT_TYPE, &REL_CONTENT_TYPE.to_ascii_uppercase());
    let bytes = replace_zip_member(&bytes, "[Content_Types].xml", content_types.as_bytes());
    assert!(Package::read(&bytes).is_ok());
}

#[test]
fn rejects_missing_internal_relationship_targets_on_read() {
    let bytes = sample().write().unwrap();
    let invalid_rels = format!(
        "<Relationships xmlns=\"{REL_NS}\"><Relationship Id=\"rIdMissing\" Type=\"urn:test:relation\" Target=\"missing.xml\"/></Relationships>"
    );
    let bytes = replace_zip_member(&bytes, "_rels/.rels", invalid_rels.as_bytes());
    assert!(matches!(Package::read(&bytes), Err(OpcError::Missing(_))));
}

#[test]
fn rejects_malformed_and_oversized_xml_parts() {
    let mut p = sample();
    p.insert_part(
        PartUri::parse("custom/bad.xml").unwrap(),
        "application/x-atlas+xml",
        b"<broken>".to_vec(),
    )
    .unwrap();
    let bytes = p.write().unwrap();
    assert!(matches!(Package::read(&bytes), Err(OpcError::Xml(_))));

    let bytes = sample().write().unwrap();
    let limits = Limits {
        max_xml_part_bytes: 8,
        ..Limits::default()
    };
    assert!(matches!(
        Package::read_with_limits(&bytes, limits),
        Err(OpcError::Limit("XML part bytes"))
    ));
}

#[test]
fn applies_public_part_byte_and_relationship_limits_and_rejects_truncated_zip() {
    let bytes = sample().write().unwrap();
    let limits = Limits {
        max_parts: 4,
        ..Limits::default()
    };
    assert!(matches!(
        Package::read_with_limits(&bytes, limits),
        Err(OpcError::Limit("part count"))
    ));
    let limits = Limits {
        max_total_part_bytes: 1,
        ..Limits::default()
    };
    assert!(matches!(
        Package::read_with_limits(&bytes, limits),
        Err(OpcError::Limit("total part bytes"))
    ));
    let limits = Limits {
        max_relationships: 1,
        ..Limits::default()
    };
    assert!(matches!(
        Package::read_with_limits(&bytes, limits),
        Err(OpcError::Limit("relationship count"))
    ));
    assert!(Package::read(&bytes[..bytes.len() - 8]).is_err());
}

#[test]
fn rejects_relationships_at_package_root_if_nested_or_id_has_whitespace() {
    let bytes = sample().write().unwrap();
    let nested = format!(
        "<Relationships xmlns=\"{REL_NS}\"><Group><Relationship Id=\"rIdX\" Type=\"urn:test\" Target=\"https://example.invalid/x\" TargetMode=\"External\"/></Group></Relationships>"
    );
    let bytes = replace_zip_member(&bytes, "_rels/document.xml.rels", nested.as_bytes());
    assert!(matches!(
        Package::read(&bytes),
        Err(OpcError::InvalidRelationship(_))
    ));

    let bytes = sample().write().unwrap();
    let bad_id = format!(
        "<Relationships xmlns=\"{REL_NS}\"><Relationship Id=\"bad id\" Type=\"urn:test\" Target=\"https://example.invalid/x\" TargetMode=\"External\"/></Relationships>"
    );
    let bytes = replace_zip_member(&bytes, "_rels/.rels", bad_id.as_bytes());
    assert!(matches!(
        Package::read(&bytes),
        Err(OpcError::InvalidRelationship(_))
    ));
}

#[test]
fn rejects_nested_content_type_entries() {
    let bytes = sample().write().unwrap();
    let nested = format!(
        "<Types xmlns=\"{CONTENT_TYPES_NS}\"><Group><Default Extension=\"xml\" ContentType=\"application/xml\"/></Group></Types>"
    );
    let bytes = replace_zip_member(&bytes, "[Content_Types].xml", nested.as_bytes());
    assert!(matches!(
        Package::read(&bytes),
        Err(OpcError::InvalidPackage(_))
    ));
}

#[test]
fn writes_core_and_app_properties_from_a_new_package() {
    let mut package = Package::new();
    let expected_core = CoreProperties {
        title: Some("title".into()),
        subject: Some("subject".into()),
        creator: Some("creator".into()),
        keywords: Some("keywords".into()),
        description: Some("description".into()),
        last_modified_by: Some("last".into()),
        revision: Some("7".into()),
        created: Some("created".into()),
        modified: Some("modified".into()),
        category: Some("category".into()),
        content_status: Some("status".into()),
        identifier: Some("identifier".into()),
        language: Some("fr-FR".into()),
        version: Some("version".into()),
    };
    let expected_app = AppProperties {
        application: Some("application".into()),
        app_version: Some("8.1".into()),
        company: Some("company".into()),
        manager: Some("manager".into()),
        pages: Some("11".into()),
        words: Some("22".into()),
        characters: Some("33".into()),
        paragraphs: Some("44".into()),
        lines: Some("55".into()),
        template: Some("template".into()),
    };
    *package.core_properties_mut() = expected_core.clone();
    *package.app_properties_mut() = expected_app.clone();

    let bytes = package.write().unwrap();
    let read = Package::read(&bytes).unwrap();
    assert_eq!(read.core_properties(), &expected_core);
    assert_eq!(read.app_properties(), &expected_app);
    assert_eq!(read.parts().len(), 2);
    assert_eq!(relationship_tuples(&read).len(), 2);
}

#[test]
fn rejects_case_folded_and_derivable_part_name_collisions() {
    let mut package = Package::new();
    package
        .insert_part(
            PartUri::parse("A").unwrap(),
            "application/octet-stream",
            vec![],
        )
        .unwrap();
    assert!(matches!(
        package.insert_part(
            PartUri::parse("a").unwrap(),
            "application/octet-stream",
            vec![]
        ),
        Err(OpcError::Duplicate(_))
    ));
    assert!(matches!(
        package.insert_part(
            PartUri::parse("a/b").unwrap(),
            "application/octet-stream",
            vec![]
        ),
        Err(OpcError::Duplicate(_))
    ));
}

#[test]
fn supports_iri_references_and_resolves_only_internal_target_paths() {
    let mut package = sample();
    let source = PartUri::parse("word/document.xml").unwrap();
    package
        .add_relationship(
            Some(source.clone()),
            "rIdFragment",
            "urn:test:relation",
            "#bookmark",
            TargetMode::Internal,
        )
        .unwrap();
    package
        .add_relationship(
            Some(source.clone()),
            "rIdQuery",
            "urn:test:relation",
            "?mode=print",
            TargetMode::Internal,
        )
        .unwrap();
    package
        .add_relationship(
            None,
            "rIdExternalRelative",
            "urn:test:relation",
            "../resource?q=1#part",
            TargetMode::External,
        )
        .unwrap();
    for bad in ["1foo:bar", "+foo:bar", ".foo:bar"] {
        assert!(
            package
                .add_relationship(
                    None,
                    format!("bad{}", bad.len()),
                    bad,
                    "relative",
                    TargetMode::External
                )
                .is_err()
        );
    }
    let bytes = package.write().unwrap();
    let read = Package::read(&bytes).unwrap();
    let rels = read.relationships(Some(&source));
    assert_eq!(
        rels.iter()
            .find(|r| r.id == "rIdFragment")
            .unwrap()
            .resolved_target
            .as_ref(),
        Some(&source)
    );
    assert_eq!(
        rels.iter()
            .find(|r| r.id == "rIdQuery")
            .unwrap()
            .resolved_target
            .as_ref(),
        Some(&source)
    );
    assert_eq!(
        read.relationships(None)
            .iter()
            .find(|r| r.id == "rIdExternalRelative")
            .unwrap()
            .target,
        "../resource?q=1#part"
    );
}

#[test]
fn validates_content_types_and_extension_syntax() {
    let mut package = Package::new();
    assert!(
        package
            .insert_part(
                PartUri::parse("valid.xml").unwrap(),
                "application/xml; charset=UTF-8",
                b"<x/>".to_vec()
            )
            .is_ok()
    );
    for (name, content_type) in [
        ("slash", "/type"),
        ("multi", "a/b/c"),
        ("param", "application/xml; bad param=x"),
    ] {
        assert!(
            package
                .insert_part(
                    PartUri::parse(&format!("{name}.bin")).unwrap(),
                    content_type,
                    vec![]
                )
                .is_err()
        );
    }
    let bytes = sample().write().unwrap();
    for extension in ["folder/ext", "bad ext", "bad?ext"] {
        let archive = Archive::open(&bytes, ZipLimits::default()).unwrap();
        let ct = archive
            .entries()
            .iter()
            .find(|e| e.name() == "[Content_Types].xml")
            .unwrap()
            .read_all()
            .unwrap();
        let xml = String::from_utf8(ct).unwrap().replace("</ns0:Types>", &format!("<ns0:Default Extension=\"{extension}\" ContentType=\"application/xml\"/></ns0:Types>"));
        let malformed = replace_zip_member(&bytes, "[Content_Types].xml", xml.as_bytes());
        assert!(
            Package::read(&malformed).is_err(),
            "accepted extension: {extension}"
        );
    }
}

#[test]
fn refuses_to_overwrite_a_part_name_when_materializing_properties() {
    let mut package = Package::new();
    package
        .insert_part(
            PartUri::parse("docProps/core.xml").unwrap(),
            "application/x-vendor-data",
            b"vendor bytes".to_vec(),
        )
        .unwrap();
    package.core_properties_mut().title = Some("New title".into());
    assert!(matches!(package.write(), Err(OpcError::Duplicate(uri)) if uri == "docProps/core.xml"));
}
