// SPDX-License-Identifier: AGPL-3.0-or-later
//! Measured accuracy of the automatic encoding detection (US-316) on
//! realistic French text. A detection is "correct" when the text it decodes
//! equals the original (so an ASCII-only text read as UTF-8 or Windows-1252
//! counts as correct: both read the same).

use atelier_text_model::plain_io::*;
use atelier_unicode::{Encoding, encode};

const SENTENCES: [&str; 20] = [
    "Le été dernier, nous avons visité la Bretagne et goûté des crêpes.",
    "Où est passée la clé de l'appartement ?",
    "Ça m'étonne : elle n'a jamais été en retard.",
    "Les élèves ont rédigé un compte rendu détaillé.",
    "Il faut réfléchir avant d'agir, même à Noël.",
    "L'œuvre complète de Hugo est disponible à la bibliothèque.",
    "Prix : 12,50 € par personne, réservation obligatoire.",
    "« Bonjour », dit-il en entrant dans la pièce.",
    "Garçons et filles se sont réunis près de l'église.",
    "Le bœuf bourguignon mijote depuis hier soir.",
    "Zoé a envoyé un message à Hélène à côté de Mâcon.",
    "Aucun accent dans cette phrase, juste du texte simple.",
    "Rendez-vous demain a quatorze heures devant la gare.",
    "Voilà une très belle journée pour être dehors, n'est-ce pas ?",
    "Françoise préfère le thé, Émile préfère le café.",
    "Les ingénieurs ont déjà vérifié l'étanchéité du réservoir.",
    "Cet été, la chaleur était écrasante : plus de 40 °C à l'ombre.",
    "Ils ont dû évacuer le bâtiment après l'alerte incendie.",
    "Quelques mots : naïveté, coïncidence, maïs, noël, aïeul.",
    "Fin du document.",
];

/// Single sentences, pairs and triples (one sentence per line).
fn corpus() -> Vec<String> {
    let mut texts = Vec::new();
    for i in 0..SENTENCES.len() {
        let n = SENTENCES.len();
        texts.push(SENTENCES[i].to_owned());
        texts.push(format!("{}\n{}", SENTENCES[i], SENTENCES[(i + 1) % n]));
        texts.push(format!(
            "{}\r\n{}\r\n{}\r\n",
            SENTENCES[i],
            SENTENCES[(i + 3) % n],
            SENTENCES[(i + 7) % n]
        ));
    }
    texts
}

fn utf16(text: &str, little: bool, bom: bool) -> Vec<u8> {
    let encoding = if little {
        Encoding::Utf16Le
    } else {
        Encoding::Utf16Be
    };
    let mut bytes = Vec::new();
    if bom {
        bytes.extend_from_slice(if little { b"\xFF\xFE" } else { b"\xFE\xFF" });
    }
    bytes.extend(encode(text, encoding).unwrap());
    bytes
}

fn correct(bytes: &[u8], original: &str) -> bool {
    let opened = open_text(bytes, &OpenOptions::default()).unwrap();
    opened.text == original.replace("\r\n", "\n")
}

#[test]
fn measured_accuracy_on_french_text() {
    let texts = corpus();
    type Maker = fn(&str) -> Vec<u8>;
    let classes: [(&str, Maker, bool); 7] = [
        ("UTF-8", |t| t.as_bytes().to_vec(), true),
        (
            "UTF-8 + BOM",
            |t| [b"\xEF\xBB\xBF".as_slice(), t.as_bytes()].concat(),
            true,
        ),
        (
            "Windows-1252",
            |t| encode(t, Encoding::Windows1252).unwrap(),
            true,
        ),
        ("UTF-16 LE + BOM", |t| utf16(t, true, true), true),
        ("UTF-16 BE + BOM", |t| utf16(t, false, true), true),
        ("UTF-16 LE no BOM", |t| utf16(t, true, false), true),
        ("UTF-16 BE no BOM", |t| utf16(t, false, false), true),
    ];
    let (mut ok_total, mut total) = (0, 0);
    for (name, make, must_be_perfect) in classes {
        let ok = texts.iter().filter(|t| correct(&make(t), t)).count();
        eprintln!("{name:18} {ok}/{}", texts.len());
        if must_be_perfect {
            assert_eq!(ok, texts.len(), "{name}");
        }
        ok_total += ok;
        total += texts.len();
    }
    eprintln!("detection on French text: {ok_total}/{total} correct");
    assert_eq!(ok_total, total);
}

#[test]
fn structurally_undecidable_cases() {
    // Windows-1252 bytes that happen to be valid UTF-8: read as UTF-8.
    let mojibake = encode("cafÃ© crÃ¨me", Encoding::Windows1252).unwrap();
    let opened = open_text(&mojibake, &OpenOptions::default()).unwrap();
    assert_eq!(
        (opened.encoding, opened.text.as_str()),
        (Encoding::Utf8, "café crème")
    );
    assert_eq!(opened.confidence, Confidence::Likely);

    // A single non-ASCII character in UTF-16 without BOM: one unit is not
    // enough evidence.
    let lone = utf16("é", true, false);
    assert_eq!(
        open_text(&lone, &OpenOptions::default()).unwrap().encoding,
        Encoding::Windows1252
    );
    // Non-Latin UTF-16 without BOM has no null bytes.
    let chinese = utf16("中文汉字", true, false);
    assert_ne!(
        open_text(&chinese, &OpenOptions::default()).unwrap().text,
        "中文汉字"
    );
    // The same bytes with the encoding imposed read perfectly.
    let forced = OpenOptions {
        encoding: Some(Encoding::Utf16Le),
        ..OpenOptions::default()
    };
    assert_eq!(open_text(&chinese, &forced).unwrap().text, "中文汉字");
    // Fallback 1252 is always flagged as uncertain.
    let latin = encode("été", Encoding::Windows1252).unwrap();
    assert_eq!(
        open_text(&latin, &OpenOptions::default())
            .unwrap()
            .confidence,
        Confidence::Uncertain
    );
}
