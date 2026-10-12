---
title: 'US-300 — Paquet OPC'
type: feature
created: '2026-10-12'
status: done
baseline_commit: 2b39b2f287265156577ac927978e22d2f3e22981
route: dispatch
review_loop_iteration: 0
context: [SPEC.md, US-V0-V1.md]
---

<frozen-after-approval reason="Lot de quatre US explicitement autorisé par l'utilisateur">
## Intent
Lire et écrire la structure OPC utilisée par DOCX/DOTX/DOCM : parties, Content Types, relations, URI relatives et propriétés core/app. Préserver les parties inconnues, leur type et leurs relations.

## Boundaries & Constraints
Rust local sans dépendances externes et sans unsafe. Réutiliser atelier-zip et atelier-xml. Périmètre fmt-ooxml uniquement, plus tests et documentation dédiés. Pas d'import sémantique document.xml, ni styles Word (US-301). Aucun accès réseau pour les relations externes. Préserver les octets inconnus et produire un paquet déterministe en mode déterministe.

## I/O & Edge-Case Matrix
| Scénario | Entrée | Résultat |
|---|---|---|
| Paquet valide | ZIP avec overrides/defaults et relations relatives | Parties typées et cibles correctement résolues |
| Extension inconnue | Partie binaire et XML inconnu | Octets, types et relations conservés à l'aller-retour |
| Entrée invalide | URI, doublon, cible interne absente ou relation corrompue | Diagnostic typé et borné |
| Relation externe | URL absolue | Conservation sans chargement |
| Limites | ZIP/XML excessif ou tronqué | Refus propre sans allocation démesurée |
</frozen-after-approval>

## Code Map
- `crates/fmt-ooxml/src/lib.rs` : scaffold et futur point d'entrée OPC.
- `crates/zip/src/lib.rs` : lecture/écriture ZIP, limites et chemins.
- `crates/xml/src/lib.rs` : événements avec espaces de noms et écrivain.
- `US-V0-V1.md:1034` : critères et dépendances US-022/023/030 réalisés.

## Tasks & Acceptance
**Execution:**
- [x] `crates/fmt-ooxml/src/` : API paquet, parties, content types, URI, relations et propriétés.
- [x] `crates/fmt-ooxml/tests/` : mini-paquets et adversaires, aller-retour inconnu et déterminisme.
- [x] `docs/us-300-opc-package.md` : exemples d'API, limites et validation.

**Acceptance Criteria:**
- Given un paquet avec parties inconnues, when lecture puis écriture puis lecture, then données, types et relations sont conservés.
- Given des relations à la racine et depuis une partie, when résolution, then les cibles internes relatives suivent OPC et les externes ne sont jamais chargées.
- Given propriétés core/app, when lecture et écriture, then les champs exposés gardent leurs valeurs et les données inconnues sont préservées selon le contrat documenté.

## Implementation Notes

Coordination : espace de travail partagé. Ne modifier que les fichiers de cette US et sa documentation dédiée. Le coordinateur maintient Cargo.lock, la CI, Graphify et le suivi global. Ne pas committer, pousser ou lancer cargo fmt --all (formater seulement ses propres fichiers). Avant Cargo, exécuter le garde de README ; commandes --locked --offline. Le parent prépare les dépendances locales ZIP/XML pour OPC et geom/image pour raster avant les tests. Signaler les fichiers prêts et conserver les preuves de commandes ciblées.

## Spec Change Log

## Review Triage Log

Triage complet (chaque constat, verdict, preuve et route) : [revue du lot](styles-formats-raster-review.md). Les lignes du tableau ci-dessous sont reproduites pour cette spec.

| ID | Verdict | Route | Preuve et décision |
|---|---|---|---|
| B1 | medium | patch | OPC write sérialise une source/cible absente alors que read la refuse; contrôle final après matérialisation, chargement dans tout ordre conservé. |
| B2 | medium | patch | PartUri accepte a%62.xml et ab.xml séparément. ECMA-376-2 §6.2.2.2 interdit unreserved encodés; rejet typé, sans décodeur produit externe. |
| B3 | medium | patch | resolve_target rejette # et ? avant résolution. Une référence relative avec fragment doit conserver le Target et résoudre le chemin de partie (ECMA §6.5.3.4, PackUriHelper.ResolvePartUri). |
| B4 | low | patch | validate_external_uri accepte 1:x et +:x : premier caractère non contrôlé. Correction directe du schéma avec séparation des références relatives autorisées en External. |
| B5 | medium | patch | valid_content_type accepte /type et a/b/c, refuse des paramètres MIME valides avec espaces; validation syntaxique RFC7231 nécessaire pour produire un paquet interopérable. |
| B6 | medium | patch | Default.Extension accepte chemins et whitespace. Contrôle ST_Extension du XSD officiel et cas adversaires nécessaires. |
| B7 | low | patch | SOF accepte 2 composants, puis convert_color ne les supporte jamais. Refus précoce avant allocations : simple correction du test de compte. |
| B8 | medium | patch | APP14 limite déjà transform à 0..2, mais ne contrôle pas 3+2 et 4+1; ces combinaisons sont rendues avec le mauvais espace. Vérification de compatibilité avant scans/rendu. |
| B9 | false | reject | Node::Section.children (node.rs) ajoute explicitement headers et footers; walk traverse children récursivement. Le cas allégué ne manque pas au parcours. |
| B10 | false | reject | ECMA-376 §17.7.3 résout chaque niveau à la première valeur trouvée depuis le style appliqué vers basedOn. chain parent→enfant + dernière valeur respecte cette règle; les membres de chaîne ne sont pas des niveaux supplémentaires. |
| E1 | medium | patch | read_scan efface un eob_run restant avant RST. Malformation atteignable; garde avant reset et fixture minimal négatif, restart progressif valide ajouté. |
| E2 | high | patch | EXIF IFD=u32::MAX atteint u16at(ifd): at+2 déborde sur usize 32 bits avant get; même risque at+4/end+4. checked_add puis get et offset maximal sans panique. |
| E3 | medium | patch | Même sortie invalide que B1, vérifiée séparément : aucune validation finale de références dans write au snapshot initial. Regroupé avec B1 pour correction, ligne propre conservée. |
| G1 | medium | patch | Revue des tests pré-vérifiée : seulement title/application/company assertés, autres champs exposés absents. Ajouter égalité de toutes les propriétés sur aller-retour et mutation. |
| A1 | medium | patch | Audit OPC supplémentaire : segments vides word//document.xml aliasaient un nom valide. Rejet segments vides/authority/trailing slash et tests appliqués avant snapshot. |
| A2 | medium | patch | Contrôle coordinateur : _rels/document.xml.rels de partie racine mal reconnu. Chemin source racine et test round-trip corrigés avant snapshot. |
| A3 | medium | patch | Contrôle coordinateur : profondeur XML Default/Relationship et ID whitespace non vérifiés, limites insuffisamment couvertes; gardes et tests appliqués avant snapshot. |
| A4 | medium | patch | Paquet neuf : core_properties_mut/app_properties_mut étaient silencieusement perdus sans partie/relation. Matérialisation et erreur collision, deux tests appliqués avant snapshot. |
| R1 | medium | patch | Style lié initialement superposé automatiquement au paragraphe : paire indépendante selon source MS LinkedStyle. Sélection explicite par appelant et test divergent appliqués avant snapshot. |
| R2 | medium | patch | Style de tableau par défaut initialement appliqué au corps hors tableau. Repli limité au contexte table et test corps/cellule appliqués avant snapshot. |
| R3 | false | reject | Clé cache contient les trois IDs de style; le résultat caché ne dépend que du catalogue/thème. Propriétés directes après hit; test deux documents aux mêmes NodeIds et alignements différents confirme absence de fuite. |
| R4 | medium | patch | Source MS-OI29500 §2.1.246b : Word réinitialise les bascules table à la valeur effective, alors que le code utilisait le mode caractère. Séparer table/paragraphe des caractères. |
| R5 | medium | patch | Source MS-OI29500 §2.1.230a : défaut vrai ne garantit pas état vrai; niveau caractère false doit basculer. defaults_true ignorait ce cas; corriger et couvrir les combinaisons. |
| R6 | maybe-false | defer | Table imbriquée sans style : le style explicite ancêtre est repris. La divergence Word n’est pas établie par les sources consultées; une fixture et un rendu Word comparé trancheraient. Gravité medium non vérifiée, comportement API à documenter. |
| R7 | low | patch | Provenance JPEG devenue inexacte après ajout bruit/q20/q98/tailles différentes; README corrigé avant snapshot. |
| T1 | high | patch | Raster en développement : masque vide/horizontal pouvait soustraire y1-y0 non ordonné. Retour masque zéro avant estimation, régression dégénérée passe. |
| T2 | medium | patch | Raster en développement : formule intersection triangle perdait un échantillon par cancellation. Formule barycentrique stable vérifiée avec oracle point-triangle indépendant. |
| T3 | medium | patch | Raster en développement : interpoler couleur à chaque sous-échantillon brouillait identité. Interpolation au centre et couverture séparée; identité/bilinear alpha/rotation vérifiés. |
| R8 | medium | patch | PartUri/maps : comparaison sensible à la casse permettait /A et /a, ainsi que /a et /a/b. ECMA-376-2 §6.2.2.3 impose équivalence ASCII et interdit noms dérivables; Eq/Ord/Hash cohérents, collisions et tests ajoutés. |
| R9 | medium | patch | Régression détectée après R8 : réserver .rels dans PartUri rejetait un Override de content type /_rels/.rels valide. Parse privé autorisant métadonnées uniquement dans Override et test réel ajoutés. |
| R10 | medium | patch | Reconnaissance relationnelle trop large (_rels/foo.bin) et sensible à la casse (_RELS/DOCUMENT.XML.RELS). Pattern exact des deux derniers segments, comparaison ASCII et tests ajoutés. |
| R11 | medium | patch | PartUri acceptait des caractères hors segment IRI tels que < > et [. Validation isegment-nz ajoutée et noms Unicode valides conservés; test invalides/Unicode. |
| R12 | medium | patch | %61%ff sautait le rejet unreserved car le groupe percent échouait UTF8. Contrôle de chaque code ASCII avant contrôle des scalaires, test contournement. |
| R13 | low | patch | Dernier patch OPC non formaté : rustfmt ciblé sur les deux fichiers, tests 13/13 après format. Contrôle global ensuite. |
| V1 | false | reject | Relecture indépendante finale allègue collision a/ab. part_names_conflict compare des segments complets séparés par /; [a] versus [ab] compare false, donc aucun rejet. Le prefixe a et a/b est correctement refusé par ECMA §6.2.2.3. |
| R14 | medium | patch | Le type MIME des relations .rels était comparé avec casse : un type APPLICATION/VND.OPENXMLFORMATS-PACKAGE.RELATIONSHIPS+XML valide était refusé. Comparaison ASCII insensible à la casse et test dédié ; les URI Type des relations gardent leur sémantique propre. |
| L1 | low | patch | Première validation globale arrêtée par Clippy sur quatre fixtures OPC field_reassign_with_default. Initialisations par mise à jour de structure, Clippy ciblé réussi puis nouvelle passe globale ; échec initial conservé. |
| L2 | low | patch | Deux expressions du test EXIF maximal dans jpeg.rs ne respectaient pas rustfmt. Formatage ciblé puis cargo fmt --all --check réussi ; seconde passe conservée dans styles-formats-raster-format-attempt.json et validation complète relancée. |

## Verification

- [Validation finale](styles-formats-raster-validation.json) : 41 contrôles réussis, 507 tests workspace, 55 tests natifs release et 57 assertions de l’éditeur réel ; sources et lock inchangés.
- Clippy Windows/WASM, format, licence, builds Windows/WASM/musl et compilation des tests musl réussis ; exécution musl déléguée à la CI Linux.
- [Exécutable portable isolé](styles-formats-raster-portable.json) : 18 ressources, vrai Worker, saisie/pixels/undo/redo, objets callback 0→0.
- [Graphe final](styles-formats-raster-graph.json) : inventaire, dépendances Cargo, statuts et empreintes vérifiés séparément.
- Première passe interrompue par Clippy : [preuve](styles-formats-raster-exploration.json). Deuxième passe interrompue par format : [preuve](styles-formats-raster-format-attempt.json). Passe finale complète sur les sources corrigées.
- R6 : seule comparaison Word nouvelle différée, divergence non établie ; voir [travaux différés](deferred-work.md). US-111 conserve son échec strict de handles et les essais physiques restent ouverts.
