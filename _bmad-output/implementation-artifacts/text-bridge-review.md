<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Revue US-011 / US-053 / US-091 — 2026-10-11

Trois revues indépendantes et simultanées ont été conduites avec GPT-6 Luna moyen,
sans contexte de développement. Le diff complet depuis
`76efdfee2124112eef5ceee21d67422b062efa80` comprenait les fichiers non suivis.
Chaque constat est conservé ci-dessous avant regroupement. La revue des preuves
ne relève aucune lacune (« No verification gaps found. »).

## Review Triage Log

| ID | Constat | Verdict | Vérification et décision |
|---|---|---|---|
| B1 | GSUB IgnoreMarks devrait ignorer le glyphe courant s'il est une marque. | false | [OpenType, chapitre 2](https://learn.microsoft.com/en-us/typography/opentype/spec/chapter2#lookup-table) précise que le glyphe courant est toujours testé et que les flags filtrent les autres glyphes. Le test ciblé préserve cette règle. Rejeté. |
| B2 | GPOS IgnoreMarks devrait ignorer une marque comme premier glyphe. | false | La même règle normative s'applique à GPOS : le premier glyphe est testé, les marques suivantes sont sautées lors de la recherche du second. Rejeté. |
| B3 | Le kern vertical serait appliqué à x_advance. | false | Dans apply_legacy_kern, toute recherche et addition est à l'intérieur de `if cov & 1 != 0`. Les sous-tables verticales ne produisent aucun ajustement horizontal. Rejeté. |
| B4 | Le kern historique devrait sauter les marques comme IgnoreMarks. | false | IgnoreMarks est un LookupFlag GSUB/GPOS ; le format kern v0 ne porte aucun tel flag. Son application aux paires adjacentes respecte son contrat, tandis que GPOS saute les marques selon ses flags. Rejeté. |
| B5 | ClassDef 2 ne bornerait pas ses fins de plages par numGlyphs. | false | validate_class_def_all refuse explicitement `e >= glyphs`, les plages inversées et les chevauchements. Le format 1 borne également start + count. Rejeté. |
| B6 | Les recherches répétées de Coverage/ClassDef épuisent inutilement le budget des longs runs valides. | medium | Le code facturait et parcourait des tables complètes déjà validées pour chaque glyphe. Patch interne : recherche dichotomique sur les formats ordonnés, accès direct ClassDef 1, compteur par étape réellement exécutée, régression de long run. La validation initiale exhaustive est conservée. |
| B7 | Les vecteurs de segmentation multiplient la mémoire linéaire. | low | Plusieurs vecteurs par scalaire existent, tout en respectant le contrat O(n). Aucun dépassement d'une limite annoncée n'est démontré. Réduire ce facteur constant demanderait de réécrire les états de l'algorithme ; effet négligeable pour l'usage courant. Rejeté selon la règle des optimisations mineures complexes. |
| B8 | Les vingt faces auraient chacune 2 720 cas, en contradiction avec 54 716. | false | Le manifeste donne 2 799 cas pour chacune des quatre faces Carlito et 2 720 pour les seize autres : somme 54 716. Le TSV sans en-tête contient 54 716 lignes et le test exige ce nombre exact. Rejeté. |
| B9 | Le relevé de validation n'empreinte pas les documents finalisés après les tests. | low | Le relevé d'exécution empreinte les sources et fixtures ; les documents sont clos ensuite. Correction directe : un relevé distinct text-bridge-documentation.json empreinte les quatorze documents finalisés et le relevé d'exécution associé, sans prétendre exécuter des tests sur la prose. |
| B10 | La documentation cacherait l'obligation d'ignorer une marque courante. | false | L'obligation alléguée contredit la norme citée pour B1/B2. La documentation décrit les marques traversées par le lookup, conformément au code. Rejeté. |
| E1 | Une marque courante pourrait être substituée avec GSUB IgnoreMarks. | false | Ce résultat est autorisé : IgnoreMarks filtre les glyphes suivants, jamais le glyphe courant. Le garde proposé changerait le comportement normatif. Rejeté indépendamment de B1. |
| E2 | Une marque courante pourrait recevoir GPOS PairPos avec IgnoreMarks. | false | Ce résultat est autorisé par la même règle du glyphe courant ; les marques intermédiaires sont filtrées. Rejeté indépendamment de B2. |
| E3 | Une mutation SharedArrayBuffer entre la validation de l'en-tête et la copie de payload peut changer la trame décodée. | medium | Reproduit par mutation contrôlée pendant la lecture DataView. Patch interne : copie bornée de la trame entière avant toute validation de son contenu, puis lectures exclusives de cette copie. Le test déterministe SharedArrayBuffer et la propriété de copie Buffer sont conservés. |

Les survivants B6, B9 et E3 ont trois causes distinctes et reçoivent chacun
la route `patch` : aucune surface publique ajoutée, aucun changement d'intention.
Les agents d'implémentation d'origine ont été réengagés pour B6 et E3.

## Contrôles complémentaires du coordinateur

- US-011 : le contexte RI de WordBreak est précompté en un passage ; la suite
  de 6 000 indicateurs régionaux ne déclenche plus de recherche arrière quadratique.
  La navigation est vérifiée à chaque octet des 2 710 cas Unicode officiels.
- US-053 : les sous-tables restent des alternatives ordonnées au sein de chaque
  lookup. Les budgets couvrent matrices, composants, doublons de sélection et
  déplacements de vecteur. La validation SingleSubst/Coverage 2 est linéaire,
  avec 30 000 plages et un test de seuil exact. La classe GDEF 4 reste valide.
  Les en-têtes GDEF 1.0/1.2/1.3 et la présence de GlyphClassDef pour IgnoreMarks
  sont contrôlés ; les versions de sous-table kern inconnues sont refusées.
- US-091 : les payloads issus de Buffer sont indépendants, l'oracle refuse
  l'hexadécimal non ASCII, les dix golden frames sont fixes et les sorties
  réellement encodées par Rust sont décodées par JavaScript. Les deux codecs
  vérifient la capacité maximale exacte et son dépassement.
- Outillage : les avertissements Clippy du nouveau module sont corrigés ;
  le répertoire du pilote de mutations est créé sur un checkout neuf.

La validation finale a exécuté les 25 contrôles avec succès sur des sources et
un lock inchangés : 260 tests workspace et 70 tests release, dont les corpus
indépendants, puis 10 000 mutations sans panique et les contrôles de qualité.
Les résultats détaillés sont dans text-bridge-validation.json ; les mutations
et les empreintes documentaires ont leurs relevés distincts.

Aucun constat différé.
