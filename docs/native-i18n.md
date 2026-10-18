# Textes du shell natif et i18n

Les textes visibles affichés par `crates/desktop-win` passent par le catalogue `web/i18n/fr-FR.txt` (bloc `# Shell natif`, clés `native.*`, 35 entrées). Aucun comportement n'a changé : les textes français sont identiques à ceux qui étaient écrits en dur.

## Module `crates/desktop-win/src/i18n.rs`

- Lit le catalogue embarqué (`include_bytes!` de `web/i18n/fr-FR.txt`, le même fichier que celui servi à la page web sous `/i18n/fr-FR.txt`), analysé une fois (`OnceLock`).
- Même format que `web/i18n.mjs` (`docs/us-102-i18n.md`) : `clé=valeur`, première `=` séparatrice, lignes vides et `#` ignorées, BOM et CRLF acceptés, clé `[A-Za-z0-9][A-Za-z0-9._-]*`, dernière occurrence gagnante, paramètres nommés `{nom}`, échappements `\n \\ \{ \} \#` (toute autre séquence est une erreur).
- Pas de pluriels ICU : aucune chaîne native n'en a besoin ; un message qui en contient est invalide côté natif.
- `t(clé, &[("nom", valeur)])` ne panique jamais : clé absente ou message invalide rend `⟦clé⟧` (comme le JS) ; paramètre manquant laisse `{nom}` visible ; les valeurs sont insérées telles quelles, jamais réinterprétées.
- Les clés d'opération de `DataError::StorageUnavailable` (`operation`) sont des clés du catalogue, traduites à l'affichage.

## Typographie

Dans le bloc natif, l'espace avant `?`, `:`, `;` et à l'intérieur des guillemets `« »` est une espace fine insécable U+202F ; seule la largeur des espaces change par rapport aux anciens textes (U+0020), le texte reste le même. Le reste du catalogue (hors bloc natif, non modifié) utilise encore U+0020 avant `:`, `;` et `?` : à harmoniser dans une story dédiée.

## Contrôles

- `cargo test -p atelier-desktop-win` : vecteurs communs `tests/i18n-vectors.txt` (lus aussi par `tests/i18n.mjs`), analyse du catalogue (lignes invalides, clés invalides, doublons, BOM/CRLF), paramètres et échappements, et `every_native_key_used_exists` (toute clé `"native.…"` écrite dans `src/**/*.rs` existe dans le catalogue et se compile).
- `node tests/check-i18n.mjs` lit aussi les littéraux `"native.…"` de `crates/desktop-win/src/**/*.rs` (hors `i18n.rs` et hors éléments `#[cfg(test)]` : une clé utilisée seulement en test reste « inutilisée »)  : une clé native utilisée par le code de production n'est donc pas « inutilisée », une clé manquante est `missing-key`, et les paramètres littéraux de `t("clé", &[("nom", …)])` sont comparés à ceux du message (`param-missing`, `param-extra`). Tests dans `tests/i18n.mjs`.

## Inventaire (fichier:ligne, après migration)

| Clé(s) | Emplacement | Usage |
|---|---|---|
| `native.data.error.*` (5), `native.data.error.storage_unavailable` | `data.rs:50-71` | `Display` de `DataError`, montré dans les boîtes de démarrage |
| `native.data.op.*` (7), `native.data.detail.probe_collisions` | `data.rs:203-253` | opération et détail de `StorageUnavailable` |
| `native.mode.portable`, `native.mode.local` | `data.rs:135-136` | libellé de mode dans le titre de la fenêtre |
| `native.startup.storage.prompt` / `.caption` | `data_win32.rs:46-51` | choix Oui/Non/Annuler du stockage portable |
| `native.startup.error.text` / `.caption` | `data_win32.rs:61-64` | erreur de démarrage finale |
| `native.startup.move.text` / `.caption` | `data_win32.rs:71-72` | consigne de déplacement de l'exécutable |
| `native.startup.invalid_argument` | `main.rs:152` | argument de ligne de commande invalide |
| `native.startup.already_running` | `main.rs:188` | instance existante qui ne répond pas |
| `native.window.title` | `main.rs:204` | titre de la fenêtre (« Atelier — Portable ») |
| `native.webview.runtime_missing` | `webview.rs:1146` | diagnostic du runtime WebView2 absent (US-114) |
| `native.dialog.open_text.title`, `open_docx.title`, `save_text.title`, `filter.text`, `filter.docx`, `save_text.suggested_name` | `win32.rs:382-397` | titres, filtres et nom proposé des boîtes de fichiers du chemin natif de repli |
| `native.close.pending_io` | `win32.rs:1249` | « Quitter quand même ? » (écriture ou lecture en cours, US-113) |
| `native.close.unsaved` | `win32.rs:1294` | « modifications non enregistrées » à la fermeture |
| `native.app.name` | `win32.rs:1253, 1298, 2593` | légende (`caption`) des `MessageBoxW` |

Les titres et filtres des boîtes ouvertes par le pont de fichiers (`bridge_ops.rs`) viennent du JS (spécification base64) : rien à migrer.

## Laissé en dur, et pourquoi

- Titres de fenêtres de contrôle (`"Atelier window self-check"`, `"Atelier WebView2 self-check"`, `"Atelier bridge self-check"`, `"Atelier probe"` : `win32.rs:1729, 1894`, `bridge-check.rs:193`, `instance_win32.rs:387`) : modes de diagnostic non interactifs.
- Messages `io::Error` en anglais (`"data root unavailable"`, erreurs `hresult_error`, etc.), sorties `eprintln!` des modes `--*-self-check` et journaux : diagnostics de développeur.
- Textes de test et littéraux de `bridge_ops.rs` / `fonts.rs` / `instance.rs` en test (exemples avec accents).
- Clés `files.error.*`, `url.error.*` : déjà des clés de catalogue transmises à la page, hors périmètre.
- Aucun repli en dur n'a été nécessaire pour les échecs très précoces : le catalogue est embarqué dans l'exécutable, donc toujours lisible ; seul un message invalide ou absent donnerait `⟦clé⟧` (les tests l'empêchent pour les clés utilisées).

## Clés brutes jamais montrées

`win32.rs` : l'échec de `FileRuntime::new` au démarrage passe par `startup_file_error`, qui affiche `t(message_key)` (test `startup_file_errors_show_the_catalogue_text_not_the_key`). Les autres usages de `message_key` (`win32.rs:529`, `2760`, `2779`) renvoient la clé à la page, qui la traduit ; aucun autre chemin natif ne l'affiche.
