# Graph Report - mon-bureau  (2026-10-10)

## Corpus Check
- Corpus is ~41,887 words - fits in a single context window. You may not need a graph.

## Summary
- 670 nodes · 1471 edges · 98 communities (28 shown, 38 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS · INFERRED: 7 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- geom/src/lib.rs / Point
- IndexedTree<T> / model/src/lib.rs
- US-001 — Initialiser le workspace / US-200 — Modèle de document texte
- US-212 — Pagination / US-304 — Lecture docx : sections, en-têtes, champs, liens
- bench/src/lib.rs / String
- check-workspace/src/main.rs / String
- check-workspace/tests/cli.rs / .new()
- unicode/src/lib.rs / encode()
- US-113 — Pont natif ↔ JS / §6.8 — Shell desktop Windows
- check-license/src/main.rs / check_tree()
- US-104 — Bibliothèque de composants d'interface / US-106 — Ruban
- bench/src/main.rs / run()
- SPEC.md v0.2 / §5.2 — Principes de fidélité
- §6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout
- bench/tests/cli.rs / stdout_csv()
- bench_indexed_tree.rs / measure()
- US-220 — Saisir du texte / US-072 — Pile annuler / rétablir
- §6.6 — Rendu / US-060 — Géométrie 2D
- US-201 — Moteur de styles / US-250 — Appliquer un style
- US-240 — Mettre en forme les caractères depuis le ruban / F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères
- US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)
- Point / Path
- tables.rs / CP437
- US-331 — Écrivain PDF / US-330 — Sous-ensemble de polices
- US-342 — Zoom / US-343 — Statistiques
- Encoding Enum / decode
- IndexedTree<T> / bench_indexed_tree
- US-340 — Rechercher / F-COM-010 — Rechercher / remplacer (texte, casse, mot entier)
- atelier-geom / GeometryError
- atelier-base
- atelier-bench
- atelier-calc
- atelier-cfb
- atelier-check-license
- atelier-check-workspace
- atelier-compress
- atelier-core-wasm
- atelier-crypto
- atelier-desktop-win
- atelier-fmt-legacy
- atelier-fmt-markdown
- atelier-fmt-odf
- atelier-fmt-ooxml
- atelier-fmt-pdf
- atelier-font
- atelier-geom
- atelier-image
- atelier-lua
- atelier-model
- atelier-raster
- atelier-render
- atelier-server
- atelier-shape
- atelier-sheet-model
- atelier-slide-model
- atelier-text-layout
- atelier-text-model
- atelier-unicode
- atelier-xml
- atelier-zip
- CI Workflow
- Technical Architecture
- Atelier Vision
- Transaction Journal
- Atomic Save
- Accessibility Mirror

## God Nodes (most connected - your core abstractions)
1. `SPEC.md v0.2` - 80 edges
2. `Point` - 34 edges
3. `GeometryError` - 30 edges
4. `IndexedTree<T>` - 25 edges
5. `US-001 — Initialiser le workspace` - 18 edges
6. `Rect` - 17 edges
7. `US-200 — Modèle de document texte` - 16 edges
8. `US-307 — Enregistrer en docx` - 14 edges
9. `run()` - 13 edges
10. `Node` - 12 edges

## Surprising Connections (you probably didn't know these)
- `US-001 — Initialiser le workspace` --references--> `Zero Dependency Rule (D11)`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `US-120 — Démonstrateur de bout en bout` --references--> `§6 — Architecture technique`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `check_tree()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs
- `create_symlink()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs
- `visit()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Text Encoding API** — docs_us_014_encodings_encoding, docs_us_014_encodings_detect_encoding, docs_us_014_encodings_decode, docs_us_014_encodings_encode [EXTRACTED 1.00]
- **Geometry Primitives** — docs_us_060_geometry_point, docs_us_060_geometry_rect, docs_us_060_geometry_path, docs_us_060_geometry_affinetransform [EXTRACTED 1.00]

## Communities (98 total, 38 thin omitted)

### Community 0 - "geom/src/lib.rs / Point"
Cohesion: 0.08
Nodes (44): affine_composition_order_and_inverse_are_explicit(), AffineTransform, assert_flat_curve(), checked(), close_then_line_quadratic_or_cubic_starts_a_new_open_subpath(), coordinates(), coordinates_cubic(), cubic_derivative_roots() (+36 more)

### Community 1 - "IndexedTree<T> / model/src/lib.rs"
Cohesion: 0.09
Nodes (34): ArenaSlot, assert_matches(), deleting_a_two_child_node_preserves_successor_identity(), empty_page(), identity_exhaustion_is_atomic(), IndexedTree, IndexedTree<T>, IndexedTreeError (+26 more)

### Community 2 - "US-001 — Initialiser le workspace / US-200 — Modèle de document texte"
Cohesion: 0.06
Nodes (69): §11 — Points ouverts (non bloquants pour la validation), §5 — Formats de fichiers, §5.1 — Matrice, §6.13 — Robustesse et protection des données, §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe), §6.3 — Modèle de document et opérations, §8 — Qualité et tests, D2 — Format natif OOXML (docx/xlsx/pptx). Lecture des formats binaires anciens (doc/xls/ppt) et ODF (odt/ods/odp). (+61 more)

### Community 3 - "US-212 — Pagination / US-304 — Lecture docx : sections, en-têtes, champs, liens"
Cohesion: 0.08
Nodes (44): §6.16 — Objectifs de performance (machine de référence : PC grand public de 2020, 8 Go), §6.5 — Mise en page (traitement de texte), F-TXT-002 — Sélection souris (clic, double-clic mot, triple-clic paragraphe, glisser, Maj+clic) et clavier (Maj+flèches, Ctrl+Maj+flèches…), F-TXT-003 — Glisser-déposer de texte dans le document, F-TXT-011 — Paragraphe : alignement (gauche, centré, droite, justifié), retraits, première ligne, interligne, espacement avant/après, tabulations (gauche, centre, droite, décimale, points de suite), F-TXT-012 — Paragraphe : bordures, trame de fond, veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant, F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule), F-TXT-035 — Champs : PAGE, NUMPAGES, DATE, TIME (+36 more)

### Community 4 - "bench/src/lib.rs / String"
Cohesion: 0.13
Nodes (30): FnMut, Into, Item, Iterator, accepts_exactly_ten_percent_and_flags_more(), BenchmarkCase, BenchmarkResult, compare() (+22 more)

### Community 5 - "check-workspace/src/main.rs / String"
Cohesion: 0.16
Nodes (18): BTreeSet, as_array(), as_object(), as_string(), check(), Json, JsonParser, main() (+10 more)

### Community 6 - "check-workspace/tests/cli.rs / .new()"
Cohesion: 0.24
Nodes (18): executable_accepts_workspace_and_local_dependency_packages(), executable_refuses_external_packages_without_changing_lock_bytes(), executable_refuses_git_source_and_preserves_lock_bytes(), executable_refuses_lock_that_needs_resolution_without_changing_bytes(), executable_refuses_non_member_path_packages_and_invalid_locks(), executable_refuses_registry_source_even_when_name_and_version_match_member(), executable_rejects_excluded_path_dependency_without_changing_lock(), executable_rejects_extra_arguments() (+10 more)

### Community 7 - "unicode/src/lib.rs / encode()"
Cohesion: 0.15
Nodes (16): decode(), decode_single_byte(), decode_utf16(), detect_encoding(), encode(), EncodeError, Encoding, encoding_reports_character_and_utf8_byte_offset() (+8 more)

### Community 8 - "US-113 — Pont natif ↔ JS / §6.8 — Shell desktop Windows"
Cohesion: 0.14
Nodes (22): §1 — Vision, §10 — Risques, §6.8 — Shell desktop Windows, §7.2 — Desktop, Zero Dependency Rule (D11), D6 — Distribution : exécutable **portable** d'abord ; installeur et mise à jour automatique plus tard., D9 — Shell desktop : **couche maison Win32 + WebView2** (pas de Tauri)., F-COM-002 — Ouvrir un fichier (boîte de dialogue native, glisser-déposer sur la fenêtre, argument en ligne de commande / association de fichier) (+14 more)

### Community 9 - "check-license/src/main.rs / check_tree()"
Cohesion: 0.17
Nodes (19): accepts_a_valid_header(), check_tree(), create_symlink(), ignores_generated_and_agent_work_directories(), main(), rejects_missing_and_incorrect_headers_and_reports_the_paths(), rejects_symlinked_files_and_directories_when_supported(), reports_invalid_utf8_instead_of_succeeding() (+11 more)

### Community 10 - "US-104 — Bibliothèque de composants d'interface / US-106 — Ruban"
Cohesion: 0.18
Nodes (19): §6.14 — Accessibilité, D13 — Accessibilité requise (cible RGAA 4.1 / WCAG 2.1 AA)., F-COM-018 — Interface à ruban (onglets par module), barre d'état, panneaux latéraux repliables, F-COM-019 — Thème clair / sombre / suivre le système ; respect du mode contraste élevé Windows, F-COM-020 — Raccourcis clavier calqués sur ceux de MS Office en français ; affectations de référence figées dans une table unique sans conflit (US-231), dont Ctrl+Maj+V pour coller le texte seul et Ctrl+Alt+C / Ctrl+Alt+V pour copier/appliquer la mise en forme, F-COM-021 — Navigation complète au clavier, compatibilité lecteurs d'écran (NVDA, Narrateur), F-COM-022 — Préférences utilisateur stockées à côté de l'exe en mode portable ; repli en mode local uniquement sur choix explicite si le dossier n'est pas inscriptible (§7.2), US-104 — Bibliothèque de composants d'interface (+11 more)

### Community 11 - "bench/src/main.rs / run()"
Cohesion: 0.21
Nodes (16): cli_lists_and_measures_multiple_real_cases(), cli_preserves_reference_and_does_not_create_output_for_invalid_reference(), cli_refuses_malformed_reference_without_panicking(), cli_reports_unwritable_output_and_incomplete_reference(), main(), Options, parse_args(), reject_same_file() (+8 more)

### Community 12 - "SPEC.md v0.2 / §5.2 — Principes de fidélité"
Cohesion: 0.17
Nodes (12): §5.2 — Principes de fidélité, §6 — Architecture technique, §9 — Licence et propriété, SPEC.md v0.2, F-COM-001 — Créer un document vierge ou depuis un modèle, F-COM-004 — Liste des documents récents, F-COM-023 — Onglets ou fenêtres multiples (plusieurs documents ouverts), User stories V0/V1 (+4 more)

### Community 13 - "§6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout"
Cohesion: 0.24
Nodes (15): §6.15 — Internationalisation, §6.4 — Texte et polices, D12 — Interface en français en MVP, toutes les chaînes externalisées dès le départ., US-010 — Générateur de tables Unicode, US-011 — Segmentation en graphèmes et en mots, US-012 — Opportunités de coupure de ligne, US-013 — Casse et normalisation, US-050 — Tables de base OpenType (+7 more)

### Community 14 - "bench/tests/cli.rs / stdout_csv()"
Cohesion: 0.27
Nodes (10): executable_list_and_case_selection_return_only_requested_rows(), executable_rejects_extreme_iteration_counts_without_panicking(), executable_rejects_same_output_and_reference_without_changing_baseline(), executable_stdout_is_csv_with_and_without_reference(), Output, PathBuf, Vec, run() (+2 more)

### Community 15 - "bench_indexed_tree.rs / measure()"
Cohesion: 0.25
Nodes (10): fixture(), FIXTURE_SIZE, insertion_positions(), INSERTIONS, LIMIT, main(), measure(), REQUIRED_SAMPLES (+2 more)

### Community 16 - "US-220 — Saisir du texte / US-072 — Pile annuler / rétablir"
Cohesion: 0.24
Nodes (10): F-COM-009 — Annuler / rétablir illimités (bornés par la mémoire), y compris après enregistrement, F-TXT-001 — Saisie de texte avec gestion complète du clavier français (touches mortes, AltGr, IME), F-TXT-004 — Insertion de caractères spéciaux, espaces insécables (insertion automatique avant `: ; ! ?` en français, paramétrable), F-TXT-005 — Correction automatique de base (guillemets français « », majuscule en début de phrase), US-072 — Pile annuler / rétablir, US-220 — Saisir du texte, US-226 — Annuler et rétablir, US-227 — Caractères spéciaux (+2 more)

### Community 17 - "§6.6 — Rendu / US-060 — Géométrie 2D"
Cohesion: 0.36
Nodes (9): §6.6 — Rendu, F-COM-013 — Impression (choix imprimante, pages, copies, recto-verso selon pilote), US-060 — Géométrie 2D, US-061 — Rasteriseur logiciel, US-080 — Format de liste d'affichage, US-081 — Backend SVG, US-082 — Backend raster, US-101 — Rendu canvas (+1 more)

### Community 18 - "US-201 — Moteur de styles / US-250 — Appliquer un style"
Cohesion: 0.25
Nodes (9): F-TXT-020 — Styles de paragraphe, de caractère, de tableau, de liste ; héritage ; galerie de styles, F-TXT-021 — Jeu de styles par défaut équivalent à Word (Normal, Titre 1–9, Titre, Citation…), F-TXT-022 — Créer, modifier, renommer et supprimer un style, F-TXT-024 — Volet de navigation (plan par titres), US-201 — Moteur de styles, US-202 — Document par défaut, US-250 — Appliquer un style, US-251 — Gérer les styles (+1 more)

### Community 19 - "US-240 — Mettre en forme les caractères depuis le ruban / F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères"
Cohesion: 0.32
Nodes (8): F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères, F-TXT-013 — Reproduire la mise en forme, F-TXT-014 — Effacer la mise en forme, US-240 — Mettre en forme les caractères depuis le ruban, US-241 — Dialogue Police, US-242 — Modifier la casse, US-247 — Reproduire la mise en forme, US-248 — Effacer la mise en forme

### Community 20 - "US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)"
Cohesion: 0.43
Nodes (7): §6.1 — Vue d'ensemble, §6.7 — Pont WASM ↔ JS (écrit à la main), US-090 — ABI WASM, US-091 — Protocole de messages, US-092 — Moteur dans un Web Worker, US-100 — Interface Host, US-103 — Capture de la saisie

### Community 21 - "Point / Path"
Cohesion: 0.40
Nodes (6): AffineTransform, De Casteljau Subdivision, MAX_COORDINATE, Path, Point, Rect

### Community 22 - "tables.rs / CP437"
Cohesion: 0.40
Nodes (4): CP437, ISO_8859_1, MAC_ROMAN, WINDOWS_1252

### Community 23 - "US-331 — Écrivain PDF / US-330 — Sous-ensemble de polices"
Cohesion: 0.50
Nodes (5): §6.12 — PDF, F-COM-014 — Export PDF (texte sélectionnable, polices incorporées en sous-ensemble, liens, signets depuis les titres), US-330 — Sous-ensemble de polices, US-331 — Écrivain PDF, US-332 — Exporter en PDF

### Community 24 - "US-342 — Zoom / US-343 — Statistiques"
Cohesion: 0.40
Nodes (5): F-COM-012 — Zoom 10 %–500 %, ajustement page/largeur, F-TXT-006 — Statistiques (mots, caractères, paragraphes, pages), US-342 — Zoom, US-343 — Statistiques, US-344 — Barre d'état

### Community 25 - "Encoding Enum / decode"
Cohesion: 0.50
Nodes (4): decode, detect_encoding, encode, Encoding Enum

### Community 26 - "IndexedTree<T> / bench_indexed_tree"
Cohesion: 0.67
Nodes (3): bench_indexed_tree, IndexedTree<T>, NodeId

### Community 27 - "US-340 — Rechercher / F-COM-010 — Rechercher / remplacer (texte, casse, mot entier)"
Cohesion: 1.00
Nodes (3): F-COM-010 — Rechercher / remplacer (texte, casse, mot entier), US-340 — Rechercher, US-341 — Remplacer

## Knowledge Gaps
- **66 isolated node(s):** `MAX_COORDINATE`, `MAX_FLATTENED_SEGMENTS`, `MAX_SUBDIVISION_DEPTH`, `FIXTURE_SIZE`, `INSERTIONS` (+61 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 165 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **38 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `SPEC.md v0.2` connect `SPEC.md v0.2 / §5.2 — Principes de fidélité` to `US-001 — Initialiser le workspace / US-200 — Modèle de document texte`, `US-212 — Pagination / US-304 — Lecture docx : sections, en-têtes, champs, liens`, `US-113 — Pont natif ↔ JS / §6.8 — Shell desktop Windows`, `US-104 — Bibliothèque de composants d'interface / US-106 — Ruban`, `§6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout`, `US-220 — Saisir du texte / US-072 — Pile annuler / rétablir`, `§6.6 — Rendu / US-060 — Géométrie 2D`, `US-201 — Moteur de styles / US-250 — Appliquer un style`, `US-240 — Mettre en forme les caractères depuis le ruban / F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères`, `US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)`, `US-331 — Écrivain PDF / US-330 — Sous-ensemble de polices`, `US-342 — Zoom / US-343 — Statistiques`, `US-340 — Rechercher / F-COM-010 — Rechercher / remplacer (texte, casse, mot entier)`?**
  _High betweenness centrality (0.096) - this node is a cross-community bridge._
- **Why does `Path` connect `geom/src/lib.rs / Point` to `check-license/src/main.rs / check_tree()`, `check-workspace/src/main.rs / String`, `check-workspace/tests/cli.rs / .new()`?**
  _High betweenness centrality (0.039) - this node is a cross-community bridge._
- **Why does `F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule)` connect `US-212 — Pagination / US-304 — Lecture docx : sections, en-têtes, champs, liens` to `SPEC.md v0.2 / §5.2 — Principes de fidélité`?**
  _High betweenness centrality (0.007) - this node is a cross-community bridge._
- **What connects `MAX_COORDINATE`, `MAX_FLATTENED_SEGMENTS`, `MAX_SUBDIVISION_DEPTH` to the rest of the system?**
  _66 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `geom/src/lib.rs / Point` be split into smaller, more focused modules?**
  _Cohesion score 0.0823293172690763 - nodes in this community are weakly interconnected._
- **Should `IndexedTree<T> / model/src/lib.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.0937625754527163 - nodes in this community are weakly interconnected._
- **Should `US-001 — Initialiser le workspace / US-200 — Modèle de document texte` be split into smaller, more focused modules?**
  _Cohesion score 0.056692242114237 - nodes in this community are weakly interconnected._
Références AST externes non résolues exclues du graphe : 40 (détail dans unresolved-structural-references.json). Le graphe simple peut fusionner plusieurs relations entre les mêmes symboles ; les relations brutes restent dans .graphify_extract.json.

## Périmètre de cette mise à jour

La carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --cargo` a réussi après mise en cache complète des documents et exclusion du runtime agent ; les API locales de Graphify régénèrent ce rapport et ses libellés. Cargo.toml est couvert par les métadonnées de ses 31 paquets, même si son extraction AST produit zéro nœud. Le coût LLM de cette mise à jour est nul ; le coût historique reste dans cost.json.
