---
title: 'Décodeur PNG (US-040)'
type: 'feature'
created: '2026-10-10'
status: 'done'
baseline_commit: '6e09261054a7cbb91bc63d270762ae15fea1131b'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.5-context.md'
  - '{project-root}/SPEC.md'
  - '{project-root}/US-V0-V1.md'
  - '{project-root}/crates/image/src/lib.rs'
  - '{project-root}/crates/compress/src/lib.rs'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Les documents contiennent des PNG qu’il faut afficher, mais le crate image est encore un squelette. Les données sont non fiables et le flux zlib peut se décompresser bien au-delà de sa taille compressée.

**Approach:** Ajouter un décodeur PNG synchrone, borné et sans dépendance externe dans `atelier-image`. Il valide la structure et les CRC, décompresse exactement les scanlines attendues, inverse les filtres et convertit tous les formats PNG statiques en pixels RGBA8 droits.

## Boundaries & Constraints

**Always:** `#![forbid(unsafe_code)]`, en-tête SPDX existant, erreurs typées sans panic. API : `Image { width: u32, height: u32, pixels: Vec<u8> }`, `DecodeLimits { max_pixels: u64, max_input_bytes: usize }` avec valeurs par défaut 16 777 216 pixels et 64 MiB, `ImageError::{Truncated, InvalidData(&'static str), Unsupported(&'static str), LimitExceeded}`, et `decode_png(data: &[u8], limits: DecodeLimits) -> Result<Image, ImageError>`. `pixels` est RGBA8, alpha droite; aucun ajustement gamma/profil colorimétrique. Étendre les échantillons à 8 bits par mise à l’échelle entière arrondie vers la valeur la plus proche; comparer `tRNS` à la précision source avant conversion. Refuser dimensions nulles, dépassements arithmétiques, produits `width × height` ou taille d’entrée au-delà des limites, avant allocation correspondante. Réutiliser `atelier-compress` et imposer exactement la longueur décompressée calculée, checksum zlib correct et consommation de toutes les données IDAT.

**Never:** dépendance Rust externe, `unsafe`, décodage JPEG/GIF/BMP, conversion ICC/gamma, allocation pilotée par une longueur PNG non validée, octets après IEND, ou octets excédentaires dans le flux zlib.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| PNG valide | Un ou plusieurs IDAT, toutes profondeurs/types légaux | RGBA8 identique au rendu de référence, dimensions conservées | Succès |
| Troncature / CRC faux | Signature, chunk, payload, CRC ou zlib incomplet/corrompu | Aucun résultat partiel | `Truncated` ou `InvalidData` |
| Limite | Entrée ou nombre de pixels au-dessus du budget | Refus avant allocation disproportionnée | `LimitExceeded` |
| Format réservé | Type critique inconnu, méthode ou combinaison illégale | Aucun décodage implicite | `Unsupported` ou `InvalidData` |

</frozen-after-approval>

## Code Map

- `crates/image/src/png.rs` — parseur de chunks, validation, inflate, filtres, Adam7 et conversion RGBA8.
- `crates/image/src/lib.rs` — façade et types publics (exports détenus par le coordinateur).
- `crates/compress/src/lib.rs` — `InflateState` zlib plafonné, checksum Adler-32 et `crc32`.
- `crates/image/tests/png.rs` — fixtures construites en tests pour chemins et rejets ciblés.
- `docs/us-040-png.md` — contrat, limites et validation.

## Tasks & Acceptance

**Execution:**
- [x] `crates/image/src/png.rs` — valider signature, ordre/longueurs des chunks, CRC de chaque chunk, IHDR/PLTE/tRNS/IDAT/IEND, bits réservés et chunks critiques inconnus; ignorer les chunks auxiliaires après validation CRC.
- [x] `crates/image/src/png.rs` — calculer avec arithmétique vérifiée la taille de chaque passe et sa stride; traiter filtres None/Sub/Up/Average/Paeth, échantillons empaquetés 1/2/4 bits et Adam7 sans lire ni écrire hors bornes.
- [x] `crates/image/tests/png.rs` — fabriquer des fixtures couvrant types 0/2/3/4/6, profondeurs permises, transparence, filtres, Adam7, erreurs de structure, CRC et limites.
- [x] `docs/us-040-png.md` — documenter représentation, limites et commandes de validation.

**Acceptance Criteria:**
- Given un PNG valide utilisant chacun des types couleur 0, 2, 3, 4 ou 6 avec profondeur légale, when décodé, then les dimensions et chaque pixel RGBA8 correspondent à l’oracle, y compris les valeurs `tRNS` et les échantillons 16 bits.
- Given des images entrelacées Adam7 ou utilisant l’un des cinq filtres, when décodées, then les pixels reconstitués sont identiques à ceux des équivalents non entrelacés.
- Given un CRC incorrect dans n’importe quel chunk, un chunk critique inconnu, ordre/longueur illégale ou palette/transparence incohérente, when décodé, then l’entrée est rejetée sans panic.
- Given des dimensions, entrée, sortie inflate, ou calcul de stride dépassant une limite ou `usize`, when le décodeur évalue l’entrée, then il retourne une erreur avant l’allocation concernée.
- Given la PngSuite, when les échantillons valides sont comparés à un oracle indépendant et les fichiers invalides sont soumis, then tous les valides correspondent et tous les invalides sont rejetés.

## Implementation Notes

Propriété exclusive : crates/image/src/png.rs, crates/image/tests/png.rs et docs/us-040-png.md, et notes de ce spec. Le coordinateur gère façade/types partagés, manifests/lock, corpus externe, preuves, docs globales, CI, Graphify et Git. Ne pas lancer de skill, sous-agent ou Graphify; ne pas modifier les autres fichiers. Autorisation utilisateur persistante de développer/valider toutes les US en parallèle : les choix techniques ordinaires sont résolus ici.

Référence de format : [W3C PNG Third Edition](https://www.w3.org/TR/png-3/) pour combinaisons de couleur/profondeur, chunks, CRC, filtres et passes Adam7. PngSuite est le corpus de conformité externe (Willem van Schaik; licence freeware à conserver avec les fixtures). Faire comparer l’oracle externe en pixels RGBA8 normalisés, avec cas témoins explicites pour le traitement 16→8 bits; conserver le corpus et le lancement de cet oracle dans la validation coordonnée, hors dépendances/runtime produit. Les chunks IDAT consécutifs forment un seul flux zlib. Chaque passe a une ligne de filtre par scanline et `ceil(pass_width × bits_per_pixel / 8)` octets de données; toutes sommes/produits sont vérifiés avant allocation. Pour Adam7, déposer chaque pixel de passe à sa coordonnée finale. Les données de palette doivent exister et chaque indice utilisé doit être dans ses bornes; `tRNS` respecte type, taille et profondeur. Les chunks auxiliaires non pris en charge sont ignorés après vérification CRC; ne pas appliquer leurs métadonnées de rendu.

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

Le flux zlib doit produire exactement la somme des scanlines prédites par IHDR et les sept passes éventuelles; sa fin doit consommer exactement les octets IDAT concaténés. Ainsi, une sortie trop courte, trop longue, un checksum invalide ou des octets après le flux ne peuvent être confondus avec un PNG valide.

## Verification

**Commands:**
- `cargo test -p atelier-image --locked --offline` — fixtures unitaires réussies.
- `cargo check -p atelier-image --target wasm32-unknown-unknown --locked --offline` — compilation sans API système ni dépendance ajoutée.

**Manual checks:**
- Exécuter le corpus PngSuite complet et comparer les fichiers valides à un décodeur indépendant en RGBA8; confirmer le rejet des cas invalides et archiver les résultats/oracle.

## Validation finale — 2026-10-10

Les quinze contrôles de `images-fonts-validation.json` passent : 210 tests workspace, 34 tests release images/polices, builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM, format, licences, garde, métadonnées et intégrité des ressources. Les sources et le lock sont inchangés pendant les contrôles. `images-fonts-oracle.json` confirme 201 entrées, 187 références régénérées à l’identique, 14 PNG invalides rejetés, 149 728 mutations sans panic, onze inventaires altérés rejetés et vingt faces FontTools. `images-fonts-upstream.json` confirme les vingt polices identiques aux sources officielles. Tous les constats sont triés, aucun différé. Le relevé `github-ci.json` identifie le commit et les jobs distants ; les tests musl sont exécutés sur Linux par la CI.
