# User stories — V0 (Fondations) et V1 (MVP traitement de texte)

| | |
|---|---|
| Référence | [SPEC.md](SPEC.md) v0.2 |
| Date | 2026-10-12 |
| Statut | Développement en cours — critères cochés après validation, aligné sur SPEC v0.2 |

## Conventions

- **ID** : les stories regroupées sous V0 (US-001 à US-120) constituent les fondations ; celles regroupées sous V1 (US-200 à US-364) constituent le MVP. Le regroupement par jalon fait foi.
- **Format** : *En tant que… je veux… afin de…*, puis les critères d'acceptation (cases à cocher).
- **Taille** (développeur seul) : **S** ≤ 2 jours · **M** ≤ 1 semaine · **L** ≤ 3 semaines. Une story qui dépasse L est découpée.
- Les stories V0 sont des stories techniques (« en tant que développeur ») : elles n'apportent pas de valeur visible seules, mais chaque épique se termine par un résultat vérifiable.
- **Réf** : exigence (`F-…`), décision (`D…`) ou section (`§…`) de la spec.
- **Dossier de données** : les chemins `atelier-data/…` désignent la racine portable par défaut ; le mode local choisi explicitement utilise la racine définie au §7.2 de la spec.

## Définition de « terminé » (toutes les stories)

1. Critères d'acceptation vérifiés.
2. Tests unitaires écrits et passants ; analyseurs ajoutés au fuzzer.
3. `cargo clippy -D warnings` et `cargo fmt --check` passent ; aucune dépendance externe ajoutée.
4. Aucun `unsafe` hors des crates FFI ; chaque bloc `unsafe` commenté.
5. Aucune panique possible sur une entrée externe (fichier, message, saisie).
6. Chaînes visibles dans le catalogue i18n ; éléments d'interface accessibles au clavier et étiquetés (ARIA).
7. Pas de régression sur les bancs de performance ni sur les images de référence.

---

# V0 — Fondations

## E0.1 — Socle du dépôt et outillage

#### US-001 — Initialiser le workspace
*En tant que* développeur, *je veux* un workspace Cargo structuré selon la spec *afin de* démarrer sur une base saine.
- [x] Les crates du §6.2 existent (vides), compilent, avec `#![forbid(unsafe_code)]` sauf `core-wasm`, `desktop-win`, `server`.
- [x] Configuration `rustfmt` et `clippy` partagée.
- [x] En-tête de licence AGPL dans chaque fichier source, vérifié par un outil maison (`tools/`).
- [x] `LICENSE` (AGPL-3.0) et `README` minimal.

Réf : D11, §6.2, §9 · Taille : S · Dépend de : —

Validation US-001 (2026-10-09) : [preuves et limites](docs/DEVELOPMENT.md#preuve-us-001--2026-10-09).

#### US-002 — Intégration continue
*En tant que* développeur, *je veux* une CI qui compile et teste les trois cibles *afin de* détecter les régressions immédiatement.
- [x] Build `x86_64-pc-windows-msvc`, `wasm32-unknown-unknown`, `x86_64-unknown-linux-musl`.
- [x] Exécution des tests, clippy, rustfmt, vérification des en-têtes de licence.
- [x] Échec si `Cargo.lock` contient un paquet extérieur au workspace.

Réf : §8 · Taille : S · Dépend de : US-001

Validation locale US-002 (2026-10-10) : [preuves et limites](docs/DEVELOPMENT.md#preuve-us-002--2026-10-10). Workflow fourni et commandes vérifiées ; exécution GitHub et tests sous Linux à confirmer sur ces hôtes.

#### US-003 — Tests par images de référence
*En tant que* développeur, *je veux* comparer des rendus à des images de référence *afin de* détecter toute régression visuelle.
- [x] Comparaison PNG pixel à pixel avec tolérance paramétrable (écart par canal et pourcentage de pixels).
- [x] En cas d'échec : image de différence générée à côté du test.
- [x] Mode « mise à jour » explicite pour régénérer les références.

Validation US-003 (2026-10-13) : développée, relue par un agent indépendant et corrigée ; [contrat](docs/us-003-image-reference-tests.md).

Réf : §8 · Taille : M · Dépend de : US-041, US-082

#### US-004 — Fuzzer maison
*En tant que* développeur, *je veux* un fuzzer mutationnel *afin de* garantir que les analyseurs ne paniquent jamais.
- [x] Interface `FuzzTarget` ; mutations : inversion de bits, insertion, suppression, épissure entre entrées du corpus, dictionnaire de jetons.
- [ ] Corpus initial par cible, sauvegarde des entrées produisant une panique, un dépassement de temps ou de mémoire.
- [x] Minimisation simple d'une entrée fautive.
- [x] Lancement d'une cible pendant une durée donnée en ligne de commande.

Réf : §6.13 · Taille : M · Dépend de : US-001

Validation US-004 (2026-10-12) : partielle (mémoire réelle non mesurée) ; [contrat](docs/us-004-fuzzer.md), [preuves du lot](_bmad-output/implementation-artifacts/foundation-integration-validation.json).

#### US-005 — Bancs de performance
*En tant que* développeur, *je veux* des mesures de performance reproductibles *afin de* suivre les objectifs du §6.16.
- [x] Bancs exécutables en ligne de commande, médiane sur N itérations, sortie CSV.
- [x] Comparaison avec une exécution de référence, signalement d'une régression > 10 %.

Réf : §6.16 · Taille : S · Dépend de : US-001

Validation US-005 (2026-10-09) : [preuves et limites](docs/DEVELOPMENT.md#preuve-us-005--2026-10-09).

## E0.2 — Unicode et encodages

#### US-010 — Générateur de tables Unicode
*En tant que* développeur, *je veux* générer les tables Unicode depuis les fichiers UCD officiels *afin de* ne dépendre d'aucune bibliothèque.
- [x] Les fichiers UCD (version figée) sont stockés dans `data/ucd/`.
- [x] L'outil génère du code Rust compact (tables à deux niveaux) : catégorie générale, classes de coupure de ligne, propriétés de graphème et de mot, casse, décompositions, propriétés emoji, noms des caractères.
- [x] Génération reproductible (même entrée → même sortie), fichiers générés versionnés.

Réf : §6.4 · Taille : M · Dépend de : US-001

#### US-011 — Segmentation en graphèmes et en mots
*En tant que* développeur, *je veux* segmenter le texte selon UAX#29 *afin que* le curseur, la suppression et la sélection par mot soient corrects.
- [x] 100 % de `GraphemeBreakTest.txt` et `WordBreakTest.txt` passent.
- [x] Itérateurs avant et arrière à partir de n'importe quelle position.

Réf : §6.4, §6.15 · Taille : M · Dépend de : US-010

#### US-012 — Opportunités de coupure de ligne
*En tant que* développeur, *je veux* les opportunités de coupure UAX#14 *afin de* couper les lignes correctement.
- [x] 100 % de `LineBreakTest.txt` de la version UCD figée par US-010 passent sur l'algorithme Unicode sans adaptation locale.
- [x] U+00A0 et U+202F suivent les règles de la classe GL, y compris l'exception LB12a autorisant notamment une coupure après une espace ordinaire précédant GL. Les tests couvrent le cas usuel sans coupure et ces exceptions ; l'insertion typographique française (US-228) ne modifie pas ces règles.
- [x] Coupures obligatoires (saut de ligne, saut de paragraphe) distinguées des coupures permises.

Réf : §6.4 · Taille : M · Dépend de : US-010

Validation US-012 (2026-10-11) : [contrat et preuves](docs/us-012-line-breaks.md), [validation du lot](_bmad-output/implementation-artifacts/text-layout-validation.json).

#### US-013 — Casse et normalisation
*En tant que* développeur, *je veux* les conversions de casse et la normalisation NFC/NFD *afin de* gérer la recherche et les changements de casse.
- [x] Majuscules, minuscules, casse de titre, repli de casse (y compris les cas spéciaux comme ß → SS).
- [x] 100 % de `NormalizationTest.txt` passent (NFC, NFD).

Réf : §6.15 · Taille : M · Dépend de : US-010

Validation US-013 (2026-10-12) : [contrat et preuves](docs/us-013-case-normalization.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

#### US-014 — Encodages de texte
*En tant que* développeur, *je veux* décoder et encoder les encodages courants *afin d'ouvrir* les fichiers texte et les formats anciens.
- [x] UTF-8, UTF-16 LE/BE (avec ou sans BOM), Windows-1252, ISO-8859-1, CP437 (noms ZIP anciens), MacRoman.
- [x] Détection : BOM, puis validité UTF-8, puis repli sur Windows-1252.
- [x] Séquences invalides remplacées par U+FFFD, sans panique.

Réf : §5.1 · Taille : S · Dépend de : US-001

## E0.3 — Compression et ZIP

#### US-020 — Décompression deflate
*En tant que* développeur, *je veux* décompresser deflate/zlib *afin de* lire les paquets ZIP et les images PNG.
- [x] Blocs stockés, Huffman fixe et dynamique ; enveloppe zlib ; CRC32 et Adler32.
- [x] Mode flux (entrée et sortie par morceaux).
- [x] Limite de taille de sortie paramétrable (protection contre les bombes).
- [x] Validé sur un corpus de flux de référence et en aller-retour avec US-021.

Réf : §6.2 `compress` · Taille : M · Dépend de : US-001

Validation US-020 (2026-10-10) : [contrats, preuves et limites](docs/DEVELOPMENT.md#preuve-us-020021-us-071-et-us-080--2026-10-10).

#### US-021 — Compression deflate
*En tant que* développeur, *je veux* compresser en deflate *afin d'écrire* des fichiers docx et PDF compacts.
- [x] Niveaux « rapide » et « normal » (LZ77 avec chaînes de hachage, Huffman dynamique).
- [x] Sortie toujours décompressible par US-020 (test de propriété sur données aléatoires).
- [x] Taux de compression documenté sur le corpus de test.

Réf : §6.2 `compress` · Taille : M · Dépend de : US-020

Validation US-021 (2026-10-10) : [contrats, preuves et limites](docs/DEVELOPMENT.md#preuve-us-020021-us-071-et-us-080--2026-10-10).

#### US-022 — Lecture ZIP
*En tant que* développeur, *je veux* lire les archives ZIP *afin d'ouvrir* les documents OOXML et ODF.
- [x] Répertoire central, Zip64, méthodes stockée et deflate, descripteurs de données, noms UTF-8 et CP437.
- [x] Limites : taux de compression maximal, taille décompressée totale, nombre d'entrées, longueur des noms.
- [x] Noms d'entrée normalisés (pas de `..`, pas de chemin absolu).
- [x] Archive tronquée ou corrompue → erreur propre ; entrées lisibles quand même accessibles.

Validation : développée et validée le 2026-10-10; [contrat et preuves](docs/us-022-023-zip.md).

Réf : §6.13 · Taille : M · Dépend de : US-020, US-014

#### US-023 — Écriture ZIP
*En tant que* développeur, *je veux* écrire des archives ZIP *afin d'enregistrer* les documents.
- [x] Ordre des entrées contrôlé par l'appelant, horodatage fixe en option (sortie déterministe).
- [x] Zip64 automatique au-delà des limites 32 bits.
- [x] Archives produites lisibles par US-022 et par l'Explorateur Windows.

Validation : développée et validée le 2026-10-10; [contrat et preuves](docs/us-022-023-zip.md).

Réf : §6.2 `zip` · Taille : S · Dépend de : US-021

## E0.4 — XML

#### US-030 — Analyseur XML en flux
*En tant que* développeur, *je veux* un analyseur XML en flux avec espaces de noms *afin de* lire OOXML et ODF.
- [x] Éléments, attributs, entités prédéfinies et numériques, CDATA, commentaires, instructions de traitement, espaces de noms résolus.
- [x] Encodages UTF-8 et UTF-16.
- [x] Les DTD sont ignorées : aucune entité externe, aucune expansion d'entité (protection « billion laughs »).
- [x] Limites de profondeur, de taille d'attribut et de nombre d'attributs.
- [x] Erreurs avec ligne et colonne ; mode tolérant pour les documents légèrement malformés.

Validation : développée et validée le 2026-10-10; [contrat et preuves](docs/us-030-031-xml.md).

Réf : §6.13 · Taille : L · Dépend de : US-014

#### US-031 — Écrivain XML
*En tant que* développeur, *je veux* un écrivain XML *afin de* générer les parties OOXML.
- [x] Échappement correct, déclarations d'espaces de noms, déclaration XML, sortie déterministe.
- [x] Sortie relisible par US-030 (aller-retour testé).

Validation : développée et validée le 2026-10-10; [contrat et preuves](docs/us-030-031-xml.md).

Réf : §6.2 `xml` · Taille : S · Dépend de : US-030

#### US-032 — Sous-arbres XML opaques
*En tant que* développeur, *je veux* capturer un sous-arbre XML brut et le réémettre *afin de* préserver les éléments non gérés.
- [x] Capture d'un élément et de ses descendants avec leurs espaces de noms effectifs.
- [x] Réémission sémantiquement identique dans un autre contexte d'espaces de noms.

Validation : développée, revue et validée le 2026-10-10 ; [contrat et preuves](docs/us-032-opaque-xml.md).

Réf : §5.2 (1) · Taille : S · Dépend de : US-030, US-031

## E0.5 — Images

#### US-040 — Décodeur PNG
*En tant que* développeur, *je veux* décoder les PNG *afin d'afficher* les images des documents.
- [x] Tous les types de couleur et profondeurs, entrelacement Adam7, palette, transparence `tRNS`, vérification des CRC.
- [x] Toutes les images valides de la PngSuite décodées correctement ; les invalides rejetées sans panique.
- [x] Limite de dimensions (pixels max) paramétrable.

Réf : §6.2 `image` · Taille : M · Dépend de : US-020

Validation US-040 (2026-10-10) : [contrat et preuves](docs/us-040-png.md), [validation du lot](docs/IMAGES-FONTS-PROGRESS.md).

#### US-041 — Encodeur PNG
*En tant que* développeur, *je veux* encoder en PNG *afin de* produire des miniatures et des images de test.
- [x] RGBA 8 bits et niveaux de gris, filtres adaptatifs.
- [x] Aller-retour avec US-040.

Réf : §6.2 `image` · Taille : S · Dépend de : US-040, US-021

Validation US-041 (2026-10-12) : [contrat et preuves](docs/us-041-png-encoder.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

#### US-042 — Décodeur JPEG
*En tant que* développeur, *je veux* décoder les JPEG *afin d'afficher* les photos insérées.
- [x] Baseline et progressif, Huffman, sous-échantillonnages 4:4:4 / 4:2:2 / 4:2:0, marqueurs de redémarrage.
- [x] Niveaux de gris, YCbCr, CMYK et YCCK (marqueur Adobe).
- [x] Lecture de l'orientation EXIF.
- [x] Écart moyen ≤ 1 niveau par canal par rapport aux images de référence.

Réf : §6.2 `image` · Taille : L · Dépend de : US-001

Validation US-042 (2026-10-12) : [contrat et preuves](docs/us-042-jpeg.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

#### US-043 — Décodeurs GIF et BMP
*En tant que* développeur, *je veux* décoder GIF et BMP *afin de* couvrir les formats d'image du MVP.
- [x] GIF : première image, transparence, entrelacement.
- [x] BMP : 1, 4, 8, 16, 24, 32 bits, compression RLE4 et RLE8, images inversées.

Réf : F-TXT-032 · Taille : S · Dépend de : US-001

Validation US-043 (2026-10-10) : [contrat et preuves](docs/us-043-gif-bmp.md), [validation du lot](docs/IMAGES-FONTS-PROGRESS.md).


## E0.6 — Polices et mise en forme du texte

#### US-050 — Tables de base OpenType
*En tant que* développeur, *je veux* lire les tables de base des polices *afin d'obtenir* les métriques et la correspondance caractère → glyphe.
- [x] Fichiers TTF, OTF et collections TTC.
- [x] Tables `cmap` (formats 4, 6, 12, 14), `head`, `hhea`, `hmtx`, `maxp`, `name`, `OS/2`, `post`.
- [x] Toutes les lectures bornées et vérifiées ; police invalide → erreur propre.

Réf : §6.4 · Taille : M · Dépend de : US-001

Validation — 2026-10-10 : [API et contrat](docs/us-050-opentype-tables.md), [preuves du lot](docs/MODEL-FONT-PROGRESS.md).

#### US-051 — Contours TrueType
*En tant que* développeur, *je veux* convertir les contours `glyf` en chemins *afin de* dessiner les glyphes.
- [x] Glyphes simples (courbes quadratiques, points implicites) et composites (transformations, profondeur bornée).
- [x] Rendu des glyphes de Carlito et Liberation identique aux images de référence.

Réf : §6.4 · Taille : M · Dépend de : US-050, US-060

#### US-052 — Contours CFF
*En tant que* développeur, *je veux* convertir les contours CFF en chemins *afin de* prendre en charge les polices OpenType/CFF.
- [x] Analyse CFF (INDEX, DICT, charsets), Type 2 charstrings, sous-routines locales et globales, CID-keyed.
- [x] Pile et profondeur d'appel bornées.

Réf : §6.4 · Taille : L · Dépend de : US-050, US-060

Validation US-052 (2026-10-12) : développée, revue et validée ; [contrat](docs/us-052-cff-outlines.md), [preuves du lot](_bmad-output/implementation-artifacts/foundation-integration-validation.json).

#### US-053 — Crénage et ligatures
*En tant que* développeur, *je veux* appliquer le crénage et les ligatures *afin d'obtenir* une typographie conforme à Word.
- [x] Table `kern` (format 0) et GPOS type 2 (ajustement de paires) ; GSUB type 1 (simple) et type 4 (ligatures).
- [x] Sélection des fonctionnalités `kern` et `liga` pour l'écriture latine, script et langue par défaut.

Réf : §6.4 · Taille : L · Dépend de : US-050

#### US-054 — Mise en forme du texte latin
*En tant que* développeur, *je veux* transformer une chaîne en glyphes positionnés *afin que* la mise en page et le curseur s'appuient dessus.
- [x] Sortie : glyphes, avances, décalages et **clusters** (correspondance glyphe ↔ indices de caractères, y compris pour les ligatures).
- [x] Taille, interlettrage et chasse appliqués.
- [x] L'API traite des séquences génériques (prête pour les écritures complexes plus tard).

Réf : §6.4 · Taille : M · Dépend de : US-053, US-011

Validation US-054 (2026-10-11) : [contrat et preuves](docs/us-054-latin-shaping.md), [validation du lot](_bmad-output/implementation-artifacts/text-layout-validation.json).

#### US-055 — Gestionnaire de polices
*En tant que* développeur, *je veux* un registre de polices avec repli *afin que* chaque texte trouve une police métriquement correcte.
- [x] Sources : polices embarquées, polices système (via le pont natif), polices incorporées au document.
- [x] Correspondance famille / graisse / style avec règles de proximité.
- [x] Table de repli métrique : Calibri → Carlito, Cambria → Caladea, Arial → Liberation Sans, Times New Roman → Liberation Serif, Courier New → Liberation Mono ; police générique en dernier recours.
- [x] La police réellement utilisée est consultable (pour l'interface).

Réf : §5.2 (3), §6.4 · Taille : M · Dépend de : US-050

Le registre US-055 accepte les octets système par API ; leur découverte et transfert par le pont natif restent couverts par US-113.

Validation US-055 (2026-10-11) : [contrat et preuves](docs/us-055-font-manager.md), [validation du lot](_bmad-output/implementation-artifacts/text-layout-validation.json).

#### US-056 — Polices embarquées
*En tant que* développeur, *je veux* embarquer les polices libres *afin de* garantir une mise en page identique partout.
- [x] Carlito, Caladea, Liberation Sans/Serif/Mono (toutes graisses et styles) dans `data/fonts/`.
- [x] Textes de licence OFL inclus et affichables.

Réf : §5.2 (3), §9 · Taille : S · Dépend de : —

Validation US-056 (2026-10-10) : [contrat et preuves](docs/us-056-embedded-fonts.md), [validation du lot](docs/IMAGES-FONTS-PROGRESS.md).


## E0.7 — Géométrie et rasterisation

#### US-060 — Géométrie 2D
*En tant que* développeur, *je veux* des primitives géométriques *afin de* représenter contours, formes et transformations.
- [x] Points, rectangles, matrices affines, chemins (lignes, quadratiques, cubiques, fermeture), boîtes englobantes, aplatissement des courbes avec tolérance.

Réf : §6.2 `geom` · Taille : S · Dépend de : US-001

#### US-061 — Rasteriseur logiciel
*En tant que* développeur, *je veux* un rasteriseur anti-crénelé *afin de* produire miniatures et images de test sans navigateur.
- [x] Remplissage non nul et pair-impair, couleurs unies avec transparence, contours (épaisseur, jointures, extrémités).
- [x] Images avec transformation (interpolation bilinéaire), clips rectangulaires et par chemin.
- [x] Sortie RGBA → PNG.

Réf : §6.6 · Taille : L · Dépend de : US-060

Validation US-061 (2026-10-12) : [contrat et preuves](docs/us-061-software-rasterizer.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

## E0.8 — Modèle commun

#### US-070 — Arbre de nœuds indexé
*En tant que* développeur, *je veux* un arbre de nœuds à identifiants stables dans une structure équilibrée *afin de* gérer de gros documents efficacement.
- [x] Identifiants uniques et stables pendant toute la vie du document.
- [x] Insertion, suppression, accès par position et par identifiant en O(log n).
- [x] Banc : 100 000 blocs, 10 000 insertions aléatoires < 50 ms.

Réf : §6.3 · Taille : M · Dépend de : US-001

#### US-071 — Opérations et transactions
*En tant que* développeur, *je veux* que toute modification passe par des opérations inversibles groupées en transactions *afin d'avoir* un annuler fiable et un modèle prêt pour la co-édition.
- [x] Interface `Operation` : `apply`, `invert`, sérialisation.
- [x] Transaction atomique : en cas d'échec, l'état est restauré.
- [x] Test de propriété : `apply` puis `invert` = identité, sur séquences aléatoires.

Réf : §6.3 · Taille : M · Dépend de : US-070

Validation US-071 (2026-10-10) : [contrats, preuves et limites](docs/DEVELOPMENT.md#preuve-us-020021-us-071-et-us-080--2026-10-10).

#### US-072 — Pile annuler / rétablir
*En tant que* développeur, *je veux* une pile d'annulation *afin de* fournir un annuler/rétablir illimité.
- [x] Les frappes consécutives sont regroupées (coupure sur pause > 1 s, déplacement du curseur ou espace après un mot).
- [x] Taille bornée par un budget mémoire paramétrable (les plus anciennes entrées sont éliminées).
- [x] La sélection est restaurée à l'annulation.

Validation : développée, revue et validée le 2026-10-10 ; [contrat et preuves](docs/us-072-undo-history.md).

Réf : F-COM-009 · Taille : S · Dépend de : US-071

#### US-073 — Modèle de sélection
*En tant que* développeur, *je veux* une sélection exprimée par identifiants de nœud *afin qu'elle* reste valide après chaque modification.
- [x] Ancre et tête (nœud + décalage), plusieurs plages.
- [x] Positions transformées par chaque opération (insertion avant, suppression englobante, fusion, scission).

Réf : §6.3 · Taille : M · Dépend de : US-071

Validation — 2026-10-10 : [API et contrat](docs/us-073-selection.md), [preuves du lot](docs/MODEL-FONT-PROGRESS.md).

#### US-074 — Journal de transactions
*En tant que* développeur, *je veux* sérialiser le journal de transactions *afin de* permettre la récupération après crash.
- [x] Format binaire versionné, ajout incrémental, somme de contrôle par entrée.
- [x] Chaque génération associe un identifiant de document, un instantané autonome de base (ressources incluses), une version de format et un journal portant le même identifiant de génération avec des numéros de séquence. Le contrat accepte un document sans fichier source.
- [x] Rejeu depuis cet instantané → état identique au dernier point durable ; seules les transactions validées et complètes sont appliquées. Une fin tronquée, une transaction incomplète ou une somme de contrôle incorrecte arrête le rejeu au dernier état valide, sans appliquer les entrées suivantes ; une génération incompatible est refusée avec une erreur propre.
- [x] Tests : document vierge modifié, fichier source enregistré ou remplacé depuis la création du journal, fin tronquée au milieu d'une transaction, mauvais identifiant de génération. Le fichier source courant n'est jamais utilisé comme base implicite.

Réf : F-COM-006, §6.13 · Taille : S · Dépend de : US-071

Validation — 2026-10-10 : [API et contrat](docs/us-074-recovery-journal.md), [preuves du lot](docs/MODEL-FONT-PROGRESS.md).

## E0.9 — Liste d'affichage

#### US-080 — Format de liste d'affichage
*En tant que* développeur, *je veux* une liste d'affichage par page *afin de* dessiner le même contenu sur l'écran, en SVG et en PDF.
- [x] Commandes : remplir/contourer un chemin, série de glyphes positionnés, image, empiler/dépiler un clip, transformation, zone de lien, marqueur de structure (pour l'accessibilité et le PDF balisé).
- [x] Sérialisation binaire compacte ; glyphes référencés par (police, identifiant de glyphe).

Réf : §6.6 · Taille : M · Dépend de : US-060

Validation US-080 (2026-10-10) : [contrats, preuves et limites](docs/DEVELOPMENT.md#preuve-us-020021-us-071-et-us-080--2026-10-10).

#### US-081 — Backend SVG
*En tant que* développeur, *je veux* convertir une liste d'affichage en SVG *afin de* préparer l'impression.
- [x] Un SVG par page, glyphes définis une fois (`<defs>`) puis réutilisés (`<use>`).
- [x] Images incorporées, clips, transformations, couleurs avec transparence.

Validation : développée, revue et validée le 2026-10-10 ; [contrat et preuves](docs/us-081-svg-backend.md).

Réf : §6.6 · Taille : S · Dépend de : US-080

#### US-082 — Backend raster
*En tant que* développeur, *je veux* rasteriser une liste d'affichage *afin de* générer miniatures et images de test.
- [x] Rendu via US-061, résolution paramétrable.

Réf : §6.6 · Taille : S · Dépend de : US-080, US-061

Validation US-082 (2026-10-12) : développée, revue et validée ; [contrat](docs/us-082-raster-backend.md), [preuves du lot](_bmad-output/implementation-artifacts/foundation-integration-validation.json).

## E0.10 — Pont WASM ↔ JS

#### US-090 — ABI WASM
*En tant que* développeur, *je veux* une ABI WASM écrite à la main *afin d'appeler* le moteur depuis le JS sans `wasm-bindgen`.
- [x] Exports `alloc`, `free`, `dispatch(ptr, len) -> ptr` ; chargeur JS correspondant.
- [x] Une panique Rust est convertie en erreur remontée au JS ; le moteur reste utilisable ou se réinitialise proprement.
- [x] Build de release optimisé en taille (taille suivie dans les bancs).

Réf : §6.7 · Taille : M · Dépend de : US-001

#### US-091 — Protocole de messages
*En tant que* développeur, *je veux* un protocole binaire versionné *afin d'échanger* commandes et événements entre le JS et le moteur.
- [x] Encodeur/décodeur Rust et décodeur/encodeur JS miroir, avec numéro de version.
- [x] Tests croisés : chaque type de message encodé d'un côté est décodé à l'identique de l'autre.
- [x] Message inconnu ou malformé → erreur, jamais de panique.

Réf : §6.7 · Taille : M · Dépend de : US-090

#### US-092 — Moteur dans un Web Worker
*En tant que* développeur, *je veux* exécuter le moteur dans un Web Worker *afin que* l'interface ne bloque jamais.
- [x] File de commandes, réponses associées par identifiant.
- [x] Les listes d'affichage sont copiées une fois depuis la mémoire WASM vers un `ArrayBuffer` JS indépendant, puis ce tampon est transféré au fil principal sans copie supplémentaire. `WebAssembly.Memory.buffer` n'est jamais placé dans la liste de transfert.
- [x] Test : après réception et transfert d'une liste, le moteur traite une nouvelle commande ; le tampon JS transféré est détaché côté émetteur, la mémoire WASM reste utilisable et les octets reçus correspondent à la sortie initiale. V0/V1 fonctionnent sans `SharedArrayBuffer`.
- [x] Une opération longue (ouverture, export) peut être annulée et signale sa progression.

Réf : §6.7 · Taille : S · Dépend de : US-091

## E0.11 — Socle du front-end

US-092 terminée (2026-10-17) : [transport, limites et opérations annulables](docs/us-092-worker.md). Ouverture et export DOCX par étapes bornées, progression monotone, annulation entre étapes ; l'annulation est au niveau API (aucun bouton d'interface raccordé).

#### US-100 — Interface Host
*En tant que* développeur, *je veux* une interface `Host` *afin que* le front-end soit identique sur le desktop et le web.
- [ ] Méthodes : ouvrir/enregistrer (dialogues), lire/écrire un fichier, lister/lire les polices système, imprimer, préférences, documents récents.
- [ ] Implémentation desktop via le pont natif (US-113) ; implémentation factice pour les tests.

Réf : §6.1 · Taille : S · Dépend de : US-092

#### US-101 — Rendu canvas
*En tant que* développeur, *je veux* rejouer la liste d'affichage sur un canvas *afin d'afficher* le document à l'écran.
- [ ] Glyphes convertis en `Path2D` et mis en cache par (police, glyphe).
- [ ] Prise en compte de `devicePixelRatio` (net sur écran 4K à 150 %, 200 %).
- [ ] Seules les pages visibles sont dessinées ; redessin limité aux zones modifiées.
- [ ] Défilement à 60 images/s sur un document de 300 pages.

Réf : §6.6 · Taille : L · Dépend de : US-051, US-080, US-092

En cours (2026-10-11) : [module Canvas](docs/us-101-canvas-renderer.md) raccordé aux contours ATDL réels du moteur de texte. Cache par glyphe et validation matérielle 60 fps restent ouverts.

#### US-102 — Internationalisation
*En tant que* développeur, *je veux* un catalogue de chaînes *afin qu'aucun* texte d'interface ne soit en dur.
- [x] Catalogue `web/i18n/fr-FR.txt` (clé = valeur), paramètres nommés, pluriels.
- [x] Outil de vérification : clés manquantes, inutilisées, chaînes en dur détectées dans le JS.
- [x] Formatage des nombres, dates et unités selon fr-FR.

Avancement US-102 (2026-10-17) : `web/i18n.mjs` branché dans `web/app.mjs` (catalogue chargé au démarrage, `i18n` injecté dans la navigation, la barre d’état et le dialogue des symboles), `/i18n.mjs` embarqué ; la référence de `tests/check-i18n.mjs` est vide (0 écart, exemptions justifiées) ; [contrat](docs/us-102-i18n.md). La WebView embarquée n’a pas été relancée (vérification faite avec Edge sur les suites navigateur).

Réf : D12, §6.15 · Taille : S · Dépend de : US-001

#### US-103 — Capture de la saisie
*En tant que* développeur, *je veux* capter le clavier via une zone de texte cachée *afin de* gérer touches mortes, AltGr et IME.
- [ ] Zone de texte invisible positionnée au curseur, focus conservé.
- [ ] Gestion de `keydown`, `beforeinput`, `compositionstart/update/end` → commandes moteur.
- [ ] Touches mortes du clavier AZERTY (^ ¨ `) et AltGr (€, @, #, {, [) produisent les bons caractères ; Alt+code numérique fonctionne.
- [ ] Texte en cours de composition affiché souligné dans le document.

Réf : §6.7 · Taille : M · Dépend de : US-092

En cours (2026-10-11) : [adaptateur clavier/IME](docs/us-103-text-input.md) raccordé aux commandes métier, avec précomposition soulignée. Les essais AZERTY/IME physiques restent ouverts.

#### US-104 — Bibliothèque de composants d'interface
*En tant que* développeur, *je veux* des composants d'interface maison accessibles *afin de* construire ruban et dialogues.
- [x] Bouton, bouton bascule, bouton partagé, liste déroulante, combobox de police (aperçu dans la police), sélecteur de couleur (thème, standard, personnalisée), champ numérique avec unité, dialogue modal, menu contextuel, onglets, info-bulle.
- [x] Chaque composant : rôle et attributs ARIA, navigation clavier conforme aux motifs WAI-ARIA, focus visible.

Réf : D13, §6.14 · Taille : L · Dépend de : US-102

Validation (2026-10-13) : [contrat, essais et limites](docs/us-104-105-106-interface.md), [preuve Windows/Edge](_bmad-output/implementation-artifacts/ui-shell-validation.json). Les essais physiques clavier et lecteurs d’écran restent à effectuer; aucun audit complet WCAG n’est revendiqué.

#### US-105 — Thèmes
*En tant qu'*utilisateur, *je veux* choisir un thème clair, sombre ou système *afin de* travailler confortablement.
- [x] Variables CSS par thème ; bascule sans rechargement.
- [x] « Système » suit le réglage Windows en direct.
- [x] Mode contraste élevé Windows respecté (`forced-colors`).
- [x] Le document reste rendu avec ses vraies couleurs (seule l'interface change).

Réf : F-COM-019 · Taille : S · Dépend de : US-104

Validation (2026-10-13) : [contrat, essais et limites](docs/us-104-105-106-interface.md), [preuve Windows/Edge](_bmad-output/implementation-artifacts/ui-shell-validation.json). Les essais physiques clavier et lecteurs d’écran restent à effectuer; aucun audit complet WCAG n’est revendiqué.

#### US-106 — Ruban
*En tant qu'*utilisateur, *je veux* un ruban à onglets *afin de* trouver les commandes comme dans Office.
- [x] Onglets Fichier, Accueil, Insertion, Mise en page, Références, Affichage ; groupes et commandes déclarés dans une table (pas de code par bouton).
- [x] Groupes réduits automatiquement quand la fenêtre est étroite ; ruban repliable (Ctrl+F1).
- [x] Touche Alt : affichage des info-touches et navigation au clavier.

Réf : F-COM-018 · Taille : M · Dépend de : US-104

Validation (2026-10-13) : [contrat, essais et limites](docs/us-104-105-106-interface.md), [preuve Windows/Edge](_bmad-output/implementation-artifacts/ui-shell-validation.json). Les essais physiques clavier et lecteurs d’écran restent à effectuer; aucun audit complet WCAG n’est revendiqué.

## E0.12 — Shell Windows

#### US-110 — Fenêtre Win32
*En tant que* développeur, *je veux* une fenêtre Win32 native *afin d'héberger* la WebView.
- [x] Boucle de messages, prise en charge du DPI par écran (Per-Monitor V2), barre de titre au thème du système.
- [x] Taille, position et état (agrandi) mémorisés et restaurés, y compris en multi-écran.

Réf : §6.8 · Taille : S · Dépend de : US-001

Validation US-110 (2027-10-11) : [contrat et limites](docs/us-110-win32-window.md), [preuve du lot](_bmad-output/implementation-artifacts/worker-shell-validation.json).

#### US-111 — Liaisons WebView2
*En tant que* développeur, *je veux* des liaisons COM WebView2 écrites à la main *afin d'afficher* le front-end sans Tauri.
- [x] Tables virtuelles des interfaces nécessaires déclarées en Rust ; comptage de références correct : après 100 ouvertures/fermetures, 0 callback vivant et toute référence COM acquise relâchée. La croissance des handles `Process` du processus hôte, attribuée au runtime WebView2 ([WebView2Feedback #5704](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5704)), est mesurée et publiée comme indicateur non bloquant.
- [x] Environnement créé avec le dossier de données résolu par US-116 (`atelier-data/` en mode portable) ; aucun cache n'est créé avant la résolution de ce dossier.
- [x] Redimensionnement, focus et DPI suivis.
- [x] Outils de développement activés en build de debug uniquement.

Réf : D9, §6.8 · Taille : L · Dépend de : US-110, US-116

Terminée (2026-10-17) : critère reformulé par le propriétaire du produit ; registre COM équilibré sur 100 cycles réels (4946/4946 références, 2882/2882 AddRef/Release, 0 callback vivant). Le contrôle strict des handles reste publié comme indicateur non bloquant ; [preuves et limites](docs/us-111-webview2.md).

#### US-112 — Ressources embarquées et isolement réseau
*En tant que* développeur, *je veux* servir les ressources depuis l'exe et bloquer tout le reste *afin que* l'application soit totalement déconnectée.
- [x] Ressources incorporées à la compilation, servies sur `https://app.atelier/` via `WebResourceRequested`, avec le bon type MIME.
- [ ] CSP stricte ; toute requête ou navigation vers une autre origine est bloquée et journalisée.
- [x] Les liens externes ne naviguent jamais dans la WebView.

Réf : §1, §6.8 · Taille : M · Dépend de : US-111

Avancement US-112 (2026-10-11) : [origine embarquée, table HTTP et six catégories CSP validées](docs/us-112-embedded-resources.md). La journalisation CSP interne aux Workers et les scénarios service-worker restent ouverts ; l’US complète n’est pas terminée.

#### US-113 — Pont natif ↔ JS
*En tant que* développeur, *je veux* un pont entre le natif et le JS *afin d'accéder* aux fichiers et au système.
- [ ] Messages `postMessage` pour les commandes, `SharedBuffer` pour les données volumineuses (fichier de 100 Mo transféré < 300 ms).
- [ ] API : dialogue d'ouverture, dialogue d'enregistrement (`IFileOpenDialog`/`IFileSaveDialog`, filtres par format), lecture de fichier, écriture atomique, liste et lecture des polices système, ouverture d'une URL dans le navigateur par défaut.
- [ ] Toute erreur système remontée avec un message traduisible.

Réf : §6.8 · Taille : M · Dépend de : US-111

Avancement US-113 (2026-10-11) : [pont fichiers JS/natif](docs/us-113-file-bridge.md) raccordé au broker et aux dialogues TXT, avec erreurs traduisibles, bornes, anti-rejeu et protection contre la réentrance. SharedBuffer, polices système, URL et validation manuelle des dialogues restent ouverts.

#### US-114 — Runtime WebView2 absent
*En tant qu'*utilisateur, *je veux* un message clair si le runtime WebView2 manque *afin de* savoir comment y remédier.
- [x] Détection avant création de la WebView ; boîte de dialogue native expliquant le problème avec l'adresse de téléchargement Microsoft (texte copiable) ; pas de plantage.

Réf : §6.8, §10 · Taille : S · Dépend de : US-110

Validation US-114 (2026-10-11) : [sonde injectée, diagnostic natif et barrière avant cache](docs/us-111-webview2.md).

#### US-115 — Instance unique et arguments
*En tant qu'*utilisateur, *je veux* que l'ouverture d'un document depuis l'Explorateur utilise la fenêtre déjà ouverte *afin de* garder mes documents ensemble.
- [ ] Mutex nommé ; transmission des chemins à l'instance existante (`WM_COPYDATA`) ; mise au premier plan.
- [ ] Plusieurs fichiers en argument ouverts chacun dans un onglet.

Réf : §6.8, F-COM-002 · Taille : S · Dépend de : US-113

#### US-116 — Mode portable
*En tant qu'*utilisateur, *je veux* que les données gérées par l'application restent dans son dossier en mode portable *afin de* l'emporter sur une clé USB.
- [x] Données (préférences, récupération, récents, cache WebView2) dans `atelier-data/` à côté de l'exe ; aucune écriture dans le registre.
- [x] Si le dossier n'est pas inscriptible : avant toute création de cache ou de données, dialogue natif proposant de déplacer l'exe vers un dossier inscriptible, de choisir explicitement le mode local sous `%LOCALAPPDATA%\Atelier`, ou de quitter. Aucun repli automatique.
- [x] En mode local, préférences, récupération, récents, rapports de crash et cache utilisent cette même racine ; le mode est identifié dans l'interface. Un dossier de données toujours inaccessible empêche le démarrage avec un message clair. Les traces produites indépendamment par Windows ne relèvent pas de la garantie portable.

Réf : D6, §7.2 · Taille : S · Dépend de : US-110

## E0.13 — Tranche verticale

Validation US-116 (2027-10-11) : [contrat et limites](docs/us-116-portable-data.md), [preuve du lot](_bmad-output/implementation-artifacts/worker-shell-validation.json).

#### US-120 — Démonstrateur de bout en bout
*En tant que* développeur, *je veux* une tranche verticale fonctionnelle *afin de* valider l'architecture avant de construire le traitement de texte.
- [ ] Ouvrir un `.txt`, l'afficher en Carlito 11 pt, paginé A4 (mise en page simplifiée).
- [ ] Curseur clignotant, frappe au clavier AZERTY, retour arrière, annuler/rétablir.
- [ ] Enregistrer en `.txt` de façon atomique.
- [ ] Latence de frappe mesurée < 16 ms sur un texte de 300 pages ; démarrage à froid < 1,5 s.

Réf : §6 · Taille : M · Dépend de : US-012, US-054, US-055, US-072, US-101, US-103, US-113

Avancement US-120 (2026-10-11) : premier [éditeur intégré](docs/us-120-editor-ui.md), avec session WASM, pages A4/Carlito, saisie/sélection, annuler/rétablir et pont fichiers TXT. Les dialogues manuels, AZERTY/IME physique et performances restent à valider ; les critères complets ne sont pas encore cochés.

---

# V1 — MVP traitement de texte (Windows, portable)

## E1.1 — Modèle du traitement de texte

#### US-200 — Modèle de document texte
*En tant que* développeur, *je veux* le modèle du traitement de texte *afin de* représenter tout le contenu d'un docx.
- [x] Nœuds : document, section, paragraphe (texte + plages d'attributs + objets ancrés), tableau / ligne / cellule, bloc opaque, en-tête/pied, champ, signet, lien.
- [x] Opérations : insérer/supprimer du texte, appliquer des attributs, scinder/fusionner des paragraphes, insérer/supprimer des blocs, opérations de tableau (lignes, colonnes, fusion), insérer/modifier un objet.
- [x] Chaque opération est inversible et testée en propriété (US-071).

Réf : §6.3 · Taille : L · Dépend de : US-070, US-071, US-073

Validation US-200 (2026-10-12) : [contrat et preuves](docs/us-200-rich-text-model.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

#### US-201 — Moteur de styles
*En tant que* développeur, *je veux* résoudre les propriétés calculées selon la cascade Word *afin que* l'affichage corresponde au fichier.
- [x] Cascade : valeurs par défaut du document → styles de tableau → chaîne de styles de paragraphe (`basedOn`) → style de caractère → mise en forme directe ; styles liés.
- [x] Propriétés calculées mises en cache, invalidées finement par opération.
- [x] Couleurs et polices de thème résolues.

Réf : F-TXT-020 · Taille : M · Dépend de : US-200

Validation US-201 (2026-10-12) : [contrat et preuves](docs/us-201-style-engine.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

#### US-202 — Document par défaut
*En tant qu'*utilisateur, *je veux* qu'un nouveau document ait les réglages habituels de Word *afin de* ne rien avoir à configurer.
- [ ] A4 portrait, marges 2,5 cm, Calibri (Carlito) 11 pt, interligne 1,08, 8 pt après, langue fr-FR.
- [ ] Styles par défaut : Normal, Titre, Sous-titre, Titre 1 à 9, Citation, Citation intense, Accentuation, Élevé, Légende, En-tête, Pied de page, Lien hypertexte, Paragraphe de liste, Sans interligne, Grille du tableau.


Fabrique Rust validée : [profil par défaut](docs/us-202-default-document.md), 25 styles, métadonnées A4/marges/langue et interligne exact 108/100. Auto259/240 est l’arrondi du modèle. **US partielle** : géométrie dans DefaultPresentation, bouton Nouveau riche non raccordé; critères utilisateur conservés ouverts.

Preuve du lot : 739 tests workspace Windows, 78 tests text-layout release et 21 contrôles dans le [reçu](_bmad-output/implementation-artifacts/rich-incremental-borders-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/rich-incremental-borders-matrix.json).

Réf : F-TXT-021 · Taille : S · Dépend de : US-201

## E1.2 — Mise en page

#### US-210 — Mise en page des lignes
*En tant qu'*utilisateur, *je veux* que les lignes soient coupées et alignées comme dans Word *afin que* mes documents s'affichent fidèlement.
- [x] Coupure gloutonne sur les opportunités UAX#14, mot trop long coupé de force.
- [x] Alignements gauche, centré, droite, justifié (dernière ligne non justifiée).
- [x] Retraits gauche, droit, première ligne, suspendu.
- [x] Interlignes simple, 1,5, double, multiple, au moins, exactement.
- [x] Mélange de tailles et polices dans une ligne : hauteur de ligne et ligne de base correctes.
- [ ] Sur le corpus de référence, ≥ 95 % des lignes coupées au même endroit que dans Word.

Validation API : [US-210](docs/us-210-rich-line-layout.md), 540 tests workspace et 15 contrôles dans le [reçu](_bmad-output/implementation-artifacts/docx-rich-layout-validation.json). **US partielle** : Microsoft Word absent de cet hôte, aucun taux de comparaison établi. Le collecteur est préparatoire. Un [aperçu DOCX A4 en lecture seule](docs/rich-docx-preview.md) raccorde désormais le layout au produit ; l’édition riche et la fidélité Word restent ouvertes.

Réf : §6.5 · Taille : L · Dépend de : US-054, US-201

#### US-211 — Tabulations
*En tant qu'*utilisateur, *je veux* utiliser des tabulations *afin d'aligner* du texte.
- [x] Taquets gauche, centré, droit, décimal (virgule en fr-FR), barre ; points de suite (points, tirets, traits).
- [ ] Taquets par défaut tous les 1,25 cm ; comportement de Word en dépassement de marge.

API et géométrie validées : [tabulations](docs/us-211-tabs.md), intervalle par défaut et retraits testés. **US partielle** : comportement de dépassement de marge non comparé à Word; le critère composé reste ouvert. Décorations retournées à l’appelant, sans raccordement UI riche.

Preuve du lot : 707 tests workspace, 57 tests text-layout release et 20 contrôles dans le [reçu](_bmad-output/implementation-artifacts/rich-page-geometry-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/rich-page-geometry-matrix.json).

Réf : F-TXT-011 · Taille : M · Dépend de : US-210

#### US-212 — Pagination
*En tant qu'*utilisateur, *je veux* que le texte soit réparti en pages comme dans Word *afin d'obtenir* le même nombre de pages.
- [x] Espacements avant/après, y compris l'espacement contextuel et la suppression de l'espacement avant en haut de page selon les règles de Word.
- [x] Veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant, sauts de page manuels.
- [ ] Sur le corpus de référence, ≥ 95 % des documents ont le même nombre de pages que dans Word.

Règles fonctionnelles validées : [pagination](docs/us-212-pagination.md), tests numériques et chaîne layout → pages → curseur. **US partielle** : le seuil de 95 % de documents ayant le même nombre de pages que Word n’est pas mesuré; aucune référence Word authentique disponible.

Preuve du lot : 707 tests workspace, 57 tests text-layout release et 20 contrôles dans le [reçu](_bmad-output/implementation-artifacts/rich-page-geometry-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/rich-page-geometry-matrix.json).

Réf : F-TXT-012, §6.5 · Taille : L · Dépend de : US-210

#### US-213 — Mise en page incrémentale
*En tant qu'*utilisateur, *je veux* que la frappe reste instantanée même dans un long document *afin de* travailler sans gêne.
- [x] Seuls les paragraphes modifiés sont remis en page ; la pagination reprend à la première page touchée et s'arrête dès qu'elle se stabilise.
- [ ] Latence de frappe < 16 ms au milieu d'un document de 300 pages.
- [x] Le résultat incrémental est identique à une mise en page complète (vérifié par test).


API validée : [mise en page incrémentale](docs/us-213-incremental-layout.md), cache sémantique et reprise/arrêt réels; résultats égaux au calcul complet, y compris après mutations de polices et changements structurels. **US partielle** : benchmark CPU 300 pages optimisé, médianes locales 71.778–73.641 ms sur deux séries de sept mesures i9-13900KF/64 Gio (ancien lot : 268.214 ms), 3 pages recalculées/297 réutilisées; cible de frappe <16 ms non atteinte ni certifiée, UI riche non raccordée.

Preuve du lot : 739 tests workspace Windows, 78 tests text-layout release et 21 contrôles dans le [reçu](_bmad-output/implementation-artifacts/rich-incremental-borders-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/rich-incremental-borders-matrix.json).

Optimisation validée : clones de cache supprimés, mise à jour identique 0.033–0.037 ms et préflight global conservé. [Mesures](_bmad-output/implementation-artifacts/editor-incremental-perf-benchmark.json), [validation du lot](_bmad-output/implementation-artifacts/editor-drag-spacing-perf-validation.json).

Réf : §6.5, §6.16 · Taille : L · Dépend de : US-212

#### US-214 — Géométrie du curseur
*En tant que* développeur, *je veux* convertir positions et coordonnées *afin de* placer le curseur et dessiner la sélection.
- [x] Position → rectangle du curseur ; point → position la plus proche (y compris dans les ligatures, via les clusters).
- [x] Rectangles de sélection sur plusieurs lignes et pages.
- [x] Déplacement vertical conservant la colonne visuelle d'origine.

API riche validée : [géométrie du curseur](docs/us-214-caret-geometry.md), clusters, sélections multipages et navigation verticale avec affinité. Interpolation uniforme documentée dans les ligatures sans données GDEF. Le raccordement à l’interface riche reste séparé. La tranche TXT était déjà validée le 2026-10-11.

Preuve du lot : 707 tests workspace, 57 tests text-layout release et 20 contrôles dans le [reçu](_bmad-output/implementation-artifacts/rich-page-geometry-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/rich-page-geometry-matrix.json).

Réf : §6.5 · Taille : M · Dépend de : US-210

#### US-215 — Bordures et trame de paragraphe (rendu)
*En tant qu'*utilisateur, *je veux* voir les bordures et trames de paragraphe *afin de* conserver la présentation de mes documents.
- [ ] Bordures haut, bas, gauche, droite et entre paragraphes ; styles, épaisseurs, couleurs, espacement.
- [ ] Paragraphes consécutifs de même bordure regroupés dans un seul cadre, comme dans Word.


Géométrie et commandes validées : [bordures et trames](docs/us-215-paragraph-borders.md), cadres partagés/multipages, entre paragraphes, styles et sortie DisplayList/SVG. **US partielle** : politiques fournies par l’appelant, persistance/import OOXML/UI riche et comparaison Word encore ouverts; critères utilisateur conservés ouverts.

Preuve du lot : 739 tests workspace Windows, 78 tests text-layout release et 21 contrôles dans le [reçu](_bmad-output/implementation-artifacts/rich-incremental-borders-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/rich-incremental-borders-matrix.json).

Réf : F-TXT-012 · Taille : S · Dépend de : US-212

## E1.3 — Édition

#### US-220 — Saisir du texte

Préparation DOCX du 2026-10-14 : [session riche conservée](docs/us-220-rich-session.md) pour remplacer du texte dans un paragraphe et annuler/rétablir sans aplatir les styles. API Rust uniquement ; saisie riche dans l'interface, IME et structure des paragraphes restent à raccorder et valider.
*En tant qu'*utilisateur, *je veux* taper du texte avec mon clavier français *afin de* rédiger mes documents.
- [ ] Caractères, accents par touches mortes, AltGr (€, @…), majuscules accentuées via le pavé ou Alt+code.
- [ ] Entrée : nouveau paragraphe (style suivant appliqué) ; Maj+Entrée : saut de ligne ; Ctrl+Entrée : saut de page.
- [ ] Retour arrière et Suppr par graphème ; Ctrl+Retour arrière et Ctrl+Suppr par mot.
- [ ] Taper avec une sélection active la remplace.
- [ ] Mode refrappe (Inser) désactivé par défaut, activable dans les options.

Tranche TXT validée le 2026-10-11 : Ctrl+Retour arrière/Suppr par mot et annulation ; voir [navigation](docs/us-221-keyboard-editing.md). Les critères de saisie riche/physique restent ouverts.

Réf : F-TXT-001 · Taille : M · Dépend de : US-103, US-200, US-214

#### US-221 — Déplacer le curseur au clavier
*En tant qu'*utilisateur, *je veux* naviguer au clavier *afin de* me déplacer rapidement dans le texte.
- [ ] Flèches ; Ctrl+←/→ par mot ; Ctrl+↑/↓ par paragraphe ; Début/Fin de ligne ; Ctrl+Début/Fin du document ; Page préc./suiv. ; Ctrl+Page préc./suiv. vers l'objet précédent/suivant.
- [ ] La vue défile pour garder le curseur visible.

Tranche TXT validée le 2026-10-11 : Ctrl+flèches par mot/paragraphe, maintien du X ; voir [contrat](docs/us-221-keyboard-editing.md). Navigation entre objets riches encore absente.

Réf : F-TXT-002 · Taille : S · Dépend de : US-214

#### US-222 — Sélectionner à la souris
*En tant qu'*utilisateur, *je veux* sélectionner à la souris *afin de* modifier une partie du texte.
- [ ] Clic : place le curseur ; double-clic : mot ; triple-clic : paragraphe ; Ctrl+clic : phrase ; clic dans la marge gauche : ligne (double : paragraphe, triple : document).
- [ ] Glisser avec défilement automatique aux bords ; extension par mot après un double-clic.
- [ ] Maj+clic étend la sélection ; Alt+glisser sélectionne un bloc rectangulaire.

Tranche TXT validée le 2026-10-11 : glisser, auto-scroll, Maj+clic, double-clic mot et triple-clic paragraphe ; voir [contrat](docs/us-222-mouse-selection.md). Phrase, marge et bloc rectangulaire restent ouverts.

Réf : F-TXT-002 · Taille : M · Dépend de : US-214

#### US-223 — Sélectionner au clavier
*En tant qu'*utilisateur, *je veux* sélectionner au clavier *afin de* travailler sans souris.
- [ ] Maj + toutes les combinaisons de US-221 ; Ctrl+A sélectionne tout ; F8 mode extension (Échap pour sortir).

Tranche TXT validée le 2026-10-11 : Ctrl+A, Maj+mouvements disponibles et F8/Échap ; Ctrl+A quitte F8. Les combinaisons liées aux objets riches restent ouvertes.

Réf : F-TXT-002 · Taille : S · Dépend de : US-221

#### US-224 — Glisser-déposer du texte
*En tant qu'*utilisateur, *je veux* déplacer du texte en le faisant glisser *afin de* réorganiser mon document.
- [x] Glisser une sélection la déplace ; avec Ctrl, la copie ; indicateur d'insertion visible.
- [x] Une seule transaction annulable.
- [ ] Déposer du texte provenant d'une autre application l'insère.

Tranche TXT validée : déplacement/copie avec Ctrl, indicateur et transaction unique annulable; voir [contrat](docs/us-224-text-drag-drop.md). Dépôt externe `text/plain` testé sur le vrai WASM avec événements synthétiques; essai natif interapplication Windows encore requis, donc US partielle.

Preuve du lot : 749 tests workspace Windows, 78 tests text-layout release, 82 assertions navigateur et 23 contrôles dans le [reçu](_bmad-output/implementation-artifacts/editor-drag-spacing-perf-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/editor-drag-spacing-perf-matrix.json).

Réf : F-TXT-003 · Taille : M · Dépend de : US-222

#### US-225 — Copier, couper, coller
*En tant qu'*utilisateur, *je veux* utiliser le presse-papiers *afin d'échanger* du contenu entre documents et applications.
- [ ] Copie au format interne riche (sans perte entre documents Atelier), HTML et texte brut.
- [ ] Coller du HTML (navigateur, Word) : paragraphes, titres, gras/italique/souligné, couleurs, listes, tableaux, liens, images ; scripts et styles dangereux ignorés.
- [ ] Ctrl+Maj+V : coller sans mise en forme ; options de collage proposées après un collage (conserver la mise en forme source, fusionner, texte seul).
- [ ] Coller une image depuis le presse-papiers l'insère.

Tranche TXT validée le 2026-10-11 : copy/cut/paste text/plain synchrones, Ctrl+Maj+V, sélection UTF-8 et coupe annulable ; voir [contrat](docs/us-225-plain-clipboard.md). HTML/interne riche, images et options restent ouverts.

Réf : F-COM-016 · Taille : L · Dépend de : US-200, US-270

#### US-226 — Annuler et rétablir

Préparation DOCX du 2026-10-14 : session riche Rust, remplacement dans un paragraphe et historique conservant sélection/styles/métadonnées. Le raccordement UI et l'enregistrement restent ouverts ; voir [contrat US-220/226](docs/us-220-rich-session.md).
*En tant qu'*utilisateur, *je veux* annuler et rétablir mes actions *afin de* corriger mes erreurs sans crainte.
- [ ] Ctrl+Z / Ctrl+Y, boutons de la barre d'accès rapide avec liste déroulante des actions (libellés lisibles : « Frappe », « Mise en gras »…).
- [ ] Le curseur et la vue reviennent à l'endroit de l'action.
- [ ] L'historique est conservé après un enregistrement.

Réf : F-COM-009 · Taille : S · Dépend de : US-072, US-220

#### US-227 — Caractères spéciaux
*En tant qu'*utilisateur, *je veux* insérer des symboles *afin d'écrire* des caractères absents du clavier.
- [ ] Dialogue Symbole : grille par police et par bloc Unicode, recherche par nom, symboles récents.
- [x] Onglet caractères spéciaux : tirets cadratin et demi-cadratin, espace insécable, espace fine insécable, trait d'union insécable, ©, ®, ™, §, ¶, points de suspension.
- [x] Ctrl+Maj+Espace : espace insécable.

Réf : F-TXT-004 · Taille : S · Dépend de : US-104, US-220

Avancement US-227 (2026-10-14) : US-227 reste partielle : le choix parmi plusieurs polices et la mise en forme/édition riche ne sont pas encore raccordés. La police du TXT est Carlito. L’aperçu DOCX reste en lecture seule. [Contrat et validation](docs/us-227-editor-utilities.md).


#### US-228 — Espaces insécables automatiques
*En tant qu'*utilisateur, *je veux* que les espaces insécables soient insérés automatiquement selon la typographie française *afin d'éviter* une ponctuation isolée en début de ligne.
- [x] Espace insécable avant `:` et dans les guillemets « » ; espace fine insécable avant `; ! ?`.
- [x] Désactivable dans les options ; Ctrl+Z juste après annule uniquement la substitution.

Validée dans le démonstrateur TXT : règles françaises, option activée par défaut, préférence locale et substitution annulable séparément; voir [contrat](docs/us-228-french-spacing.md). Collage/composition/remplacement restent exacts. Le raccordement à la saisie riche US-220 reste séparé.

Preuve du lot : 749 tests workspace Windows, 78 tests text-layout release, 82 assertions navigateur et 23 contrôles dans le [reçu](_bmad-output/implementation-artifacts/editor-drag-spacing-perf-validation.json); [audit des matrices](_bmad-output/implementation-artifacts/editor-drag-spacing-perf-matrix.json).

Réf : F-TXT-004 · Taille : S · Dépend de : US-220

#### US-229 — Correction automatique
*En tant qu'*utilisateur, *je veux* une correction automatique de base *afin de* taper plus vite avec une typographie soignée.
- [ ] Guillemets droits → « » ; apostrophe droite → ’ ; majuscule en début de phrase ; deux majuscules en début de mot corrigées ; `--` → tiret ; `(c)` → ©.
- [ ] Liste de remplacements modifiable ; chaque règle désactivable ; Ctrl+Z juste après annule la correction.

Réf : F-TXT-005 · Taille : S · Dépend de : US-220

#### US-230 — Menu contextuel
*En tant qu'*utilisateur, *je veux* un menu au clic droit *afin d'accéder* aux actions pertinentes.
- [ ] Contenu selon le contexte : texte, lien, tableau, image, en-tête.
- [x] Ouvrable au clavier (touche Menu, Maj+F10).

Réf : F-COM-018 · Taille : S · Dépend de : US-104

Avancement US-230 (2026-10-14) : US-230 reste partielle pour les contextes lien, tableau, image et en-tête, encore absents de l’éditeur riche. Les formats clipboard HTML/RTF ne sont pas ajoutés. [Contrat et validation](docs/us-230-editor-utilities.md).


#### US-231 — Raccourcis clavier de Word en français
*En tant qu'*utilisateur habitué à Word, *je veux* retrouver les mêmes raccourcis *afin de* ne rien réapprendre.
- [ ] Table de correspondance établie en relevant les raccourcis de Word 365 en français (annexe à ajouter au dépôt), par exemple Ctrl+G gras, Ctrl+I italique, Ctrl+U souligné.
- [ ] Tous les raccourcis de la table correspondant à une fonction V1 sont implémentés, déclarés dans une seule table.
- [ ] La table fige les affectations suivantes : Ctrl+Maj+V colle le texte seul (US-225), Ctrl+Alt+C copie la mise en forme et Ctrl+Alt+V l'applique (US-247). Un contrôle détecte deux commandes actives dans un même contexte avec la même combinaison ; l'absence de mise en forme copiée désactive la commande correspondante sans changer le sens du raccourci.
- [ ] Affichés dans les info-bulles des commandes.

Référence pour ces trois affectations : [raccourcis Word pour Windows, documentation Microsoft](https://support.microsoft.com/fr-fr/accessibility/word/keyboard-shortcuts-in-word), consultée le 2026-10-09. Les autres affectations sont relevées sur l'installation française de référence prévue par cette story.

Réf : F-COM-020 · Taille : M · Dépend de : US-106

## E1.4 — Mise en forme, styles et listes

#### US-240 — Mettre en forme les caractères depuis le ruban
*En tant qu'*utilisateur, *je veux* changer l'apparence du texte *afin de* mettre en valeur certains passages.
- [ ] Police (liste avec aperçu, polices récentes, polices du thème), taille (liste + saisie libre, Ctrl+Maj+> / <), gras, italique, souligné (simple, double, épais, pointillé, tirets, ondulé), barré, exposant, indice, couleur de police, surlignage.
- [ ] Sans sélection, la mise en forme s'applique au mot sous le curseur, comme dans Word.
- [ ] L'état des boutons reflète la sélection (état mixte affiché).
- [ ] Si la police demandée est absente, la police de remplacement est indiquée (info-bulle).

Réf : F-TXT-010 · Taille : M · Dépend de : US-106, US-201

#### US-241 — Dialogue Police
*En tant qu'*utilisateur, *je veux* un dialogue Police complet *afin d'accéder* aux options avancées.
- [ ] Tout ce qui est dans US-240, plus double barré, petites majuscules, majuscules, masqué, espacement des caractères (étendu, condensé), échelle, position, crénage à partir de N points.
- [ ] Aperçu en direct ; bouton « Par défaut » qui modifie le style Normal du document.

Réf : F-TXT-010 · Taille : M · Dépend de : US-240

#### US-242 — Modifier la casse
*En tant qu'*utilisateur, *je veux* changer la casse d'une sélection *afin de* corriger rapidement un texte.
- [ ] Phrase, minuscules, MAJUSCULES, Majuscule À Chaque Mot, iNVERSER ; Maj+F3 alterne les modes ; accents conservés.

Réf : F-TXT-010 · Taille : S · Dépend de : US-013, US-240

#### US-243 — Mettre en forme les paragraphes depuis le ruban
*En tant qu'*utilisateur, *je veux* aligner et espacer mes paragraphes *afin de* structurer la page.
- [ ] Alignements, augmenter/diminuer le retrait, interligne (valeurs courantes + options), ajout/suppression de l'espacement avant/après, trame de fond, bordures (menu), afficher les marques de mise en forme.
- [ ] S'applique à tous les paragraphes touchés par la sélection.

Réf : F-TXT-011 · Taille : M · Dépend de : US-106, US-201

#### US-244 — Dialogue Paragraphe
*En tant qu'*utilisateur, *je veux* un dialogue Paragraphe *afin de* régler précisément retraits, espacements et enchaînements.
- [ ] Onglet « Retrait et espacement » : alignement, retraits, première ligne / suspendu, espacements avant/après, interligne et valeur, espacement contextuel.
- [ ] Onglet « Enchaînements » : veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant.
- [ ] Unités en cm et pt, saisie d'autres unités convertie ; aperçu.

Réf : F-TXT-011, F-TXT-012 · Taille : M · Dépend de : US-243

#### US-245 — Tabulations et règle
*En tant qu'*utilisateur, *je veux* gérer les tabulations et les retraits depuis la règle *afin de* travailler visuellement.
- [ ] Règle horizontale : marges, retraits (gauche, droit, première ligne) déplaçables, taquets ajoutés par clic, déplacés par glisser, supprimés en les sortant de la règle ; choix du type de taquet.
- [ ] Dialogue Tabulations : position, alignement, points de suite, définir, effacer, tout effacer, taquet par défaut.
- [ ] Règle verticale (marges haut/bas).

Réf : F-TXT-011, F-TXT-056 · Taille : M · Dépend de : US-211, US-243

#### US-246 — Dialogue Bordures et trame
*En tant qu'*utilisateur, *je veux* encadrer et ombrer des paragraphes *afin de* les distinguer.
- [ ] Prédéfinis (aucune, encadré, ombre), choix des côtés, style, couleur, épaisseur, espacement, trame (couleur).
- [ ] Aperçu cliquable pour activer ou désactiver chaque côté.

Réf : F-TXT-012 · Taille : S · Dépend de : US-215, US-104

#### US-247 — Reproduire la mise en forme
*En tant qu'*utilisateur, *je veux* copier la mise en forme d'un texte vers un autre *afin de* gagner du temps.
- [ ] Simple clic : une application ; double clic : mode verrouillé jusqu'à Échap ; curseur pinceau.
- [ ] Copie caractère et, si un paragraphe entier est source, paragraphe ; Ctrl+Alt+C copie la mise en forme et Ctrl+Alt+V l'applique au clavier, selon la table US-231. Ctrl+Maj+V reste réservé au collage de texte seul (US-225).

Réf : F-TXT-013 · Taille : S · Dépend de : US-240

#### US-248 — Effacer la mise en forme
*En tant qu'*utilisateur, *je veux* revenir à la mise en forme du style *afin de* nettoyer un texte collé.
- [ ] Bouton « Effacer toute la mise en forme » (caractère + paragraphe, retour au style Normal) ; Ctrl+Espace pour la mise en forme de caractère seule.

Réf : F-TXT-014 · Taille : S · Dépend de : US-240

#### US-250 — Appliquer un style
*En tant qu'*utilisateur, *je veux* appliquer des styles *afin de* garder un document cohérent.
- [ ] Galerie de styles dans le ruban avec aperçu en direct au survol.
- [ ] Volet Styles : liste (styles utilisés / tous), aperçu, indication du style courant ; raccourcis Titre 1–3 de Word.
- [ ] Appliquer un style de paragraphe conserve la mise en forme directe de caractère, comme dans Word.

Réf : F-TXT-020 · Taille : M · Dépend de : US-201, US-106

#### US-251 — Gérer les styles
*En tant qu'*utilisateur, *je veux* créer et modifier des styles *afin d'adapter* la présentation à mes besoins.
- [ ] Créer (depuis la sélection ou de zéro), modifier, renommer, supprimer (le texte reprend le style parent).
- [ ] Paramètres : nom, type, basé sur, style du paragraphe suivant, police, paragraphe, tabulations, bordures, numérotation ; « Mettre à jour selon la sélection ».
- [ ] Modifier un style met à jour tout le texte qui l'utilise en une transaction annulable.
- [ ] Les styles prédéfinis ne peuvent pas être supprimés.

Réf : F-TXT-022 · Taille : M · Dépend de : US-250

#### US-252 — Listes à puces et numérotées
*En tant qu'*utilisateur, *je veux* créer des listes *afin d'énumérer* des éléments.
- [ ] Boutons puces et numérotation avec bibliothèque de formats (•, ◦, ▪, –, ✓ ; 1. 1) a) A. i. I.).
- [ ] Création automatique : « 1. » ou « - » / « * » suivi d'une espace en début de paragraphe crée une liste (Ctrl+Z annule la conversion ; désactivable).
- [ ] Entrée sur un élément vide termine la liste ; Retour arrière au début d'un élément retire la puce.

Réf : F-TXT-023 · Taille : M · Dépend de : US-200, US-243

#### US-253 — Listes multiniveaux
*En tant qu'*utilisateur, *je veux* des listes à plusieurs niveaux *afin de* structurer un plan.
- [ ] Tab / Maj+Tab en début d'élément changent le niveau.
- [ ] Bibliothèque de listes multiniveaux (1. / 1.1. / 1.1.1., titres numérotés) ; liaison des niveaux aux styles Titre 1–9.
- [ ] Définir un nouveau format : par niveau, format du numéro, style, police, alignement, retrait, caractère après le numéro.
- [ ] Recommencer à 1, continuer la numérotation, définir la valeur de départ.

Réf : F-TXT-023 · Taille : L · Dépend de : US-252

#### US-254 — Volet de navigation
*En tant qu'*utilisateur, *je veux* voir le plan de mon document *afin de* m'y déplacer rapidement.
- [ ] Arborescence des titres (niveaux repliables), clic pour atteindre, mise en évidence de la section courante.
- [ ] Mise à jour en direct pendant la frappe ; onglet « Pages » avec miniatures.

Réf : F-TXT-024 · Taille : S · Dépend de : US-250, US-082

## E1.5 — Tableaux

#### US-260 — Insérer un tableau
*En tant qu'*utilisateur, *je veux* insérer un tableau *afin de* présenter des données en lignes et colonnes.
- [ ] Grille de sélection (jusqu'à 10 × 8) avec aperçu en direct ; dialogue pour un nombre quelconque de lignes et colonnes et le mode d'ajustement (largeur fixe, au contenu, à la fenêtre).
- [ ] Style « Grille du tableau » appliqué par défaut.

Réf : F-TXT-030 · Taille : S · Dépend de : US-261

#### US-261 — Mise en page des tableaux : largeurs et cellules
*En tant qu'*utilisateur, *je veux* que mes tableaux soient dimensionnés comme dans Word *afin de* conserver leur présentation.
- [ ] Algorithmes de largeur fixe et automatique façon Word ; largeurs en valeur absolue, en pourcentage ou automatiques ; retrait et alignement du tableau.
- [ ] Marges des cellules, alignement vertical, cellules fusionnées horizontalement et verticalement, hauteur de ligne (au moins / exacte).
- [ ] Résolution des conflits de bordures entre cellules adjacentes selon les règles de Word.
- [ ] Les tableaux imbriqués importés sont mis en page récursivement dans leur cellule, avec profondeur bornée et erreur propre au-delà de la limite. Leur contenu reste en lecture seule en V1 ; la création et l'édition de tableaux imbriqués relèvent de V3 (F-TXT-031).

Réf : F-TXT-030, §6.5 · Taille : L · Dépend de : US-210

#### US-262 — Mise en page des tableaux : pagination
*En tant qu'*utilisateur, *je veux* que les tableaux longs se répartissent correctement sur plusieurs pages *afin de* rester lisibles.
- [ ] Fractionnement des lignes entre pages (sauf si interdit), ligne d'en-tête répétée sur chaque page.
- [ ] Contenu d'une cellule plus grand qu'une page fractionné.

Réf : F-TXT-030 · Taille : M · Dépend de : US-261, US-212

#### US-263 — Modifier la structure d'un tableau
*En tant qu'*utilisateur, *je veux* naviguer et modifier la structure d'un tableau *afin de* l'adapter à mes données.
- [ ] Tab / Maj+Tab de cellule en cellule ; Tab dans la dernière cellule ajoute une ligne.
- [ ] Sélection de cellules, lignes, colonnes, tableau (souris et clavier) ; bouton « + » au survol entre lignes/colonnes.
- [ ] Insérer des lignes au-dessus/en dessous, des colonnes à gauche/à droite ; supprimer cellules, lignes, colonnes, tableau.

Réf : F-TXT-030 · Taille : M · Dépend de : US-261

#### US-264 — Fusionner et fractionner des cellules
*En tant qu'*utilisateur, *je veux* fusionner et fractionner des cellules *afin de* construire des tableaux complexes.
- [ ] Fusion d'une sélection rectangulaire (contenus concaténés en paragraphes).
- [ ] Fractionnement d'une cellule en N lignes × M colonnes.

Réf : F-TXT-030 · Taille : M · Dépend de : US-263

#### US-265 — Redimensionner lignes et colonnes
*En tant qu'*utilisateur, *je veux* redimensionner les colonnes et les lignes *afin d'ajuster* le tableau au contenu.
- [ ] Glisser une bordure verticale ou horizontale (avec repères sur la règle) ; Alt pendant le glisser affiche les mesures.
- [ ] Ajustement automatique au contenu ou à la fenêtre ; uniformiser la largeur des colonnes ou la hauteur des lignes.

Réf : F-TXT-030 · Taille : M · Dépend de : US-263

#### US-266 — Styles, bordures et propriétés de tableau
*En tant qu'*utilisateur, *je veux* mettre en forme mes tableaux *afin de* les rendre lisibles.
- [ ] Galerie de styles de tableau et options : ligne d'en-tête, ligne des totaux, lignes et colonnes à bandes, première et dernière colonne.
- [ ] Bordures et trames par cellule (outil pinceau de bordure).
- [ ] Propriétés : alignement du tableau, largeur, répétition de l'en-tête, autoriser le fractionnement des lignes, marges par défaut des cellules.

Réf : F-TXT-030 · Taille : M · Dépend de : US-263

## E1.6 — Images et objets

#### US-270 — Insérer une image
*En tant qu'*utilisateur, *je veux* insérer une image *afin d'illustrer* mon document.
- [ ] Depuis un fichier (PNG, JPEG, GIF, BMP), par collage ou par glisser-déposer.
- [ ] Insérée alignée sur le texte, réduite à la largeur de la zone de texte si nécessaire, orientation EXIF appliquée.
- [ ] Image stockée dans le document (format d'origine conservé).
- [ ] Format non pris en charge → message clair.

Réf : F-TXT-032 · Taille : M · Dépend de : US-040, US-042, US-043, US-200

#### US-271 — Redimensionner et déplacer une image
*En tant qu'*utilisateur, *je veux* ajuster la taille et la position d'une image *afin de* l'intégrer dans la page.
- [ ] Poignées d'angle (proportions conservées) et de côté ; Maj inverse le comportement ; dialogue de taille exacte (cm, %).
- [ ] Déplacement par glisser avec indicateur d'insertion ; flèches pour déplacer une image flottante.
- [ ] Bouton de réinitialisation de la taille.

Réf : F-TXT-032 · Taille : M · Dépend de : US-270

#### US-272 — Habillage des images
*En tant qu'*utilisateur, *je veux* choisir comment le texte entoure une image *afin de* maîtriser la mise en page.
- [ ] Habillages : aligné sur le texte, carré, haut et bas, derrière le texte, devant le texte.
- [ ] Ancrage à un paragraphe (icône d'ancre visible quand l'image est sélectionnée) ; position relative à la marge, la page, la colonne ou le paragraphe ; « déplacer avec le texte ».
- [ ] La mise en page contourne les objets flottants (zones d'exclusion par ligne).

Réf : F-TXT-032, §6.5 · Taille : L · Dépend de : US-271, US-212

#### US-273 — Texte alternatif
*En tant qu'*utilisateur, *je veux* décrire mes images *afin que* le document soit accessible.
- [ ] Panneau texte alternatif (description, « décoratif ») ; lu et écrit dans le docx ; exposé aux lecteurs d'écran dans l'éditeur.

Hors périmètre V1 : restitution accessible de ce texte alternatif dans un PDF balisé, prévue en V3 avec F-COM-015 (§6.14). L'export PDF V1 n'est pas déclaré balisé ni conforme PDF/UA.

Réf : D13, §6.14 · Taille : S · Dépend de : US-270

#### US-274 — Objets non pris en charge
*En tant qu'*utilisateur, *je veux* voir les objets que l'application ne sait pas modifier *afin de* ne rien perdre de mon document.
- [ ] SmartArt, graphiques, objets OLE, équations : affichage de l'image de repli du fichier (ou cadre « objet non affichable » s'il n'y en a pas).
- [ ] L'objet est sélectionnable, déplaçable et supprimable comme un bloc, et préservé intégralement à l'enregistrement.
- [ ] Info-bulle : « Cet objet ne peut pas être modifié dans cette version. »

Réf : F-TXT-040, §5.2 · Taille : M · Dépend de : US-305, US-306

## E1.7 — Mise en page du document

#### US-280 — Format, orientation et marges
*En tant qu'*utilisateur, *je veux* régler le papier et les marges *afin de* préparer l'impression.
- [ ] Formats prédéfinis (A4, A5, A3, Lettre, Légal, enveloppes courantes) et personnalisé ; portrait/paysage.
- [ ] Marges prédéfinies (normales, étroites, larges, miroir) et personnalisées, reliure.
- [ ] Application au document entier ou à la section courante.

Réf : F-TXT-050 · Taille : S · Dépend de : US-212

#### US-281 — Sections
*En tant qu'*utilisateur, *je veux* découper mon document en sections *afin d'avoir* des mises en page différentes.
- [ ] Sauts de section : page suivante, continu, page paire, page impaire.
- [ ] Chaque section a son format, ses marges, son orientation, ses en-têtes et pieds.
- [ ] Marques de saut de section visibles quand les marques de mise en forme sont affichées.

Réf : F-TXT-051 · Taille : M · Dépend de : US-280

#### US-282 — En-têtes et pieds de page
*En tant qu'*utilisateur, *je veux* des en-têtes et pieds de page *afin de* répéter des informations sur chaque page.
- [ ] Double-clic dans la marge haute ou basse pour éditer ; corps du document grisé ; Échap ou double-clic dans le corps pour sortir.
- [ ] Contenu riche (texte, tableaux, images, champs).
- [ ] Première page différente, pages paires et impaires différentes, « Lier au précédent » entre sections.
- [ ] Distance au bord de la page réglable.

Réf : F-TXT-052 · Taille : L · Dépend de : US-281

#### US-283 — Numéros de page
*En tant qu'*utilisateur, *je veux* numéroter mes pages *afin de* m'y retrouver à l'impression.
- [ ] Insertion en haut ou en bas, à gauche, au centre ou à droite (crée l'en-tête/pied si besoin) ; « Page X sur Y ».
- [ ] Formats 1, i, I, a, A, - 1 - ; numéro de départ ou continuation par section.

Réf : F-TXT-053 · Taille : M · Dépend de : US-282, US-284

#### US-284 — Champs de base
*En tant qu'*utilisateur, *je veux* insérer des champs automatiques *afin d'avoir* des informations toujours à jour.
- [ ] Champs PAGE, NUMPAGES, SECTIONPAGES, DATE, TIME avec formats français (« 9 octobre 2026 », « 09/10/2026 », « 14:30 »).
- [ ] Date insérée fixe ou à mise à jour automatique.
- [ ] F9 met à jour les champs sélectionnés ; Alt+F9 bascule l'affichage du code de champ ; champs grisés au survol.
- [ ] Champs inconnus : résultat en cache affiché et code préservé.

Réf : F-TXT-035 · Taille : M · Dépend de : US-200, US-212

#### US-285 — Modes d'affichage
*En tant qu'*utilisateur, *je veux* changer de mode d'affichage *afin d'adapter* la vue à ma tâche.
- [ ] Mode Page (par défaut) et mode Plan (niveaux de titres, développer/réduire, monter/descendre un bloc avec Alt+Maj+↑/↓).
- [ ] Afficher/masquer les règles ; afficher les marques de mise en forme (¶, espaces, tabulations, espaces insécables, sauts).
- [ ] Affichage de plusieurs pages côte à côte.

Réf : F-TXT-056 · Taille : M · Dépend de : US-250, US-212

## E1.8 — Liens et signets

#### US-290 — Liens hypertexte
*En tant qu'*utilisateur, *je veux* insérer des liens *afin de* renvoyer vers une page web, une adresse e-mail ou un endroit du document.
- [ ] Ctrl+K : texte affiché, URL, adresse e-mail, signet ou titre du document ; modifier, supprimer le lien.
- [ ] Détection automatique des URL et adresses e-mail à la frappe (désactivable).
- [ ] Ctrl+clic : lien interne → navigation ; lien externe → ouverture dans le navigateur par défaut après confirmation (l'application elle-même n'accède pas au réseau).
- [ ] Info-bulle avec l'adresse.

Réf : F-TXT-034 · Taille : M · Dépend de : US-113, US-200

#### US-291 — Signets et Atteindre
*En tant qu'*utilisateur, *je veux* poser des signets et atteindre un endroit précis *afin de* naviguer dans un long document.
- [ ] Insérer, renommer, supprimer un signet, trier par nom ou position ; affichage facultatif des crochets de signet.
- [ ] Dialogue Atteindre : page, section, ligne, signet, tableau, image ; valeurs relatives (+3).

Avancement US-291 (2026-10-13) : bibliothèque livrée dans `atelier_text_model::navigation` (signets : insérer, déplacer, renommer avec mise à jour des liens, supprimer, trier ; Atteindre : page, section, ligne, signet, tableau, objet, titre, valeurs relatives, forme `p3s2`), relue par un agent indépendant et corrigée ; [contrat](docs/us-291-bookmarks-goto.md). Reste ouvert : dialogues Signet et Atteindre, crochets de signet, défilement de la vue, table des pages et des lignes fournie par la mise en page. Un signet ne peut pas couvrir plusieurs paragraphes (limite du modèle).

Réf : F-TXT-034 · Taille : S · Dépend de : US-200

## E1.9 — Formats de fichiers

#### US-300 — Paquet OPC
*En tant que* développeur, *je veux* lire et écrire la structure d'un paquet OPC *afin de* gérer docx, dotx et docm.
- [x] `[Content_Types].xml`, relations (`_rels`), parties et URI relatives, propriétés `core.xml` et `app.xml`.
- [x] Toutes les parties, y compris inconnues, conservées en mémoire avec leur type et leurs relations.

Réf : §5.1 · Taille : M · Dépend de : US-022, US-023, US-030

Validation US-300 (2026-10-12) : [contrat et preuves](docs/us-300-opc-package.md), [validation intégrée](_bmad-output/implementation-artifacts/styles-formats-raster-validation.json).

#### US-301 — Lecture docx : texte, propriétés et styles
*En tant qu'*utilisateur, *je veux* ouvrir mes fichiers Word *afin de* retrouver leur contenu et leur mise en forme.
- [x] `document.xml` : paragraphes, runs, propriétés de caractère et de paragraphe, sauts, tabulations, symboles.
- [x] `styles.xml` (y compris styles latents), `theme1.xml` (couleurs et polices de thème), `settings.xml` (taquet par défaut, options de compatibilité utiles à la mise en page), `fontTable.xml`.
- [x] Polices incorporées (obfuscation ODTTF) chargées.

Validation API : [contrat et limites US-301](docs/us-301-docx-import.md), fixture indépendante Python, désobfuscation exacte de Carlito et import vers le moteur de lignes. Les parties inconnues restent conservées; les propriétés non représentées sont signalées. [Revue et correctifs](_bmad-output/implementation-artifacts/docx-rich-layout-review.md), [validation](_bmad-output/implementation-artifacts/docx-rich-layout-validation.json). Le [raccordement à l’interface en lecture seule](docs/rich-docx-preview.md) est livré : ouvrir DOCX, consulter les pages A4, revenir au TXT intact. L’édition et l’export sémantique restent à développer.

Réf : F-COM-002, §5 · Taille : L · Dépend de : US-300, US-201

#### US-302 — Lecture docx : numérotation
*En tant qu'*utilisateur, *je veux* que mes listes s'ouvrent correctement *afin de* garder la numérotation.
- [ ] `numbering.xml` : `abstractNum`, `num`, surcharges de niveaux, redémarrages, liaison aux styles ; numéros calculés identiques à Word sur le corpus.

Réf : F-TXT-023 · Taille : M · Dépend de : US-301, US-253

#### US-303 — Lecture docx : tableaux
*En tant qu'*utilisateur, *je veux* que mes tableaux s'ouvrent correctement *afin de* conserver mes données mises en forme.
- [ ] Grille, largeurs, fusions (`gridSpan`, `vMerge`), propriétés de tableau, ligne et cellule, styles de tableau conditionnels.

Avancement US-303 (2026-10-13) : lecture livrée dans `atelier-fmt-ooxml`, relue par un agent indépendant et corrigée ; [contrat](docs/us-303-docx-tables.md). Reste ouvert : aucune vérification contre Word, ordre de priorité des styles conditionnels non confirmé, propriétés rangées hors du modèle riche.

- [ ] Tableaux imbriqués importés : lecture, affichage via US-261 et préservation à l'enregistrement via US-306/307. Leur contenu reste en lecture seule en V1 ; aucune création ou édition de tableau imbriqué avant V3 (F-TXT-031).

Réf : F-TXT-030 · Taille : M · Dépend de : US-301, US-261

#### US-304 — Lecture docx : sections, en-têtes, champs, liens
*En tant qu'*utilisateur, *je veux* retrouver la mise en page de mes documents Word *afin de* les imprimer tels quels.
- [ ] `sectPr` (format, marges, orientation, colonnes préservées), en-têtes et pieds (par défaut, première page, pairs).
- [ ] Champs simples et complexes (`fldSimple`, `fldChar`/`instrText`), liens hypertexte, signets.

Réf : F-TXT-050 à F-TXT-053, F-TXT-034, F-TXT-035 · Taille : M · Dépend de : US-301, US-282, US-284

#### US-305 — Lecture docx : images et objets
*En tant qu'*utilisateur, *je veux* retrouver les images de mes documents Word *afin que* rien ne manque.
- [ ] DrawingML `wp:inline` et `wp:anchor` (taille, position, habillage, rognage préservé), images liées signalées.
- [ ] `mc:AlternateContent` : choix de la branche prise en charge, sinon repli ; images VML anciennes affichées.

Réf : F-TXT-032, F-TXT-040 · Taille : M · Dépend de : US-301, US-272

#### US-306 — Préservation des éléments inconnus
*En tant qu'*utilisateur, *je veux* qu'ouvrir et enregistrer un document ne détruise rien *afin de* pouvoir l'éditer sans risque.
- [x] Tout élément ou attribut non géré est rattaché à son nœud (US-032) et réémis à sa place.
- [x] Parties non gérées (commentaires, notes, glossaire, customXml, VBA…) recopiées à l'identique avec leurs relations.
- [x] `mc:Ignorable` respecté ; espaces de noms d'extension conservés.
- [x] Test : sur le corpus, ouvrir puis enregistrer sans modifier → aucune perte d'élément (comparaison XML normalisée).

Avancement US-306 (2026-10-13) : capture livrée dans `atelier-fmt-ooxml` (éléments, attributs et propriétés inconnus rattachés à leur nœud avec position et ordre, parties non interprétées identiques octet pour octet, balisage de compatibilité), relue par un agent indépendant et corrigée ; [contrat](docs/us-306-unknown-preservation.md). La complétude est prouvée par inventaire et reconstruction sur six documents. Aller-retour (US-307 fusionné) vérifié le 2026-10-17 : 67 documents (4 du dépôt, 63 synthétiques à éléments inconnus à chaque niveau), 0 perte en XML normalisé. Reste : commentaires XML et instructions de traitement non préservés dans les parties interprétées ; aucun fichier Word testé.

Réf : §5.2 (1) · Taille : L · Dépend de : US-301, US-032

#### US-307 — Enregistrer en docx
*En tant qu'*utilisateur, *je veux* enregistrer en docx *afin de* partager mes documents avec des utilisateurs de Word.
- [ ] Toutes les parties générées depuis le modèle, sortie déterministe.
- [ ] Les fichiers produits s'ouvrent sans erreur ni réparation dans Word 365 et LibreOffice (corpus de 100 documents).
- [ ] Documents créés de zéro dans Atelier : rendu identique dans Word (comparaison visuelle sur un jeu de documents types).

Réf : F-COM-003, §5 · Taille : L · Dépend de : US-301 à US-306, US-031

#### US-308 — Corpus et tests d'aller-retour
*En tant que* développeur, *je veux* un corpus de documents de référence *afin de* mesurer la fidélité en continu.
- [ ] Au moins 200 documents (créés avec Word et LibreOffice, documents publics de licence compatible), couvrant les fonctions V1.
- [ ] Par document : PDF de référence généré par Word, nombre de pages attendu.
- [ ] Rapport automatique : nombre de pages identique, lignes coupées identiques, écart visuel par page, aller-retour sans perte.

Réf : §8 · Taille : M · Dépend de : US-003, US-307

#### US-309 — Modèles et documents à macros
*En tant qu'*utilisateur, *je veux* ouvrir des modèles et des documents contenant des macros *afin de* les utiliser sans risque.
- [ ] Ouvrir un `.dotx` crée un nouveau document sans titre basé sur le modèle.
- [ ] `.docm` : macros VBA jamais exécutées, conservées si l'on enregistre en `.docm` ; bandeau d'information ; avertissement de perte des macros si l'on enregistre en `.docx`.
- [ ] Enregistrer sous `.dotx` possible.

Réf : §5.1 · Taille : S · Dépend de : US-306

#### US-310 — Ouvrir un fichier ODT
*En tant qu'*utilisateur, *je veux* ouvrir des documents LibreOffice *afin de* les lire et les modifier.
- [x] `content.xml`, `styles.xml`, `meta.xml` : texte, styles (héritage ODF, styles automatiques), listes, tableaux, images, en-têtes et pieds, mise en page, liens, signets, champs page/date.
- [ ] À l'enregistrement : proposition d'enregistrer en docx (ODF non pris en charge en écriture).
- [ ] Rendu conforme à LibreOffice sur un corpus de 50 documents (nombre de pages identique pour ≥ 90 %).

Avancement US-310 (2026-10-13) : import livré dans `atelier-fmt-odf`, relu par un agent indépendant et corrigé ; [contrat](docs/us-310-odt-import.md), [corpus LibreOffice](tests/odt-reference/README.md). Le critère de pagination n'est pas vérifiable avant la mise en page complète ; la proposition d'enregistrer en docx relève de US-322.

Réf : D2, §5.1 · Taille : L · Dépend de : US-022, US-030, US-200

#### US-311 — Conteneur CFB
*En tant que* développeur, *je veux* lire le format Compound File Binary *afin d'ouvrir* les documents Word anciens.
- [x] Versions 3 et 4, FAT, DIFAT, mini-FAT, répertoire, flux ; détection des cycles et des chaînes invalides.

Réf : [MS-CFB] · Taille : M · Dépend de : US-001

Validation US-311 (2026-10-12) : développée, revue et validée ; [contrat](docs/us-311-cfb-container.md), [preuves du lot](_bmad-output/implementation-artifacts/foundation-integration-validation.json).

#### US-312 — Ouvrir un .doc : texte, mise en forme, styles
*En tant qu'*utilisateur, *je veux* ouvrir mes anciens documents Word *afin de* les retrouver et les convertir.
- [ ] FIB, table des pièces (texte compressé ANSI et Unicode), CHPX, PAPX, feuille de styles STSH, polices, propriétés du document.

Avancement US-312 (2026-10-13) : lecture livrée dans `atelier-fmt-legacy`, relue par un agent indépendant et corrigée ; [contrat](docs/us-312-doc-import.md), [corpus](tests/doc-reference/README.md) produit par LibreOffice et non par Word. Reste ouvert : modifications simples de la table des pièces (`Prm0`) non appliquées, Word 6/95 refusé.

Réf : D2, [MS-DOC] · Taille : L · Dépend de : US-311, US-014, US-200

#### US-313 — Ouvrir un .doc : listes, tableaux, sections, en-têtes
*En tant qu'*utilisateur, *je veux* que la structure de mes anciens documents soit conservée *afin de* ne pas avoir à la refaire.
- [ ] Listes (LST, LFO), tableaux (TAP, fusions), sections (SEPX), en-têtes et pieds, champs, liens, signets.

Avancement US-313 (2026-10-13) : structure livrée dans `atelier-fmt-legacy`, relue par un agent indépendant et corrigée ; [contrat](docs/us-313-doc-structure.md), corpus LibreOffice de cinq fichiers de structure. Reste ouvert : fusion horizontale, sauts de section particuliers et en-têtes pairs prouvés seulement sur fixtures construites ; aucun fichier Word contenant un tableau n'a été testé ; un signet sur plusieurs paragraphes est tronqué.

Réf : D2, [MS-DOC] · Taille : L · Dépend de : US-312

#### US-314 — Ouvrir un .doc : images
*En tant qu'*utilisateur, *je veux* retrouver les images de mes anciens documents *afin que* rien ne manque.
- [ ] Images en ligne (PICF) et flottantes (OfficeArt) aux formats PNG, JPEG, BMP ; EMF/WMF préservés avec cadre de remplacement (rendu en V3).

Réf : D2, [MS-DOC] · Taille : M · Dépend de : US-313, US-270

#### US-315 — Documents chiffrés
*En tant qu'*utilisateur, *je veux* un message clair à l'ouverture d'un document protégé par mot de passe *afin de* comprendre pourquoi il ne s'ouvre pas.
- [x] Détection des docx chiffrés (conteneur CFB `EncryptionInfo`) et des doc chiffrés.
- [ ] Message : « Les documents protégés par mot de passe seront pris en charge dans une prochaine version. »

Avancement US-315 (2026-10-13) : détection livrée dans `atelier_fmt_legacy::detect`, relue par un agent indépendant et corrigée ; [contrat](docs/us-315-encrypted-detection.md), corpus de fichiers chiffrés produits par LibreOffice. Reste ouvert : afficher le message (clé proposée `encrypted-document`) en appelant la détection avant l'import docx. Aucun fichier chiffré par Microsoft Office n'a été testé.

Réf : F-TXT-065 · Taille : S · Dépend de : US-311

#### US-316 — Fichiers texte
*En tant qu'*utilisateur, *je veux* ouvrir et enregistrer des fichiers texte *afin d'éditer* des notes simples.
- [ ] Ouverture avec détection de l'encodage (US-014), choix manuel possible ; un paragraphe par ligne.
- [ ] Enregistrement en UTF-8 (défaut, sans BOM), UTF-16 ou Windows-1252 ; fins de ligne CRLF ou LF ; avertissement de perte de mise en forme.

Avancement US-316 (2026-10-13) : bibliothèque livrée dans `atelier_text_model::plain_io` (détection et choix de l'encodage, UTF-8, UTF-16, Windows-1252, CRLF ou LF, aller-retour exact, rapport de perte de mise en forme), relue par un agent indépendant et corrigée ; [contrat](docs/us-316-text-files.md). Reste ouvert : dialogue de choix d'encodage quand la détection est incertaine, options d'enregistrement et message d'avertissement dans l'éditeur.

Réf : §5.1 · Taille : S · Dépend de : US-014, US-200

#### US-317 — Fichiers endommagés
*En tant qu'*utilisateur, *je veux* récupérer le maximum d'un fichier endommagé *afin de* ne pas tout perdre.
- [ ] Fichier partiellement corrompu : ouverture de ce qui est lisible, message listant ce qui n'a pas pu être lu.
- [ ] Aucun plantage : tous les lecteurs (docx, odt, doc, txt) sont sous fuzzing continu.

Réf : §6.13 · Taille : M · Dépend de : US-004, US-307, US-310, US-314

## E1.10 — Fichiers et application

#### US-320 — Onglet Fichier et accueil
*En tant qu'*utilisateur, *je veux* un écran d'accueil *afin de* créer ou rouvrir rapidement un document.
- [ ] Nouveau : document vierge et modèles fournis (lettre, CV, rapport, compte rendu).
- [ ] Récents : liste avec chemin et date, épingler, retirer, fichier introuvable signalé.
- [ ] Ouvrir, Enregistrer, Enregistrer sous, Exporter en PDF, Imprimer, Propriétés, Options, À propos.

Réf : F-COM-001, F-COM-004 · Taille : M · Dépend de : US-106, US-113

#### US-321 — Ouvrir un document
*En tant qu'*utilisateur, *je veux* ouvrir un document de plusieurs façons *afin de* travailler comme j'en ai l'habitude.
- [ ] Ctrl+O (dialogue natif filtré : tous les documents pris en charge, docx, odt, doc, txt), glisser-déposer d'un fichier sur la fenêtre, argument de ligne de commande / « Ouvrir avec ».
- [ ] Document déjà ouvert → bascule vers son onglet.
- [ ] Indicateur de progression pour les gros fichiers ; ouverture annulable.

Réf : F-COM-002 · Taille : S · Dépend de : US-113, US-115, US-307

#### US-322 — Enregistrer un document
*En tant qu'*utilisateur, *je veux* enregistrer mon travail *afin de* le conserver.
- [ ] Ctrl+S ; premier enregistrement → Enregistrer sous ; F12 Enregistrer sous (docx, docm, dotx, txt).
- [ ] Indicateur « modifié » dans l'onglet et le titre de la fenêtre.
- [ ] À la fermeture d'un document modifié : Enregistrer / Ne pas enregistrer / Annuler ; fermeture de l'application avec plusieurs documents modifiés gérée.
- [ ] Document ouvert depuis un format en lecture seule (odt, doc) → Enregistrer propose docx.

Réf : F-COM-003 · Taille : M · Dépend de : US-307, US-323

#### US-323 — Enregistrement atomique
*En tant qu'*utilisateur, *je veux* que mon fichier ne soit jamais corrompu à l'enregistrement *afin de* ne jamais perdre de travail.
- [ ] Écriture dans un fichier temporaire du même dossier, vidage sur disque, puis publication atomique : `ReplaceFileW` pour une destination existante (attributs et droits conservés), renommage atomique pour une nouvelle destination sans écraser un fichier apparu entre-temps.
- [ ] Disque plein, droits insuffisants, clé USB retirée : l'original reste intact, message clair, le document reste ouvert et modifié.
- [ ] Test : interruption simulée à chaque étape → ancienne version complète avant publication ou nouvelle version complète après publication, jamais de fichier partiel. Pour une nouvelle destination : fichier absent ou complet. L'original n'est jamais tronqué.

Réf : F-COM-005, §6.13 · Taille : S · Dépend de : US-113

#### US-324 — Fichier verrou
*En tant qu'*utilisateur, *je veux* être averti si un document est déjà ouvert ailleurs *afin d'éviter* les conflits.
- [ ] Fichier `~$nom.docx` au format de Word (nom de l'utilisateur), caché, créé à l'ouverture et supprimé à la fermeture.
- [ ] Verrou présent (Atelier ou Word) : choix lecture seule / ouvrir une copie / ouvrir quand même ; verrou orphelin (> 24 h ou processus absent) ignoré proprement.
- [ ] Nom d'utilisateur réglable dans les options (défaut : nom de session Windows).

Réf : F-COM-007 · Taille : S · Dépend de : US-113

#### US-325 — Récupération automatique
*En tant qu'*utilisateur, *je veux* récupérer mon travail jusqu'à la dernière sauvegarde de récupération après un plantage ou une coupure *afin de* limiter la perte de modifications.
- [ ] À la première modification, création d'un point de récupération autonome selon US-074 dans `atelier-data/recovery/` ; ensuite, persistance des transactions validées toutes les 2 minutes (paramétrable) et à chaque mise en veille. Une erreur de persistance est signalée ; les modifications postérieures au dernier point durable peuvent être perdues lors d'un arrêt brutal.
- [ ] Après enregistrement réussi ou rechargement du fichier source, la nouvelle génération (instantané + journal cohérent) est publiée atomiquement avant suppression de l'ancienne. Le rejeu ne dépend ni de l'existence ni du contenu courant du fichier source.
- [ ] Au redémarrage après un arrêt anormal : volet listant les documents récupérables, aperçu, restaurer ou abandonner.
- [ ] Une restauration ouvre un document distinct ; l'enregistrement exige un choix explicite de destination et n'écrase pas automatiquement le fichier source. Données supprimées après enregistrement réussi et fermeture normale, ou après abandon explicite des modifications.
- [ ] Tests de reprise : document sans fichier, source modifiée ou supprimée, arrêt pendant publication d'une génération et fin de journal tronquée. Le dernier point durable reste récupérable sans transaction partielle.

Réf : F-COM-006 · Taille : M · Dépend de : US-074, US-113

#### US-326 — Modification externe
*En tant qu'*utilisateur, *je veux* savoir si le fichier a été modifié par un autre programme *afin de* ne pas écraser ces modifications.
- [ ] Surveillance du fichier ouvert ; bandeau : « Ce fichier a été modifié en dehors d'Atelier » → Recharger / Garder ma version.
- [ ] Fichier supprimé ou renommé : bandeau et Enregistrer sous proposé.

Réf : F-COM-008 · Taille : S · Dépend de : US-113

#### US-327 — Plusieurs documents
*En tant qu'*utilisateur, *je veux* ouvrir plusieurs documents *afin de* travailler sur plusieurs fichiers à la fois.
- [ ] Onglets de documents dans la fenêtre : Ctrl+Tab / Ctrl+Maj+Tab, Ctrl+W pour fermer, réorganisation par glisser.
- [ ] Chaque document a son historique d'annulation, sa vue et son zoom.
- [ ] Copier-coller entre documents sans perte (format interne).

Réf : F-COM-023 · Taille : M · Dépend de : US-092, US-320

#### US-328 — Propriétés du document
*En tant qu'*utilisateur, *je veux* renseigner les propriétés du document *afin de* le décrire et le retrouver.
- [ ] Titre, sujet, auteur, mots-clés, commentaires, catégorie ; dates et statistiques en lecture seule.
- [ ] Lues et écrites dans `core.xml` / `app.xml` ; reprises dans les métadonnées du PDF exporté.

Avancement US-328 (2026-10-13) : bibliothèque livrée dans `atelier_fmt_ooxml` (propriétés de base et étendues, dates W3CDTF, modification, mise à jour à l'enregistrement, conversion vers les métadonnées PDF, aller-retour sans modification identique octet pour octet), relue par un agent indépendant et corrigée ; [contrat](docs/us-328-document-properties.md). LibreOffice relit les paquets tels qu'ils sont écrits. Reste ouvert : dialogue Propriétés, appel à l'enregistrement et passage des métadonnées à l'export PDF. Aucun fichier Word testé.

Réf : F-COM-017 · Taille : S · Dépend de : US-300

#### US-329 — Options
*En tant qu'*utilisateur, *je veux* régler l'application *afin de* l'adapter à mes habitudes.
- [ ] Général : nom d'utilisateur, initiales, thème, unité de mesure (cm, mm, pt, pouces).
- [ ] Enregistrement : format par défaut, intervalle de récupération, dossier par défaut.
- [ ] Vérification : options de correction automatique et d'espaces insécables.
- [ ] Avancé : marques de mise en forme affichées, mode refrappe, nombre de documents récents.
- [ ] Stockage dans `atelier-data/config.ini` ; fichier corrompu → valeurs par défaut avec message.

Réf : F-COM-022 · Taille : M · Dépend de : US-116, US-104

## E1.11 — Impression et export PDF

#### US-330 — Sous-ensemble de polices
*En tant que* développeur, *je veux* extraire un sous-ensemble de police *afin d'incorporer* uniquement les glyphes utilisés dans le PDF.
- [x] Sous-ensembles TrueType (`glyf`/`loca`, composites inclus) et CFF ; tables minimales reconstruites.
- [x] Respect des droits d'incorporation (`OS/2 fsType`) : police non incorporable → repli avec avertissement.

Validation US-330 (2026-10-13) : développée, relue par un agent indépendant et corrigée ; [contrat](docs/us-330-font-subsetting.md). Polices CID et glyphes `seac` vérifiés sur fixtures synthétiques uniquement.

Réf : §6.12 · Taille : M · Dépend de : US-051, US-052

#### US-331 — Écrivain PDF
*En tant que* développeur, *je veux* générer des fichiers PDF *afin d'exporter* les documents.
- [x] PDF 1.7 : table xref, flux compressés, pages, polices CIDFontType2 / CFF incorporées avec `ToUnicode`, images (JPEG tel quel, PNG en Flate avec masque alpha), chemins, couleurs RVB, transparence, clips.
- [x] Rendu depuis la liste d'affichage (US-080).
- [ ] Les PDF s'ouvrent sans erreur dans Acrobat Reader, Edge et Firefox ; le texte est sélectionnable, copiable et cherchable (accents corrects).

Avancement US-331 (2026-10-13) : écrivain livré dans `atelier-fmt-pdf`, relu par un agent indépendant (conforme) ; [contrat](docs/us-331-pdf-writer.md). Vérifié par PyMuPDF, poppler, fontTools, LibreOffice et un contrôle octet par octet du xref ; affichage constaté dans Firefox. Reste à constater à la main : ouverture dans Acrobat et Edge, sélection, copie et recherche de texte (procédure en fin de contrat).

Réf : §6.12 · Taille : L · Dépend de : US-080, US-330, US-021

#### US-332 — Exporter en PDF
*En tant qu'*utilisateur, *je veux* exporter mon document en PDF *afin de* le partager sous une forme non modifiable.
- [ ] Fichier → Exporter en PDF : choix des pages, qualité des images (originale / compressée), ouvrir le fichier après export.
- [ ] Liens externes et internes actifs, signets générés depuis les titres, métadonnées du document.
- [ ] Rendu identique à l'écran (comparaison raster sur le corpus).
- [ ] 100 pages exportées en < 3 s.

Réf : F-COM-014 · Taille : M · Dépend de : US-331, US-328

#### US-333 — Imprimer
*En tant qu'*utilisateur, *je veux* imprimer mon document *afin d'en* avoir une copie papier.
- [ ] Ctrl+P : aperçu des pages, choix de l'imprimante, pages (toutes, courante, plage), copies, recto-verso et orientation selon le pilote.
- [ ] Vue d'impression construite en SVG vectoriel (US-081), envoyée via l'API d'impression de WebView2.
- [ ] Impression fidèle à l'écran (texte vectoriel, images à pleine résolution).

Réf : F-COM-013 · Taille : M · Dépend de : US-081, US-111

## E1.12 — Navigation et outils

#### US-340 — Rechercher
*En tant qu'*utilisateur, *je veux* rechercher du texte *afin de* trouver rapidement un passage.
- [ ] Ctrl+F : volet de recherche, résultats listés avec leur contexte, toutes les occurrences surlignées, compteur.
- [x] Options : respecter la casse, mot entier, ignorer les accents, respecter les espaces insécables.
- [ ] Recherche dans les en-têtes, pieds et tableaux ; Entrée / Maj+Entrée pour suivant/précédent.

Avancement (2026-10-13) : Volet Ctrl+F raccordé au TXT avec contexte, compteur, quatre options Unicode, surlignage et Entrée/Maj+Entrée. Liste plafonnée à 1 000 occurrences avec avertissement : le critère « toutes les occurrences » reste partiel au-delà. Le raccordement aux en-têtes, pieds et tableaux du DOCX reste ouvert ; [livraison et validation](docs/us-340-343-navigation-ui.md).

Réf : F-COM-010 · Taille : M · Dépend de : US-013, US-200

#### US-341 — Remplacer
*En tant qu'*utilisateur, *je veux* remplacer du texte *afin de* corriger un document en une fois.
- [x] Ctrl+H : suivant, remplacer, remplacer tout (une seule transaction annulable, nombre de remplacements affiché).
- [ ] Le texte de remplacement prend la mise en forme du texte trouvé.

Avancement (2026-10-13) : Remplacement TXT intégré, quatre options Unicode, compte exact au-delà des 1 000 résultats affichés, transaction isolée Undo/Redo, refus atomique des plages/révisions périmées et dépassements. Édition et attributs riches restent ouverts ; [contrat et validation](docs/us-341-replacement.md).

Réf : F-COM-010 · Taille : S · Dépend de : US-340

#### US-342 — Zoom
*En tant qu'*utilisateur, *je veux* zoomer *afin de* lire confortablement.
- [x] Curseur et pourcentage dans la barre d'état, Ctrl+molette, dialogue (10–500 %, largeur de page, page entière, plusieurs pages).
- [ ] Le zoom conserve la position de lecture ; mémorisé par document pendant la session.

Avancement (2026-10-13) : Zoom intégré au texte et à l’aperçu DOCX, avec deux pages verticales, ancrage de lecture et modes ajustés au conteneur. La vue texte garde son zoom pendant un aller-retour DOCX. Mémorisation après fermeture/réouverture d’un fichier encore ouverte : identité absente du pont natif ; [contrat et validation](docs/us-342-zoom.md).

Réf : F-COM-012 · Taille : S · Dépend de : US-101

#### US-343 — Statistiques
*En tant qu'*utilisateur, *je veux* connaître le nombre de mots *afin de* respecter une longueur imposée.
- [ ] Barre d'état : mots (de la sélection et du document) ; dialogue : pages, mots, caractères avec et sans espaces, paragraphes, lignes ; option notes et zones de texte.
- [ ] Comptage identique à Word sur le corpus (tolérance ≤ 0,5 %).

Avancement (2026-10-13) : Compteur document/sélection et dialogue à six mesures raccordés au TXT, avec pages/lignes issues de la mise en page Rust. Notes/zones de texte désactivées et expliquées ; ces fonctions et la preuve de parité Word restent ouvertes ; [livraison et validation](docs/us-340-343-navigation-ui.md).

Réf : F-TXT-006 · Taille : S · Dépend de : US-011

#### US-344 — Barre d'état
*En tant qu'*utilisateur, *je veux* une barre d'état informative *afin de* connaître ma position et l'état du document.
- [ ] Page X sur Y (clic → Atteindre), mots, langue, mode d'affichage, zoom, indicateur d'enregistrement.
- [x] Éléments affichés configurables (clic droit).

Avancement (2026-10-13) : Pages visibles et dialogue Atteindre raccordés au TXT et à l’aperçu, sans modifier la sélection ; six champs masquables par clic droit/Maj+F10 pendant la session. La langue du TXT est explicitement non définie ; mots et métadonnées riches du DOCX restent indisponibles, donc premier critère partiel ; [contrat et validation](docs/us-344-status-bar.md).

Réf : F-COM-018 · Taille : S · Dépend de : US-342, US-343

## E1.13 — Accessibilité

#### US-350 — Lire et parcourir le document avec un lecteur d'écran
*En tant qu'*utilisateur de lecteur d'écran, *je veux* lire et parcourir le document *afin de* l'éditer de façon autonome.
- [ ] Miroir accessible caché (rôle document, titres avec niveau, paragraphes, listes, tableaux avec en-têtes, images avec texte alternatif, liens) couvrant au minimum les pages visibles et le voisinage du curseur.
- [ ] NVDA et le Narrateur lisent caractère, mot, ligne et paragraphe lors des déplacements, et annoncent la sélection.
- [ ] Navigation rapide par titres, tableaux et liens avec NVDA.

Réf : F-COM-021, §6.14 · Taille : L · Dépend de : US-214, US-101

#### US-351 — Annonces de mise en forme et de contexte
*En tant qu'*utilisateur de lecteur d'écran, *je veux* être informé de la mise en forme et des changements *afin de* savoir ce que je fais.
- [ ] Annonces lors de l'application d'une mise en forme (« Gras activé »), du passage d'un style ou d'une cellule de tableau à l'autre, des résultats de recherche, des notifications (enregistrement, erreurs).
- [ ] Raccourci pour lire la mise en forme courante (police, taille, style).

Réf : F-COM-021, §6.14 · Taille : M · Dépend de : US-350

#### US-352 — Interface entièrement au clavier
*En tant qu'*utilisateur sans souris, *je veux* piloter toute l'interface au clavier *afin de* travailler efficacement.
- [ ] F6 / Maj+F6 parcourent les zones (ruban, document, volets, barre d'état) ; Échap ramène au document.
- [ ] Info-touches Alt sur tout le ruban et l'onglet Fichier ; dialogues parcourables à la tabulation, focus piégé dans les modales.
- [ ] Aucune fonction V1 accessible uniquement à la souris (redimensionnement d'image et de tableau compris, via dialogues).

Réf : F-COM-021 · Taille : M · Dépend de : US-106, US-104

#### US-353 — Contraste et agrandissement
*En tant qu'*utilisateur malvoyant, *je veux* une interface lisible en contraste élevé et agrandie *afin de* l'utiliser confortablement.
- [ ] Mode contraste élevé Windows respecté par toute l'interface.
- [ ] Interface utilisable à 200 % de mise à l'échelle Windows sans perte de fonction ; contrastes AA vérifiés pour les thèmes clair et sombre.

Réf : F-COM-019, §6.14 · Taille : S · Dépend de : US-105

#### US-354 — Audit RGAA de la V1
*En tant qu'*éditeur du logiciel, *je veux* un audit RGAA *afin de* garantir et déclarer le niveau d'accessibilité.
- [ ] Grille RGAA 4.1 complétée pour l'interface ; tests manuels NVDA et Narrateur sur des scénarios types (créer, mettre en forme, insérer un tableau, enregistrer, exporter).
- [ ] Défauts bloquants corrigés ; déclaration d'accessibilité publiée avec la version.

Réf : D13, §6.14, §8 · Taille : M · Dépend de : US-350 à US-353

## E1.14 — Distribution

#### US-360 — Build de release portable
*En tant qu'*utilisateur, *je veux* un exécutable unique à télécharger *afin de* lancer l'application sans installation.
- [ ] `atelier.exe` unique (< 30 Mo) contenant WASM, front-end et polices ; icône, informations de version, manifeste (DPI, chemins longs, page de code UTF-8).
- [ ] Archive de distribution : exe + `LICENSE` + notices tierces (polices, données Unicode).
- [ ] Fonctionne sur Windows 10 22H2 et Windows 11 (x64) avec le runtime WebView2 présent.

Réf : D6, §7.2 · Taille : S · Dépend de : US-112, US-116

#### US-361 — À propos
*En tant qu'*utilisateur, *je veux* connaître la version et la licence *afin de* savoir ce que j'utilise.
- [x] Version, date de build, licence AGPL-3.0 (texte complet consultable), adresse du code source, licences des polices et des données Unicode.

Réf : §9 · Taille : S · Dépend de : US-320

Avancement US-361 (2026-10-14) : US-361 est validée. La consultation fonctionne aussi pendant un aperçu DOCX sans changer le document. [Contrat et validation](docs/us-361-editor-utilities.md).


#### US-362 — Validation des performances
*En tant qu'*utilisateur, *je veux* une application rapide *afin de* travailler sans attendre.
- [ ] Mesures sur la machine de référence pour chaque cible du §6.16 applicable à V1 (démarrage, ouverture de 100 pages, latence de frappe sur 300 pages, défilement, mémoire, taille de l'exe).
- [ ] Rapport joint à la version ; tout objectif manqué fait l'objet d'une story corrective.

Réf : §6.16 · Taille : M · Dépend de : US-005, US-213, US-307

#### US-363 — Gestion des plantages
*En tant qu'*utilisateur, *je veux* retrouver le dernier point de récupération valide après un plantage *afin de* limiter la perte de travail.
- [ ] Panique du moteur ou erreur fatale interceptable : tentative de persistance immédiate si l'état est encore accessible, rapport technique local (`atelier-data/crash/`) et message proposant de redémarrer. Si cette tentative échoue, le dernier point durable reste utilisable.
- [ ] Arrêt forcé, coupure de courant ou état du moteur inaccessible : aucune écriture finale n'est garantie ; la récupération suit US-325, avec perte possible des modifications postérieures au dernier point durable.
- [ ] Au redémarrage : récupération proposée (US-325) et accès au rapport (aucun envoi automatique).

Réf : §6.13, §11 (7) · Taille : S · Dépend de : US-325

#### US-364 — Garantie « aucun appel réseau »
*En tant qu'*utilisateur, *je veux* la garantie que l'application n'envoie rien sur Internet *afin de* protéger ma vie privée.
- [ ] Test automatisé : un scénario complet (ouvrir, éditer, enregistrer, exporter, imprimer) ne déclenche aucune requête hors de `app.atelier` (journal US-112).
- [ ] Revue du code natif : aucun appel aux API réseau Windows.
- [ ] Les seules sorties sont l'ouverture d'un lien dans le navigateur par l'utilisateur, après confirmation.

Réf : §1, D11 · Taille : S · Dépend de : US-112

---

## Ordre de réalisation suggéré

Chaque étape commence après ses prérequis des étapes précédentes. Au sein d'une étape, l'ordre des dépendances déclarées fait foi ; seules les stories indépendantes peuvent être développées en parallèle. Les plages ci-dessous ne contiennent que des IDs existants.

| Étape | Stories | Résultat vérifiable |
|---|---|---|
| 1 | US-001, US-002, US-004, US-005, US-010 → 014, US-020 → 023, US-030 → 032 | Ouvrir un docx et en lister le XML (outil en ligne de commande) |
| 2 | US-040 → 043, US-050 → 056, US-060, US-061 | Rendre un texte et une image en PNG en ligne de commande |
| 3 | US-070 → 074, US-080 → 082, US-003 | Modèle générique annulable, rendu SVG/PNG |
| 4 | US-090 → 092, US-100 → 106, US-110 → 116 | Application Windows vide avec ruban |
| 5 | US-120 | **Fin de V0** : éditeur de texte brut paginé |
| 6 | US-200 → 202, US-210 → 215, US-220 → 224, US-226 → 231 | Éditeur riche minimal ; le presse-papiers riche complet arrive à l'étape 9 |
| 7 | US-300, US-301, US-328 | Import DOCX : texte, styles et propriétés ; l'enregistrement complet arrive à l'étape 10 |
| 8 | US-240 → 248, US-250 → 254, US-302 | Mise en forme, styles, listes |
| 9 | US-260 → 266, US-303, US-270 → 273, US-305, US-225 | Tableaux, images et presse-papiers riche |
| 10 | US-280 → 285, US-290, US-291, US-304, US-306, US-274, US-307, US-323, US-322 | Mise en page complète ; ouvrir et enregistrer un DOCX V1 avec préservation de l'inconnu |
| 11 | US-330 → 333 | Export PDF et impression |
| 12 | US-308, US-309, US-310 → 317 | Corpus, ODT, DOC, robustesse |
| 13 | US-320, US-321, US-324 → 327, US-329, US-340 → 344 | Application complète |
| 14 | US-350 → 354, US-360 → 364 | **Fin de V1** : version publiable |

## Traçabilité exigences → stories (V1)

| Exigence | Stories |
|---|---|
| F-COM-001 / 004 | US-320 |
| F-COM-002 | US-115, US-321 |
| F-COM-003 | US-307, US-322 |
| F-COM-005 | US-323 |
| F-COM-006 | US-074, US-325 |
| F-COM-007 | US-324 |
| F-COM-008 | US-326 |
| F-COM-009 | US-072, US-226 |
| F-COM-010 | US-340, US-341 |
| F-COM-012 | US-342 |
| F-COM-013 | US-333 |
| F-COM-014 | US-330, US-331, US-332 |
| F-COM-016 | US-225 |
| F-COM-017 | US-328 |
| F-COM-018 | US-106, US-230, US-344 |
| F-COM-019 | US-105, US-353 |
| F-COM-020 | US-231 |
| F-COM-021 | US-350, US-351, US-352, US-354 |
| F-COM-022 | US-116, US-329 |
| F-COM-023 | US-327 |
| F-TXT-001 | US-103, US-220 |
| F-TXT-002 | US-221, US-222, US-223 |
| F-TXT-003 | US-224 |
| F-TXT-004 | US-227, US-228 |
| F-TXT-005 | US-229 |
| F-TXT-006 | US-343 |
| F-TXT-010 | US-240, US-241, US-242 |
| F-TXT-011 | US-211, US-243, US-244, US-245 |
| F-TXT-012 | US-212, US-215, US-244, US-246 |
| F-TXT-013 | US-247 |
| F-TXT-014 | US-248 |
| F-TXT-020 | US-201, US-250 |
| F-TXT-021 | US-202 |
| F-TXT-022 | US-251 |
| F-TXT-023 | US-252, US-253, US-302 |
| F-TXT-024 | US-254 |
| F-TXT-030 | US-260 → 266, US-303 |
| F-TXT-032 | US-270, US-271, US-272, US-305 |
| F-TXT-034 | US-290, US-291 |
| F-TXT-035 | US-284 |
| F-TXT-040 | US-274, US-305, US-306 |
| F-TXT-050 | US-280 |
| F-TXT-051 | US-281 |
| F-TXT-052 | US-282 |
| F-TXT-053 | US-283 |
| F-TXT-056 | US-245, US-285 |
