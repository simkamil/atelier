---
title: 'Crénage et ligatures (US-053)'
type: 'feature'
created: '2026-10-11'
status: 'done'
baseline_commit: '76efdfee2124112eef5ceee21d67422b062efa80'
route: 'dispatch'
review_loop_iteration: 0
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-0.6-context.md'
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** Les glyphes et contours existent, mais l'espacement des paires et les ligatures latines manquent. **Approach:** Ajouter une façade de layout OpenType sûre et réutilisable, pour appliquer GSUB liga et GPOS kern ou kern historique, avant US-054.

## Boundaries & Constraints

**Always:** Rust sûr sans dépendance externe. `FontFace::layout() -> Result<FontLayout<'_>, LayoutError>` et variante `layout_with_limits(LayoutLimits)`. Façade empruntant les tables, validée une fois. API `apply_ligatures(&[GlyphCluster]) -> Result<Vec<GlyphCluster>, LayoutError>` et `pair_adjustments(&[GlyphCluster]) -> Result<Vec<GlyphAdjustment>, LayoutError>`. `GlyphCluster { glyph_id:u16, cluster:Range<usize> }` emploie des indices de scalaires dans le texte d'origine, demi-ouverts; plages non vides et glyph IDs < numGlyphs. Substitution simple conserve la plage; ligature fusionne les plages des composants (min start/max end), les marques sautées restent présentes avec leur plage. Les quatre deltas signés x/y placement et advance dans GlyphAdjustment sont en i32 unités de police, cumul checked; autant de résultats que de glyphes en entrée pour pair_adjustments.

Sélection par table : script latn, DefaultLangSys; DFLT seulement si latn absent, pas si son DefaultLangSys est nul ou vide. Ne retenir que GSUB liga et GPOS kern, y compris required feature s'il porte ce tag. Dédupliquer puis appliquer dans l'ordre des indices LookupList. Première sous-table et premier enregistrement de ligature qui correspondent, selon la priorité encodée dans la police, sans tri par longueur.

Support : Coverage formats 1/2, ClassDef 1/2, GSUB types 1 formats 1/2 et 4 format 1; extension GSUB7 vers 1/4; GPOS2 PairPos formats 1/2 et extension9 vers 2. Pas d'extension récursive. ValueRecords avec les quatre valeurs de base; champs Device/VariationIndex actifs non nuls -> Unsupported (des champs présents mais offsets nuls sont autorisés). Honorer flags 0 et IgnoreMarks (GDEF GlyphClassDef classe 3), sinon Unsupported pour lookup sélectionné. Les marques ignorées n'interrompent ni ligature ni paire; respecter le déplacement du curseur PairPos défini par valueFormat2. Une présence de feature GPOS kern applicable choisit GPOS pour tout le run, même si aucune paire n'a de valeur; sinon kern version0 format0 horizontal. Ne jamais sommer les deux sources. Sous-tables kern horizontales ordinaires sont additives, coverage override remplace; minimum/cross-stream et autres formats actifs -> Unsupported explicite.

Valider les offsets relatifs, compteurs, ordre/duplicates de Coverage/ClassDef/paires, indices de feature/lookup/classe, glyphes substitués, dimensions de matrices, en-têtes/réservés. Aucune allocation contrôlée uniquement par un compteur hostile. Limites publiques par défaut : 1_000_000 unités de validation, 10_000_000 unités de travail par opération, 65_536 glyphes d'entrée, 1024 lookups sélectionnés; cumulées pour toute la façade/opération, compteurs checked. Erreurs distinctes de troncature/données/format/limite/glyphe/plage, jamais de résultat partiel ni panique. Une table absente est une opération identité/zéro. Une table sélectionnée malformée produit une erreur, pas un fallback silencieux.

**Never:** façonnage de texte/cmap dans cette US, scripts complexes, bidi, mark positioning, variations/hinting, modification des fontes ou du parseur sfnt existant hors export coordonné.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| Carlito/Caladea | latn liga/kern, extensions, classes | Ligatures et deltas de référence | Succès |
| Liberation | GPOS + kern historique coexistants | GPOS seul; Mono identité/zéro | Succès |
| Marques | IgnoreMarks et GDEF classe3 | Marques préservées, clusters composants fusionnés | Succès |
| Tables hostiles | Offset, compte, classe, cycle extension, débordement | Aucun résultat partiel/panique | LayoutError |
| Budget | seuil exact puis un au-delà | Accepté puis refusé | LimitExceeded |

</frozen-after-approval>

## Code Map

- `crates/font/src/lib.rs` — FontFace::table/metrics, export par coordinateur.
- `data/fonts/` — 20 TTF inchangés (GPOS9 Carlito; PairPos1/2 Caladea; double source Liberation).
- Normes : https://learn.microsoft.com/en-us/typography/opentype/spec/{gpos,gsub,chapter2,kern}.

## Tasks & Acceptance

**Execution:**
- [x] `crates/font/src/layout.rs` — parsing validé, sélection, substitution et positionnement bornés.
- [x] `crates/font/tests/layout.rs` — fixtures synthétiques de chaque format, flags, priorités, curseur valueFormat2, sources et erreurs/budgets; 20 fontes réelles.
- [x] `docs/us-053-kerning-ligatures.md` — API, unités, clusters, priorités et limites.

**Acceptance Criteria:**
- Références indépendantes des 20 faces concordantes; paires connues AV/To et ligatures fi/fl/ffi/ffl testées selon features réelles.
- Les formats GSUB1 non exercés par liga dans les fontes embarquées sont testés synthétiquement.
- Le parseur traite données invalides et travail amplifié sans panique ni dépassement de limites.

## Implementation Notes

Autorisation persistante : développement parallèle GPT-6 Luna moyen, validation et push. Propriété exclusive de l'agent : les trois fichiers des tâches et notes append-only de cette spec. Le coordinateur gère lib.rs, manifests, CI, preuves et oracle FontTools, Graphify et Git. Ne lancer ni skill, ni Graphify, ni sous-agent, ne modifier aucun autre fichier. UTF-8 explicite. Cargo ciblé autorisé; rustfmt --edition 2024 sur fichiers possédés uniquement, format workspace final par coordinateur. Le coordinateur fournira un oracle indépendant; ne pas créer des références à partir des sorties du produit.

## Spec Change Log

## Review Triage Log

| ID | Constat | Verdict | Vérification et décision |
|---|---|---|---|
| B1 | GSUB IgnoreMarks devrait ignorer le glyphe courant s'il est une marque. | false | [OpenType, chapitre 2](https://learn.microsoft.com/en-us/typography/opentype/spec/chapter2#lookup-table) précise que le glyphe courant est toujours testé et que les flags filtrent les autres glyphes. Le test ciblé préserve cette règle. Rejeté. |
| B2 | GPOS IgnoreMarks devrait ignorer une marque comme premier glyphe. | false | La même règle normative s'applique à GPOS : le premier glyphe est testé, les marques suivantes sont sautées lors de la recherche du second. Rejeté. |
| B3 | Le kern vertical serait appliqué à x_advance. | false | Dans apply_legacy_kern, toute recherche et addition est à l'intérieur de `if cov & 1 != 0`. Les sous-tables verticales ne produisent aucun ajustement horizontal. Rejeté. |
| B4 | Le kern historique devrait sauter les marques comme IgnoreMarks. | false | IgnoreMarks est un LookupFlag GSUB/GPOS ; le format kern v0 ne porte aucun tel flag. Son application aux paires adjacentes respecte son contrat, tandis que GPOS saute les marques selon ses flags. Rejeté. |
| B5 | ClassDef 2 ne bornerait pas ses fins de plages par numGlyphs. | false | validate_class_def_all refuse explicitement `e >= glyphs`, les plages inversées et les chevauchements. Le format 1 borne également start + count. Rejeté. |
| B6 | Les recherches répétées de Coverage/ClassDef épuisent inutilement le budget des longs runs valides. | medium | Le code facturait et parcourait des tables complètes déjà validées pour chaque glyphe. Patch interne : recherche dichotomique sur les formats ordonnés, accès direct ClassDef 1, compteur par étape réellement exécutée, régression de long run. La validation initiale exhaustive est conservée. |
| B8 | Les vingt faces auraient chacune 2 720 cas, en contradiction avec 54 716. | false | Le manifeste donne 2 799 cas pour chacune des quatre faces Carlito et 2 720 pour les seize autres : somme 54 716. Le TSV sans en-tête contient 54 716 lignes et le test exige ce nombre exact. Rejeté. |
| B9 | Le relevé de validation n'empreinte pas les documents finalisés après les tests. | low | Le relevé d'exécution empreinte les sources et fixtures ; les documents sont clos ensuite. Correction directe : un relevé distinct text-bridge-documentation.json empreinte les quatorze documents finalisés et le relevé d'exécution associé, sans prétendre exécuter des tests sur la prose. |
| B10 | La documentation cacherait l'obligation d'ignorer une marque courante. | false | L'obligation alléguée contredit la norme citée pour B1/B2. La documentation décrit les marques traversées par le lookup, conformément au code. Rejeté. |
| E1 | Une marque courante pourrait être substituée avec GSUB IgnoreMarks. | false | Ce résultat est autorisé : IgnoreMarks filtre les glyphes suivants, jamais le glyphe courant. Le garde proposé changerait le comportement normatif. Rejeté indépendamment de B1. |
| E2 | Une marque courante pourrait recevoir GPOS PairPos avec IgnoreMarks. | false | Ce résultat est autorisé par la même règle du glyphe courant ; les marques intermédiaires sont filtrées. Rejeté indépendamment de B2. |

Chaque constat est classé avant regroupement ; les patches internes sont validés par les contrôles finaux. La [revue commune](text-bridge-review.md) conserve les treize constats et les contrôles supplémentaires du coordinateur.

## Verification

- `cargo test -p atelier-font --locked --offline` — tests existants et layout passent.
- `cargo check -p atelier-font --target wasm32-unknown-unknown --locked --offline` — API portable.

Validation finale : 25 contrôles, 260 tests workspace, 70 tests release. Les sources et le lock sont restés identiques pendant les contrôles. Voir [relevé](text-bridge-validation.json) et [revue triée](text-bridge-review.md). Aucun constat différé.
