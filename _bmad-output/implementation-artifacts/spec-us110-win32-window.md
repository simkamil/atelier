---
title: 'US-110 — Fenêtre Win32 persistante et DPI'
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

Fournir la fenêtre native qui hébergera WebView2. La coque doit démarrer, recevoir les messages Win32, suivre le thème système et le DPI par écran, et restaurer une géométrie utilisable après redémarrage ou retrait d'un écran. Cette US ne prétend pas afficher un éditeur.

## Boundaries & Constraints

Rust std uniquement, FFI `extern system` manuelle isolée dans `win32.rs`, blocs unsafe avec justification de validité et de durée de vie. Tous les types externes ont repr(C), signatures conformes au SDK. API publique `window::initialize_dpi()->std::io::Result<()>` à appeler avant tout HWND ; `window::run(state_file:&Path,title:&str)->io::Result<()>` utilise le chemin déjà résolu par US-116, sans chercher une autre racine. Le coordinateur possède lib.rs/main.rs et les relie ; ne pas modifier ces fichiers. Tout HWND et contexte sont libérés une fois, le callback FFI ne laisse jamais une panique traverser l'ABI et ne garde pas de pointeur temporaire. Éviter état global mutable non protégé. Pas de dépendance crate/SDK généré/COM/WebView2 à ce stade.

Activer Per-Monitor V2 avant toute création. Suivre WM_DPICHANGED en appliquant le rectangle conseillé, traiter WM_SIZE et conserver la future surface cliente accessible. Barre système standard, mode clair/sombre via DwmSetWindowAttribute et réglage système lu en lecture seule, réactualisé sur WM_SETTINGCHANGE/WM_THEMECHANGED. Ne jamais écrire le registre. Boucle GetMessageW distingue erreur, quit et message ; WM_CLOSE/WM_DESTROY assurent la fermeture et la persistance sans boucler après quit.

État versionné borné : rectangle normal, DPI, maximisé. Ne pas mémoriser une géométrie minimisée comme état normal. Écrire dans le fichier fourni, remplacement atomique dans le même dossier, erreur explicite sans perdre le précédent. La restauration choisit l'écran concerné ou le plus proche, convertit correctement les coordonnées workspace/screen de WINDOWPLACEMENT, puis borne taille/position à son work area, y compris écrans négatifs et changement DPI. La taille minimale doit rester utilisable sans dépasser un petit écran. Un fichier absent/corrompu/trop grand utilise un défaut sûr ; ne pas allouer selon une longueur arbitraire.

Fournir `window::self_check(state_file:&Path)->io::Result<String>` sous Windows : crée une vraie fenêtre cachée, vérifie DPI et surface cliente, envoie un changement DPI contrôlé, teste sauvegarde/restauration et détruit la fenêtre sans interaction visible. L'état de test reste sous target. Les tests géométriques doivent couvrir plusieurs écrans synthétiques (ce n'est pas une preuve d'un vrai dispositif multi-écran).

## I/O & Edge-Case Matrix

| État | Comportement | Échec |
|---|---|---|
| Première ouverture | fenêtre normale, PMv2, surface cliente non vide | io::Error |
| Rectangle/maximisé mémorisés | état normal et maximisé restaurés | état corrompu → défaut |
| Écran retiré, coordonnées négatives, DPI différent | fenêtre bornée dans work area | aucune panique |
| Minimise puis ferme | rectangle normal préservé | persistance explicite |
| Changement DPI/thème | rectangle conseillé et barre système adaptés | erreur API contrôlée |
| Propriétés/format invalides, fichier énorme | défaut borné, lecture limitée | aucune panique |

</frozen-after-approval>

## Code Map

- `crates/desktop-win/src/main.rs` — squelette, intégration par coordinateur.
- `crates/desktop-win/src/lib.rs` — exports data/window par coordinateur.
- `SPEC.md` §6.8, §7.2 — FFI système et racine portable commune.
- `crates/desktop-win/src/data.rs` — autre agent, ne pas modifier ; le chemin d'état arrive en paramètre.

## Tasks & Acceptance

- [x] `crates/desktop-win/src/window.rs` — état versionné, géométrie sûre, orchestration, API et tests purs.
- [x] `crates/desktop-win/src/win32.rs` — liaisons, boucle de messages et propriétaire HWND, PMv2/thème, persistance native et self_check.
- [x] `docs/us-110-win32-window.md` — APIs, format d'état, validation et limites matérielles.

Given Windows, when la coque démarre puis ferme, then la boucle est saine et taille/position/maximisé sont persistés. Given un écran disparu ou un état hostile, when la fenêtre est restaurée, then la barre et surface restent dans une work area. Given un test natif caché, when DPI change, then la fenêtre suit le rectangle demandé et ferme proprement.

## Implementation Notes

Propriété exclusive : tâches et notes append-only. Coordinateur possède main/lib/CI/Graphify/Git. Pas de sous-agent/skill/Graphify/commit/push. UTF-8/LF explicite, commentaires SPDX. Agent voisin data peut être encore en cours : attendre son fichier avant Cargo ou utiliser rustc isolé si nécessaire. Le coordinateur appelle initialize_dpi avant la résolution portable ; local est identifié dans le titre transmis à run.

## Spec Change Log

## Review Triage Log

| ID | Verdict | Constat | Preuve et décision | Route |
|---|---|---|---|---|
| B1 | false | DPI système pour tous les écrans | run et self_check créent d’abord un HWND caché sur le rectangle borné, lisent GetDpiForWindow puis remplacent le DPI de cet écran et recalculent la restauration avant affichage ; les autres écrans provisoires ne déterminent pas la taille finale. | rejet |
| B2 | false | Les coordonnées écran seraient multipliées par le DPI | restore_state applique scale seulement à width/height ; left/top sont bornés sans multiplication. Le déplacement décrit ne se produit pas. | rejet |
| B3 | low | Taille initiale fixe en pixels physiques | default_for ignore le DPI pour 1024x720 : à 192 DPI la taille logique est divisée par deux. Correction directe des dimensions et bornage. | patch fenêtre |
| B4 | medium | Minimum fixe en pixels physiques | restore_state et WM_GETMINMAXINFO utilisent 640x420 fixes : à 192 DPI le minimum logique descend à 320x210. Même cause que B3, helper partagé et tests 192 DPI. | patch fenêtre |
| B6 | medium | Le self-check crée un descendant avant de rejeter une jonction sortante | Le chemin target/jonction/manquant passe le contrôle lexical, create_dir_all crée manquant au-delà de target puis canonicalize refuse. Valider les ancêtres existants avant toute création. | patch fenêtre |
| B8 | maybe-false | Échec de SetWindowLongPtrW ignoré pendant WM_NCCREATE | La valeur de retour n’est pas contrôlée, mais le HWND neuf appartient au même thread/processus et GWLP_USERDATA est valide : aucun scénario atteignable d’échec n’est démontré. La documentation Microsoft précise zéro+GetLastError, sans montrer un échec avec ces arguments. Il faudrait une injection d’échec natif ; impact non établi et correction ajoutant des branches, rejet du durcissement hypothétique. | rejet (faible si confirmé) |
| E2 | false | Énumération interrompue accepterait une liste incomplète | collect renvoie FALSE sur erreur et monitors rejette EnumDisplayMonitors==0. Vérification Win32 réelle via ctypes : callback appelé une fois puis FALSE, résultat EnumDisplayMonitors=0 ; le résultat incomplet n’est pas rendu. | rejet |
| E3 | medium | Jonction self-check et descendant absent | Vérifié séparément : create_dir_all précède la vérification canonique et peut écrire hors target. Même défaut que B6. | patch fenêtre |
| V1 | medium | Les actions du démarrage normal ne sont pas vérifiées | Constat pré-vérifié : tests resolver et self-check contournent le mapping main des trois outcomes. Vérifier le coordinateur privé avec le vrai resolver et les actions injectées pour chemin/titre/arrêt, sans nouvelle API publique. | patch intégration |


## Verification

- `cargo test -p atelier-desktop-win --locked --offline` — tests état/géométrie et vrai HWND caché sous Windows.
- `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings` — aucun avertissement.

Les routes patch du journal sont résolues et couvertes par les tests ciblés. Le journal complet et ses preuves sont dans [worker-shell-review.md](worker-shell-review.md). Aucun constat différé ; clôture soumise à la validation globale.

Validation finale : 27 contrôles, 312 tests workspace et 21 tests shell release. Sources et lock identiques pendant les contrôles. Voir [preuve](worker-shell-validation.json) et [revue triée](worker-shell-review.md), aucun constat différé.
