---
title: 'US-111/114 — Hébergement WebView2 et runtime absent'
type: 'feature'
created: '2026-10-11'
status: 'in-progress'
baseline_commit: 'b72b0377832358f590e313602cbef9f55a4ecc6d'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.12-context.md'
---

<frozen-after-approval reason="persistent user authorization to develop stories in parallel">

## Intent

Héberger un vrai contrôle WebView2 dans la fenêtre Win32 existante et traiter proprement l'absence de runtime. Le prochain lot US-112/113 apportera les ressources de l'application et le pont natif. Cette livraison peut rester sur about:blank : elle n'affirme pas livrer un éditeur.

## Boundaries & Constraints

Rust std uniquement, FFI/COM extern system manuelles repr(C), unsafe justifié. SDK officiel Microsoft.Web.WebView2 1.0.4258.31 : data/webview2/include/WebView2.h est l'oracle ABI exact. Root importe chargeur statique x64 MSVC, licence et manifest, et possède build.rs. Aucun crate externe, DLL cherchée dans cwd/PATH ni export interne du runtime. Root possède main/lib/CI/Git/Graphify ; agent possède window.rs, win32.rs, nouveau webview.rs et docs/us-111-webview2.md. Aucun sous-agent, Graphify, commit/push ni tests workspace.

API window::run(state_file:&Path,title:&str,webview_data:&Path)->io::Result<()> ; root passe DataPaths.webview_dir seulement pour Ready. Ne jamais créer/résoudre une autre racine/cache avant Ready. Conserver --self-check natif existant indépendant du runtime. Fournir window::webview_self_check(state_file:&Path,webview_data:&Path,cycles:usize)->io::Result<String> sur Windows ; contrôler que chemins de test restent sous target avant toute création. Root l'expose via --webview-self-check (paths dérivés d'un seul dossier target fourni), avec 100 cycles bornés. Non-Windows : stubs cohérents.

Initialiser COM STA et équilibrer CoUninitialize après destruction du contrôleur et callbacks. QueryInterface reconnaît IUnknown et IID réel, AddRef/Release symétriques, erreurs HRESULT et pointeurs nuls contrôlés, aucune panique ne traverse FFI. Environnement puis contrôleur créés async ; callbacks ne gardent aucun pointeur temporaire vers un HWND/context détruit. Fermer avant fin d'initialisation, échec immédiat/asynchrone, timeout et destruction sont sûrs et bornés. Close avant Release, aucune référence cyclique ni use-after-free. Ne pas quitter avec callbacks auto-détenus encore vivants sans stratégie sûre de drainage/annulation ; documenter la stratégie réelle.

Adapter Bounds à client rect physique sur WM_SIZE/WM_DPICHANGED ; restaurer visibilité après minimize et MoveFocus(PROGRAMMATIC) sur WM_SETFOCUS. PMv2 reste activé avant HWND. Le DPI de WebView suit le moniteur du parent ; distinguer test de message synthétique et vrais écrans. Settings::put_AreDevToolsEnabled vaut cfg!(debug_assertions), donc false en release, et lire la valeur pour le self-check.

Détecter GetAvailableCoreWebView2BrowserVersionString avant CreateEnvironment, libérer version avec CoTaskMemFree. Runtime absent → dialogue natif français avec URL https://developer.microsoft.com/microsoft-edge/webview2/ et texte expliquant Ctrl+C pour copier le message et l'adresse ; aucun navigateur/installeur lancé automatiquement. Erreur runtime disparue pendant création également contrôlée. Tests injectés prouvent absence → aucun environnement/cache créé, Ready seulement → création, erreurs explicites. Le contrôle réel répète 100 créations/fermetures de contrôleur et confirme les références/callbacks applicatifs revenus au niveau initial ; compte handles avec échauffement et mesure réelle, sans seuil arbitraire masquant une fuite.

## I/O & Edge-Case Matrix

| État | Action | Attendu | Erreur |
|---|---|---|---|
| Ready/runtime présent | ouvrir | contrôleur réel dans client rect, cache sous racine fournie | HRESULT explicite |
| Move/Quit | démarrage | aucun environnement/cache | aucun accès caché |
| Runtime absent | probe échoue | message natif copiable, arrêt propre avant création | pas de crash |
| Resize/focus/DPI/minimize | messages parent | bounds/focus/visibilité adaptés | pas de pointeur mort |
| Initialisation async | fermeture/erreur/callback tardif | nettoyage une fois, fin bornée | erreur propagée |
| Debug/release | lire settings | DevTools true/false respectivement | lecture vérifiée |
| Répétition100 | ouvrir/fermer | contrôleurs/callbacks équilibrés | fuite fait échouer contrôle |

</frozen-after-approval>

## Code Map

- `crates/desktop-win/src/win32.rs` — Context boxed, HWND propriétaire, WM_SIZE/DPICHANGED/CLOSE et pump existants.
- `crates/desktop-win/src/window.rs` — API publique et stubs non-Windows, géométrie persistante.
- `crates/desktop-win/src/data.rs` — DataPaths.webview_dir est pur ; démarrage root contrôle Ready.
- `data/webview2/include/WebView2.h` — signatures, IIDs, ordre des vtables ; ne pas se fier à des indices mémorisés.

## Tasks & Acceptance

- [x] `webview.rs` — COM RAII/callbacks, probe, paramètres, diagnostics et tests ciblés.
- [x] `win32.rs`, `window.rs` — cycle de vie hébergé, bounds/focus/DPI, self-check100 et API.
- [x] `docs/us-111-webview2.md` — ABI/chargeur, démarrage et preuves/limites.
- [ ] Root main/lib/build/CI — Ready gate, chargeur statique et self-check automatisé.

Given Ready avec runtime, when la fenêtre change de taille/DPI/focus, then le vrai contrôle suit le parent et ferme proprement. Given runtime absent, when démarrage normal, then dialogue français copiable et aucune création d'environnement. Given100 cycles et release, when self-check tourne, then références applicatives équilibrées et DevTools désactivés. Les réglages/cache respectent la racine US-116 et le self-check natif d'US-110 reste autonome.

## Implementation Notes

Le coordinateur a finalisé COM/cleanup après la limite d’usage de l’agent. Le contrôle strict100 échoue sur les handles ; US-111 reste ouverte. Le wrapper CI publie le résultat strict et ne tolère que le diagnostic final de croissance après100 cycles fonctionnels sans callback restant. Voir frontend-host-exploration.json et docs/us-111-webview2.md. US-114 peut être validée indépendamment.

2026-10-11 — Module implémenté ; la validation de livraison ne clôt pas les critères matériels/intégration encore ouverts dans US-V0-V1.md.

## Spec Change Log

## Review Triage Log

| ID | Claim | Verdict | Route | Evidence |
|---|---|---|---|---|
| B1 | transform stack off-by-one | false | Rejected | transforms starts with identity (length1); before the128th push length128 is accepted, before129th length129 >128 rejects. >= would wrongly reject valid128th transform. Rust MAX_STACK_DEPTH counts pushed transforms, not identity. |
| B2 | rectangle extent exceeds coordinate bound | false | Rejected | Rust validate_rect checks finite extents and endpoint coordinates, not width<=MAX_COORDINATE. Two endpoints at ±1e150 yield2e150, still finite. JS matches this contract; no overflow or invalid accepted geometry demonstrated. |
| B3 | pageGap NaN/negative | low | Patch Canvas | Constructor accepted non-finite/negative pageGap, causing invalid page offsets or overlap. Direct finite/non-negative validation and browser assertions added. |
| B4 | invalid Canvas callbacks | low | Patch Canvas | Non-function resolver/onDiagnostic options failed during replay and could replace diagnostics. Constructor now validates callback types, covered in Chromium. |
| B5 | offscreen overlays replayed | low | Rejected | Offscreen overlay iteration is real, while canvas clip prevents incorrect pixels. Normal composition uses a small rectangle set; filtering adds another branch without an everyday defect or measured material regression. |
| B6 | benchmark always green | low | Patch report label | The culling assertions are functional; timing is explicitly headless/informative in docs and has no physical60fps claim. Add informational:true directly to the measurement record to make this distinction machine-readable. |
| B7 | selection clone fallback aliases owner | medium | Patch input | structuredClone failure returned the original object, breaking the stable composition snapshot. Reject through existing onError path; clone outgoing payloads as well to prevent callbacks mutating internal snapshot. |
| B8 | IME duplicate after zero timer | maybe-false | Defer unverified medium | Synthetic delayed insertText can trigger this path; a real WebView2 event sequence crossing this task boundary has not been established. Indefinite suppression demonstrably drops later identical user text. Capture native IME traces at US-103 manual acceptance to settle ordering. |
| B9 | paste duplicate after zero timer | false | Rejected | onPaste cancels the native paste default before routing. A successful canceled paste does not subsequently generate native insertion; synthetic unrelated later insertFromPaste must not be suppressed indefinitely. Same-task duplicate protection remains tested. |
| B10 | focusable aria-hidden textarea | medium | Patch input | Created textarea was focusable and aria-hidden, hiding the active input from accessibility APIs. Remove aria-hidden, add an accessible label and owner override documentation; browser DOM assertions cover the direct semantics. |
| E1 | unsupported edit delegated twice | medium | Patch input | Successful onUnhandled in beforeinput was called again for matching input. Reuse existing same-task handled-input suppression; retain fallback when first callback failed and native text when no delegate exists. |
| E2 | noncloneable composition selection | medium | Patch input; same cause B7 | Verified separately: copySelection returned an alias after clone failure; subsequent owner mutation affected commit. The B7 correction also covers this outcome. |
| E3 | pageGap invalid | low | Patch Canvas; same cause B3 | Verified separately: adding NaN/Infinity to top made later pages non-visible; negative gap overlapped. Constructor validation covers this outcome. |

## Verification

- `cargo test -p atelier-desktop-win --locked --offline` — tests ciblés natifs et doubles COM.
- `cargo clippy -p atelier-desktop-win --all-targets --locked --offline -- -D warnings` — aucun avertissement.
- `atelier.exe --webview-self-check target/webview-check` — root lance100 cycles réels en debug/release.

Module livré et revu : 34 contrôles fonctionnels/qualité, 318 tests workspace, 27 tests shell release. US-114 validée ; US-101/103/111 restent ouvertes pour les critères explicités au backlog. Voir frontend-host-validation.json et frontend-host-review.md.
