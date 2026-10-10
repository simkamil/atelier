---
title: 'US-043 — Décodeurs GIF et BMP'
type: 'feature'
created: '2026-10-10'
status: 'done'
baseline_commit: '6e09261054a7cbb91bc63d270762ae15fea1131b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.5-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Le crate image ne sait pas décoder GIF et BMP, pourtant requis pour couvrir les images du MVP et la story d’insertion US-270.

**Approach:** Ajouter deux parseurs autonomes qui retournent l’image complète en pixels RGBA8. GIF produit la première image compositée sur l’écran logique; BMP prend en charge les profondeurs et modes usuels explicitement listés par US-043.

## Boundaries & Constraints

**Always:** API `decode_gif(data: &[u8], limits: DecodeLimits) -> Result<Image, ImageError>` et `decode_bmp(...)` dans `crates/image/src/gif.rs` et `bmp.rs`; `Image` est RGBA8 droit, sans conversion de profil/gamma. Respecter le contrat commun `DecodeLimits` (16 777 216 pixels et 64 MiB d’entrée par défaut) et `ImageError` (`Truncated`, `InvalidData`, `Unsupported`, `LimitExceeded`). Toute lecture, allocation, expansion LZW/RLE et taille calculée est bornée et vérifiée; entrée hostile = erreur, jamais panic. Aucun runtime externe ni `unsafe`.

GIF accepte GIF87a/89a, palette globale ou locale, extension de contrôle graphique/transparence, extensions non graphiques ignorées, LZW et ordre d’entrelacement quatre passes. Décoder uniquement le premier descripteur d’image, placé à son offset dans le canevas logique; Sans transparence GCE et avec palette globale, les pixels non peints prennent la couleur de fond logique; avec transparence GCE ou sans palette globale, le fond est transparent. Les indices transparents laissent le fond transparent. La sortie conserve les dimensions de l’écran logique. BMP accepte les profondeurs 1/4/8/16/24/32, `BI_RGB`, `BI_BITFIELDS`, `BI_RLE4` et `BI_RLE8` compatibles. Supporter lignes bottom-up et top-down non compressées; RLE est bottom-up seulement. Les palettes, masques, dimensions, pas de ligne, offsets et flux d’indices sont validés. `BI_RGB` 32 bits donne alpha 255; un vrai masque alpha de `BITFIELDS` est respecté. Les composantes masquées sont normalisées en 8 bits.

**Never:** Décoder les images suivantes d’un GIF, conserver les métadonnées/profils, corriger implicitement un fichier incohérent ou prétendre prendre en charge d’autres compressions BMP (JPEG/PNG/Huffman). Pas de seuil implicite supérieur aux limites communes.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| GIF valide | Palette locale, LZW, GCE transparent, frame entrelacée et décalée | Canevas logique RGBA du premier frame, lignes réordonnées, transparence conservée | N/A |
| GIF invalide | LZW tronqué, code invalide, palette/indice absent, rectangle hors écran | Aucun résultat partiel | `Truncated` ou `InvalidData` |
| BMP valide | 1/4/8/16/24/32 bits, RGB/BITFIELDS/RLE, orientation permise | Image RGBA avec couleurs et alpha définis par convention | N/A |
| BMP invalide | stride/masques/palette/RLE incohérents, profondeur-compression incompatible | Aucun accès hors plage ni pixel partiel | `Truncated` ou `InvalidData` |
| Limite | Dimension/produit de pixels ou entrée au-dessus des plafonds | Aucun buffer démesuré | `LimitExceeded` |

</frozen-after-approval>

## Code Map

- `crates/image/src/lib.rs` -- types communs `Image`, `DecodeLimits`, `ImageError` et exports, propriété du coordinateur.
- `crates/image/src/gif.rs` -- parseur GIF / LZW, premier frame uniquement.
- `crates/image/src/bmp.rs` -- parseur DIB, palette, masques, lignes et RLE.
- `crates/image/tests/gif_bmp.rs` -- fixtures et cas hostiles de la story.
- `US-V0-V1.md` -- US-043 et dépendance US-270; `SPEC.md` -- règle sans dépendance runtime et robustesse.
- [GIF89a](https://www.w3.org/Graphics/GIF/spec-gif89a.txt) -- structures, GCE, LZW et entrelacement.
- [MS-WMF BitmapInfoHeader](https://learn.microsoft.com/openspecs/windows_protocols/ms-wmf/567172fa-b8a2-4d79-86a2-5e21d6659ef3), [Compression Enumeration](https://learn.microsoft.com/openspecs/windows_protocols/ms-wmf/4e588f70-bd92-4a6f-b77f-35d0feaf7a57) -- BMP dimensions, modes et contrainte d’orientation RLE.

## Tasks & Acceptance

**Execution:**
- [x] `crates/image/src/gif.rs` -- décoder le premier frame, palettes, GCE, LZW, entrelacement et offsets -- remplir l’API sans dépasser les budgets.
- [x] `crates/image/src/bmp.rs` -- décoder pixels RGB, bitfields et RLE autorisés, dans les deux orientations permises -- produire les mêmes conventions RGBA.
- [x] `crates/image/tests/gif_bmp.rs` -- couvrir chaque cas de la matrice, troncatures et entrées adversariales -- prouver l’absence de panique et les erreurs.
- [x] `docs/us-043-gif-bmp.md` -- documenter API, formats exacts, transparence/alpha et limites -- permettre l’usage par US-270.

**Acceptance Criteria:**
- Given un GIF valide avec extensions, transparence, entrelacement et image décalée, when `decode_gif` est appelé, then il rend le premier frame dans le canevas logique avec pixels et alpha attendus.
- Given un BMP valide de chaque profondeur et mode supportés, when `decode_bmp` est appelé, then les pixels, l’orientation et les masques correspondent à la source, dont l’alpha `BITFIELDS`.
- Given une entrée tronquée, incohérente, à indice hors palette, avec flux RLE/LZW invalide ou dépassant une limite, when l’un des décodeurs est appelé, then il retourne une erreur stable sans panic ni allocation hors budget.
- Given un mode BMP non pris en charge ou incompatible avec sa profondeur, when il est décodé, then il retourne `Unsupported` ou `InvalidData` sans interprétation partielle.

## Implementation Notes

GIF implémente l’écran logique, la couleur de fond, les extensions GCE/commentaires, palettes locale/globale, LZW borné et les quatre passes d’entrelacement. BMP couvre les profondeurs demandées, RGB555/BGR/BGRX, BITFIELDS alpha, RLE4/RLE8 et fonds de palette pour les pixels RLE non peints. `biSizeImage` hors du fichier est rejeté.

Vérification : 12 tests `gif_bmp`, corpus oracle indépendant du coordinateur (201 cas) et compilation complète `cargo test -p atelier-image --locked --offline` passent. `cargo clippy -p atelier-image --lib --locked --offline -- -D warnings` passe. Le `cargo fmt --check` du crate signale uniquement des différences dans `tests/png.rs`, fichier appartenant au lot PNG parallèle.
Propriété exclusive : crates/image/src/gif.rs, crates/image/src/bmp.rs, crates/image/tests/gif_bmp.rs et docs/us-043-gif-bmp.md, et notes de ce spec. Le coordinateur gère façade/types partagés, manifests/lock, corpus externe, preuves, docs globales, CI, Graphify et Git. Ne pas lancer de skill, sous-agent ou Graphify; ne pas modifier les autres fichiers. Autorisation utilisateur persistante de développer/valider toutes les US en parallèle : les choix techniques ordinaires sont résolus ici.


## Spec Change Log


## Review Triage Log

Chaque constat est trié individuellement avant regroupement. Les corrections sont locales, sans nouvelle API produit. Les trois agents de revue sont GPT-6 Luna moyen. La revue Edge Case et la revue Verification Gap n’ont signalé aucun constat.

| ID | Constat | Verdict | Preuve et résolution |
|---|---|---|---|
| B1 | Virgule absente entre XML et font dans README | low | Constat exact sur le diff initial; correction directe de la liste. |
| B2 | Énumération des suivis avec deux et | low | Constat exact dans README; énumération corrigée directement. |
| B3 | Annonce obsolète du futur corpus images | low | tests/README annonce un futur corpus déjà livré; phrase remplacée. |
| B4 | Graphify pourrait traiter les fichiers RGBA | false | Les .rgba ne sont pas des extensions prises en charge : la CLI les classe explicitement non classifiés et ne les soumet ni à AST ni au LLM. Exclure leur extension ne corrige aucun traitement réel. |
| B5 | Comptage du corpus trop permissif | medium | valid >= 180 laisserait disparaître sept références; assert_eq!(valid,187) détecte désormais toute perte. |
| B6 | Empreintes du manifeste absentes de CI | medium | Le test de pixels ne validait pas les empreintes documentées. tests/check-assets.ps1 vérifie les 201 entrées et 187 références, l’égalité TSV/manifeste, et les 20 polices; exécuté dans le job Windows. |
| B7 | Mention des agents inutile au suivi | false | Le suivi est daté et cette information documente la préférence explicite de l’utilisateur pour GPT-6 Luna moyen. Aucun comportement ni invariant de produit ne diverge. |
| B8 | Rapport de revue exclu du diff/lien absent | false | prepare-images-fonts-review.py exclut le préfixe images-fonts-review- ; le rapport images-fonts-review.md ne correspond pas à ce motif. Il est produit après les revues et livré à ce chemin; il ne pouvait exister avant leur résultat. |
| B9 | PngSuite distribué depuis HTTP | low | Les deux hôtes HTTPS échouent à TLS; conserver le lien officiel, documenter le SHA figé et le corpus versionné sans téléchargement CI. Correction documentaire. |
| B10 | Validation de schéma annoncée trop générale | low | Commentaire remplacé par les contrôles réels; garde natif ajoutant types, valeurs, cardinalité, unicité, commits et chemins. Onze inventaires volontairement altérés doivent échouer. |
| R1 | Portée GCE après extension Plain Text | medium | Test ajouté d’abord en échec confirmé par l’agent : la transparence était héritée après un bloc graphique Plain Text. Nettoyer control après en-tête 12 octets et données ignorées; préserver contrôle après commentaire/application. Test Cargo passe. |
| R2 | Pixels sautés par RLE rendus transparents | medium | Le corpus indépendant échouait sur bmp-rle4-delta.bmp : alpha 0 aux pixels non peints. Initialisation avec palette[0] opaque; références RLE4/RLE8 et test dédié passent. |
| R3 | biSizeImage tronqué silencieusement | medium | Le min(file_end) masquait une taille déclarée excessive; garde renvoie Truncated. Régression : flux RLE réel de six octets annoncé à sept. |
| R4 | Entiers PNG dépassant 31 bits | medium | Les premières sources ne refusaient pas explicitement les champs supérieurs à 2^31-1. Guardes chunk et IHDR avant allocation; test max_pixels=u64::MAX couvre largeur, hauteur et longueur. |
| R5 | Signature de test &mut Vec inutile | low | Clippy all-targets refusait ptr_arg; set_file_size prend &mut [u8]. |
| R6 | matches!(Err(_)) redondant en test | low | Clippy all-targets refusait redundant_pattern_matching; assertion .is_err(). |



## Design Notes

GIF compose les pixels du premier descripteur d’image sur un canevas RGBA initialisé au fond logique opaque en l’absence de transparence GCE et si une palette globale existe; sinon le canevas est transparent. L’index transparent laisse alpha 0. Pour BMP, les lignes sont normalisées en coordonnées top-down. RLE4/RLE8 traitent EOL/EOB/delta, séquences absolues et alignement de mot; coordonnées et indices hors domaine sont rejetés.

## Verification

**Commands:**
- `cargo test -p atelier-image --locked --offline` -- attendu : tous les tests GIF/BMP passent.
- `cargo fmt -p atelier-image -- --check` -- attendu : format conforme.
- `py -m graphify .` -- coordinateur seulement après intégration.

Aucune commande de validation du code exécutée durant la phase de planification.



## Validation finale — 2026-10-10

Les quinze contrôles de `images-fonts-validation.json` passent : 210 tests workspace, 34 tests release images/polices, builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM, format, licences, garde, métadonnées et intégrité des ressources. Les sources et le lock sont inchangés pendant les contrôles. `images-fonts-oracle.json` confirme 201 entrées, 187 références régénérées à l’identique, 14 PNG invalides rejetés, 149 728 mutations sans panic, onze inventaires altérés rejetés et vingt faces FontTools. `images-fonts-upstream.json` confirme les vingt polices identiques aux sources officielles. Tous les constats sont triés, aucun différé. Le relevé `github-ci.json` identifie le commit et les jobs distants ; les tests musl sont exécutés sur Linux par la CI.
