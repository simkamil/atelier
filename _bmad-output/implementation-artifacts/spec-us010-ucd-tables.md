---
title: 'Tables Unicode générées (US-010)'
type: 'feature'
created: '2026-10-10'
status: 'done'
baseline_commit: '20a6a05bd69d22f4f004b7a907d2c9696b24c4ad'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.2-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Segmentation, coupures et casse ne disposent pas des données Unicode officielles. **Approach:** Figer UCD 17.0.0 et générer des tables Rust compactes et reproductibles avec un outil local sans dépendance externe.

## Boundaries & Constraints

**Always:** Sources originales sous `data/ucd/17.0.0/`, licence Unicode complète, manifeste des SHA-256 et URL. L’archive officielle déjà téléchargée est `target/prototype-oracle/UCD-17.0.0.zip`, SHA-256 `2066d1909b2ea93916ce092da1c0ee4808ea3ef8407c94b4f14f5b7eb263d28e`. Préserver ses octets de données. Outil `atelier-ucd-gen` Rust sûr, CLI `--ucd DIR --output FILE [--check]`; aucune entrée réseau à la génération/compilation. Un build script génère dans OUT_DIR et exige l’égalité avec `crates/unicode/src/generated.rs` versionné; pas d’écriture dans les sources durant un build. Dépendance de build locale au générateur autorisée. Tables à deux niveaux, pages 256 dédupliquées, index couvrant U+0000..10FFFF. Parser strict, plages/comptes vérifiés et erreur explicite sans panic. Limites d’entrée et sortie nommées.

Données requises : catégorie générale, CCC, Line_Break, Grapheme_Cluster_Break, Word_Break, Indic_Conjunct_Break, propriétés emoji (dont Extended_Pictographic), Cased/Case_Ignorable/White_Space, mappings simples et complets de casse/fold, règles conditionnelles SpecialCasing conservées, décompositions avec tags et exclusions/compositions canoniques, noms (y compris Hangul et noms algorithmiques). Fournir API typée ou chaînes statiques pour propriétés, séquences pour mappings, accès aux règles conditionnelles. Respecter les valeurs @missing et First/Last; une propriété absente ne vaut pas implicitement zéro. Les algorithmes UAX et normalisation/casse contextuelle relèvent de US-011/012/013.

**Never:** bibliothèque Rust tierce, unsafe, dépendance aux fichiers installés sur l’hôte, téléchargement au build, remplacement des encodages US-014, version latest, résultats dérivés d’un Unicode Python de version différente.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Données officielles | UCD 17.0.0 complet attendu | mêmes octets Rust à chaque génération; build offline | Succès |
| Données incorrectes | absent, plage inversée/hors Unicode, mapping invalide, version différente | aucune sortie partielle publiée | erreur CLI non nulle |
| Cas particuliers | First/Last, @missing, casse multiple/contextuelle, nom Hangul/CJK, emoji/ZWJ | propriétés et données officielles exactes | Succès |
| Vérification | sortie versionnée altérée | --check/build refusent la dérive | erreur explicite |

</frozen-after-approval>

## Code Map

- `tools/ucd-gen/{Cargo.toml,src/lib.rs,src/main.rs,tests/}` — nouvel outil std uniquement, réutilisable par build.rs.
- `crates/unicode/src/lib.rs` et `src/tables.rs` — encodages existants à préserver; export des nouvelles propriétés coordonné.
- `crates/unicode/{build.rs,src/properties.rs,src/generated.rs,tests/ucd.rs}` — génération et consultation sans disque à l’exécution.
- `data/ucd/` — sources, licence, manifeste et README; corpus officiels pour prochaines US.

## Tasks & Acceptance

**Execution:**
- [x] `data/ucd/**` — extraire les sources nécessaires avec provenance et licence complète; conserver les corpus officiels.
- [x] `tools/ucd-gen/**` — parser et générer de façon déterministe; CLI --check et erreurs testées.
- [x] `crates/unicode/build.rs`, `src/generated.rs`, `src/properties.rs`, `tests/ucd.rs` — intégrer tables et API; vérifier les cas particuliers et absence de dérive au build.
- [x] `docs/us-010-ucd-tables.md` — API, régénération, version, limites et séparation données/algorithmes.

**Acceptance Criteria:**
- Given UCD figé, when génération répétée et build offline, then sortie identique à celle versionnée et données utilisables sur Windows/WASM/musl.
- Given chaque scalaire Unicode, when les propriétés sont comparées indépendamment aux sources officielles, then les valeurs correspondent, y compris les valeurs par défaut/ranges.
- Given casse, décomposition et noms particuliers, when les données sont consultées, then les séquences et conditions sont conservées sans appliquer prématurément les algorithmes futurs.

## Verification

- `cargo test -p atelier-ucd-gen -p atelier-unicode --locked --offline` — génération, consultations et entrées invalides passent.
- `cargo run -p atelier-ucd-gen --locked --offline -- --ucd data/ucd/17.0.0 --output crates/unicode/src/generated.rs --check` — sortie identique.

## Implementation Notes

Autorisation persistante de développer, valider et publier les US en parallèle avec GPT-6 Luna moyen. Le coordinateur gère manifests partagés, lock, exports partagés, CI, preuves, Graphify et Git. L’agent ne lance ni skill, ni Graphify, ni sous-agent. UTF-8 explicite; aucune modification hors propriété indiquée.

## Spec Change Log

## Review Triage Log

Propriété exclusive : tools/ucd-gen/**, data/ucd/**, crates/unicode/build.rs, crates/unicode/src/properties.rs, crates/unicode/src/generated.rs, crates/unicode/tests/ucd.rs et doc US-010. Le parent raccorde crates/unicode/src/lib.rs, manifests et lock.


| ID | Constat | Verdict et preuve | Disposition |
|---|---|---|---|
| B1 | rename ne remplace pas un fichier sur Windows | false : deux générations CLI successives vers le même fichier existant ont réussi sur Windows ; Rust std prend en charge son remplacement. | Rejeté, aucun contournement destructif. |
| B2 | Commentaire is_emoji confondu avec Extended_Pictographic | low : la fonction lit bien Emoji, mais son commentaire nomme une autre propriété. | Correction directe du commentaire. |
| B3 | Valeurs de propriétés invalides acceptées | medium : push_property accepte une chaîne arbitraire dans LB/GCB/WB/InCB ; une édition invalide peut être publiée. | Validation contre PropertyValueAliases figé et tests négatifs. |
| B4 | 256 valeurs refusées par un index u8 | low : >255 exclut la frontière256 représentable, bien que les propriétés17 réelles soient plus petites. | Correction directe de la borne. |
| B5 | Comparaison Unicode exhaustive hors CI | medium : tests natifs représentatifs et reproductibilité peuvent accepter une erreur cohérente générateur/sortie. | Empreintes indépendantes de tous les scalaires vérifiées en test natif. |
| B6 | Mappings de casse partiellement couverts en test natif | medium : les dix consultations ne sont pas toutes comparées aux références dans les tests existants. | Même test exhaustif, empreinte séparée par consultation. |
| B7 | Séquences de casse conditionnelle peu vérifiées en CI | medium : seule la présence d’une condition ne détecte pas une permutation des séquences. | Empreinte de toutes les colonnes des seize règles officielles. |
| E1 | Ligne de propriété malformée ignorée | medium : le continue de property_file ignore aussi des lignes non vides à un champ. | Rejet explicite des lignes noncommentaires invalides et test. |
| V1 | Oracle exhaustif absent de la CI | medium : constat de vérification pré-vérifié par les recherches du reviewer. | Couvert par le test natif d’empreintes indépendantes, sans Python en CI. |

Validation finale : 21 contrôles, 233 tests workspace et 43 tests release. Sources et lock inchangés durant les contrôles. Voir [relevé](prototype-foundations-validation.json) et [revue complète](prototype-foundations-review.md). Aucun constat différé.
