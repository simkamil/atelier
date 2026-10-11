# Revue du lot Canvas, saisie et hébergement natif — 2026-10-11

Trois agents GPT-6 Luna, raisonnement moyen, contexte vierge, lancés ensemble.
Baseline b72b0377832358f590e313602cbef9f55a4ecc6d. Diff initial exhaustif (suivis et non suivis) :6 786 410 octets, SHA2569343439ce1948b98521b7c0f38021aad6a22c102bc55a99d3b5245ebdefe40e6.
Blind hunter : floor min(floor(sqrt(6627.35)+1),10)=10, dix constats.
Edge hunter : trois constats après traces, claims lus selon ses instructions.
Verification-gap : « No verification gaps found. »
Chaque constat est jugé individuellement avant regroupement. Les causes communes B3/E3 et B7/E2 partagent leurs corrections, sans supprimer de ligne.

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

Le coordinateur a également supprimé les doubles dialogues d’erreur natifs,
propagé les erreurs Close, retenu/libéré l’environnement sur le STA et
sérialisé les tests de compteurs globaux. La croissance de handles n’est pas
résolue et US-111 reste ouverte ; elle est consignée dans l’exploration et dans
le résultat strict, distinct du succès fonctionnel CI. Aucun seuil ni fermeture
forcée d’un handle de propriétaire inconnu n’a été ajouté.

Un constat reste différé, de gravité moyenne non vérifiée : ordre natif IME
à travers plusieurs tâches. Les tests synthétiques ne permettent pas de
l’établir. Les critères matériels et d’intégration US-101/103 restent ouverts.

Revue complémentaire du wrapper GUI release par le reviewer verification-gap,
après la correction ProcessStartInfo : aucun problème réel restant identifié.
La sortie vide, un processus encore actif, toute erreur fonctionnelle et un
réglage DevTools incohérent ne sont pas acceptés. Le timeout tue uniquement
l’arbre du processus diagnostique possédé, puis attend sa terminaison.
Les agents d’origine ont corrigé Canvas/input et repassé leurs tests ciblés.
