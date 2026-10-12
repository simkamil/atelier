// SPDX-License-Identifier: AGPL-3.0-or-later
use atelier_font::{
    embedded::EMBEDDED_FONTS,
    manager::{FontManager, FontManagerLimits, FontSource},
};

#[test]
fn cache_tokens_distinguish_managers_and_survive_moves() {
    let first = FontManager::empty();
    let second = FontManager::empty();
    assert_ne!(first.cache_revision(), second.cache_revision());
    let token = first.cache_revision();
    let moved = first;
    assert_eq!(moved.cache_revision(), token);
}

#[test]
fn successful_mutations_renew_tokens_and_failures_preserve_them() {
    let mut fonts = FontManager::empty();
    let initial = fonts.cache_revision();
    assert!(fonts.register(&[], 0, FontSource::Document).is_err());
    assert_eq!(fonts.cache_revision(), initial);
    let id = fonts
        .register(EMBEDDED_FONTS[0].data, 0, FontSource::Document)
        .unwrap();
    let registered = fonts.cache_revision();
    assert_ne!(registered, initial);
    fonts.remove(id).unwrap();
    let removed = fonts.cache_revision();
    assert_ne!(removed, registered);
    assert_ne!(removed, initial);
    assert!(fonts.remove(id).is_err());
    assert_eq!(fonts.cache_revision(), removed);
    let mut limited = FontManager::with_limits(FontManagerLimits {
        max_faces: 0,
        max_total_bytes: 0,
    });
    let token = limited.cache_revision();
    assert!(
        limited
            .register(EMBEDDED_FONTS[0].data, 0, FontSource::Document)
            .is_err()
    );
    assert_eq!(limited.cache_revision(), token);
}

#[test]
fn concurrent_manager_creation_does_not_alias_tokens() {
    let mut tokens = std::thread::scope(|scope| {
        let tasks: Vec<_> = (0..16)
            .map(|_| scope.spawn(|| FontManager::empty().cache_revision()))
            .collect();
        tasks
            .into_iter()
            .map(|task| task.join().unwrap())
            .collect::<Vec<_>>()
    });
    tokens.sort_unstable();
    tokens.dedup();
    assert_eq!(tokens.len(), 16);
}
