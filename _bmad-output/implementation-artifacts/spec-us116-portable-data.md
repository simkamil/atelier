---
title: 'US-116 — Résolution explicite du stockage portable'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: 'c091f6a52aa58af94966592ff356750471200d44'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="intent supplied by persistent user authorization">

## Intent

Résoudre le stockage avant de lancer la fenêtre ou WebView2. Le défaut est atelier-data à côté de l'exe ; un stockage portable inaccessible exige un choix utilisateur explicite et ne doit jamais créer automatiquement de cache dans LOCALAPPDATA.

## Boundaries & Constraints

Rust std seulement ; FFI des dialogues dans `data_win32.rs` avec justification de tous unsafe. Pas d'écriture au registre. API publique `data::resolve_data_paths(executable:&Path,local_appdata:Option<&Path>,choose:impl FnOnce(&DataError)->StartupChoice)->Result<StartupOutcome,DataError>`. Exposer `StartupChoice::{Local,Move,Quit}`, `StartupOutcome::{Ready(DataPaths),MoveRequired,Quit}`, `DataMode::{Portable,Local}`. `DataPaths` expose root/mode et accesseurs `preferences_file`, `window_state_file`, `recovery_dir`, `recent_file`, `crash_dir`, `webview_dir` tous sous une seule racine. Exposer `mode_label()` pour le titre utilisateur. Le coordinateur possède main/lib et relie le démarrage.

Résolution portable : chemin absolu de l'exe validé, racine parent/atelier-data, create_dir_all puis sonde unique create_new, écrire/sync/fermer/supprimer la sonde ; tester une écriture réelle, pas seulement un flag permissions. Ne pas effacer un fichier préexistant ni suivre un nom prévisible de sonde. La sonde et création de la seule racine ne sont pas des données utilisateur/cache ; aucun sous-dossier cache/récupération ne précède le choix. Échec : appeler choose une seule fois, transmettre raison exploitable ; Move et Quit terminent sans consulter/créer le chemin local. Local demande un LOCALAPPDATA absolu disponible, prépare LOCALAPPDATA/Atelier avec la même sonde ; échec local final bloque et ne boucle/replie jamais. Ne jamais créer de WebView avant Ready. Les accesseurs sont purs, le cache sera créé ultérieurement par US-111 à ce seul chemin.

Dialogues natifs français : expliquer stockage portable inaccessible et trois issues (déplacer l'exe vers un dossier inscriptible, choisir explicitement local, quitter). MessageBoxW Oui/Non/Annuler admis si le texte associe sans ambiguïté Oui=local, Non=déplacer et Annuler=quitter ; déplacer indique ensuite comment déplacer et quitte, sans déplacement automatique. `choose_data_mode(&DataError)->StartupChoice`, `show_startup_error(&DataError)` et `show_move_instructions()` disponibles sous Windows pour main. Aucun test ne déclenche de dialogue interactif : choix injecté et couverture pure de l'association des réponses Win32. Mode local visible dans le titre via mode_label. Toute erreur finale donne un message clair.

## I/O & Edge-Case Matrix

| Précondition / choix | Résultat | Effet local |
|---|---|---|
| Portable inscriptible | Ready Portable et chemins cohérents | aucun |
| Portable inaccessible / Quit | Quit | aucun |
| Portable inaccessible / Move | MoveRequired puis instructions et arrêt | aucun |
| Portable inaccessible / Local explicite | Ready Local sous LOCALAPPDATA/Atelier | préparation seulement après choix |
| Local absent/relatif/inaccessible | erreur finale claire | pas de repli |
| Collision sonde, échec I/O, chemin hostile | pas de fichier existant détruit, nettoyage et erreur | borné |

</frozen-after-approval>

## Code Map

- `SPEC.md` §7.2 et US-116 — une racine portable/local pour toutes les données gérées.
- `crates/desktop-win/src/main.rs` — intégration par coordinateur, initialize_dpi avant HWND/dialogues.
- `crates/desktop-win/src/lib.rs` — export data par coordinateur.
- `crates/desktop-win/src/window.rs` — agent US110, aucune dépendance requise : reçoit window_state_file et titre.

## Tasks & Acceptance

- [x] `crates/desktop-win/src/data.rs` — types/API, résolution/probe, chemins, messages et tests isolés sous target/temp.
- [x] `crates/desktop-win/src/data_win32.rs` — dialogues natifs sans autre écriture et correspondance des choix testable.
- [x] `docs/us-116-portable-data.md` — contrat de démarrage, choix, accesseurs, données gérées et tests.

Given un dossier portable inscriptible, when la résolution termine, then tous les chemins restent dans cette racine et aucun cache local n'apparaît. Given un échec portable, when Local n'est pas explicitement choisi, then aucune donnée locale n'est créée. Given Local inaccessible, when la résolution échoue, then le démarrage s'arrête avant tout cache et l'erreur est explicite. Given Windows, when la coque démarre en local, then le titre identifie ce mode.

## Implementation Notes

Propriété exclusive : tâches et notes append-only. Le coordinateur main/lib/CI/validation/Graphify/Git ; pas de sous-agent/skill/Graphify/commit/push. UTF-8/LF explicite. Tests en dossiers temporaires uniques, sans changer les ACL utilisateur ni manipuler vrais LOCALAPPDATA/exe du produit. Pour simuler portable inaccessible de manière reproductible, placer un fichier à la place du dossier atelier-data. Agent window peut être en cours : Cargo attendra les modules voisins.

## Spec Change Log

## Review Triage Log

| ID | Verdict | Constat | Preuve et décision | Route |
|---|---|---|---|---|
| B5 | false | create_dir_all écrirait à travers le lien racine avant rejet | Une racine qui est déjà un lien vers un dossier existant ne nécessite aucune création par create_dir_all ; symlink_metadata rejette ensuite le lien avant la sonde. Un lien cassé fait échouer la création. Le cas cité ne crée pas de descendant. | rejet |
| B10 | false | Le choix Local n’est pas mémorisé | Le contrat exige un choix explicite quand le portable est inaccessible, à chaque résolution ; aucune préférence persistante de basculement n’est demandée. Redemander conserve cette garantie et ne perd aucune donnée locale. | rejet |


## Verification

- `cargo test -p atelier-desktop-win --locked --offline` — branches portable/local/quit/move, chemins et erreurs.
- `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings` — zéro avertissement.

Les routes patch du journal sont résolues et couvertes par les tests ciblés. Le journal complet et ses preuves sont dans [worker-shell-review.md](worker-shell-review.md). Aucun constat différé ; clôture soumise à la validation globale.

Validation finale : 27 contrôles, 312 tests workspace et 21 tests shell release. Sources et lock identiques pendant les contrôles. Voir [preuve](worker-shell-validation.json) et [revue triée](worker-shell-review.md), aucun constat différé.
