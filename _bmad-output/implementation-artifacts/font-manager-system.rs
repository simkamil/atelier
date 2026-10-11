// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::manager::{FontManager, FontRequest, FontSource, FontStyle};

fn main() {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(paths.len(), 4);
    let bytes: Vec<_> = paths.iter().map(|p| std::fs::read(p).unwrap()).collect();
    let mut manager = FontManager::new();
    let ids: Vec<_> = bytes
        .iter()
        .map(|b| manager.register(b, 0, FontSource::System).unwrap())
        .collect();
    let variants = [
        (400, FontStyle::Normal),
        (700, FontStyle::Normal),
        (400, FontStyle::Italic),
        (700, FontStyle::Italic),
    ];
    for (index, (weight, style)) in variants.into_iter().enumerate() {
        let request = FontRequest::new(["Arial"], weight, style);
        let actual = manager.resolve_for_char(&request, 'A').unwrap();
        assert_eq!(actual.id, ids[index]);
        assert_eq!(actual.family, "Arial");
        assert_eq!(actual.weight, weight);
        assert_eq!(actual.style, style);
        assert_eq!(actual.source, FontSource::System);
        println!(
            "{}\t{}\t{:?}\t{}",
            actual.family,
            actual.weight,
            actual.style,
            actual.id.get()
        );
    }
}
