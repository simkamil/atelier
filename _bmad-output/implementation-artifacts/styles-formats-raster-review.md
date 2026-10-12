# Revue du lot styles, OPC, JPEG et raster — 2026-10-12

Développement et trois revues indépendantes avec GPT-6 Luna, raisonnement moyen. Les trois agents frais ont reçu le diff complet des sources sans contexte; seule la couche cas limites reçoit ensuite les quatre specs. Aucun reviewer ne modifie le code. Toutes les sorties ont été reçues avant triage.

Baseline : `2b39b2f287265156577ac927978e22d2f3e22981`. Diff source initial : 312808 octets, SHA-256 `87f2bc348065e665180ee38f5478969a4a4b15f2ea9bd855f78cd50cd9a31240`. Les nouveaux fichiers et fixtures binaires sont inclus. Diff complet avec graphe : 12253079 octets, SHA-256 `f3e9b0134387acf4e45c95d75a5168d54a0430193eca491a564c38ebf7b7aecd`. Le graphe généré est contrôlé séparément par les reçus d’intégrité.

Blind : 305,5 Kio, plancher min(floor(sqrt(305,5)+1),10)=10, dix constats. Edge : trois constats. Verification-gap : un constat. La relecture finale OPC a produit V1, réfuté séparément. Les lignes A/R/T sont les contrôles supplémentaires du coordinateur et de l’audit OPC. Chaque constat reçoit son verdict avant regroupement.

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

## Sources primaires

- [ECMA-376, téléchargements officiels Part1/Part2](https://ecma-international.org/publications-and-standards/standards/ecma-376/) : Part2 2021 §6.2.2, §6.5.3.4 et XSD des types; Part1 §17.7.3. Texte extrait seulement pour vérification, aucun code tiers copié.
- [PackUriHelper officiel .NET](https://github.com/dotnet/runtime/blob/main/src/libraries/System.IO.Packaging/src/System/IO/Packaging/PackUriHelper.cs) : ResolvePartUri garde le chemin, comparaison canonique.
- [TargetUri](https://learn.microsoft.com/en-us/dotnet/api/system.io.packaging.packagerelationship.targeturi) : les cibles External relatives sont autorisées.
- [Word, bascules des paragraphes](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oe376/f936abaf-a9cb-439b-923d-7f688d9202e9), [tables](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/14452bbe-be4d-4dbb-90e6-3d23ae9361bc) et [défauts](https://learn.microsoft.com/en-us/openspecs/office_standards/ms-oi29500/f7130225-2368-48f3-acae-a9d278d0fb25).

## État

Correctifs retenus validés par les 41 contrôles de la [passe finale](styles-formats-raster-validation.json), les tests ciblés et la [copie portable](styles-formats-raster-portable.json). Les deux essais interrompus sont conservés séparément (Clippy puis format). R6 est le seul constat nouveau différé, de gravité moyenne non vérifiée; il ne prouve pas une erreur du module livré.
