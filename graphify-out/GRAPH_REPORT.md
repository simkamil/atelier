# Graph Report - mon-bureau  (2026-10-10)

## Corpus Check
- 86 files · ~55,894 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 43 file(s) not represented in the graph (top: .bin 29, .out 7, (none) 4)

## Summary
- 930 nodes · 2300 edges · 104 communities (29 shown, 42 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 13 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- model/src/lib.rs / .new()
- render/src/lib.rs / DisplayListError
- String / bench/src/lib.rs
- Point / geom/src/lib.rs
- compress/src/lib.rs / .push()
- US-104 — Bibliothèque de composants d'interface / US-212 — Pagination
- Result / CodecError
- US-113 — Pont natif ↔ JS / §6.8 — Shell desktop Windows
- check-workspace/tests/cli.rs / .new()
- §6.13 — Robustesse et protection des données / §5.1 — Matrice
- unicode/src/lib.rs / encode()
- §6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout
- check-license/src/main.rs / check_tree()
- SPEC.md v0.2 / §5.2 — Principes de fidélité
- US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)
- US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles
- US-200 — Modèle de document texte / US-071 — Opérations et transactions
- bench/tests/cli.rs / stdout_csv()
- bench_indexed_tree.rs / measure()
- US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base
- US-220 — Saisir du texte / US-072 — Pile annuler / rétablir
- §6.6 — Rendu / US-060 — Géométrie 2D
- US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)
- Point / Path
- DisplayList / atelier-geom
- tables.rs / CP437
- US-074 — Journal de transactions / US-325 — Récupération automatique
- Encoding Enum / decode
- IndexedTree<T> / bench_indexed_tree
- ATOP Binary Format / US-071 Transactions
- atelier-geom / atelier-render
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
- atelier-image
- atelier-lua
- atelier-model
- atelier-raster
- atelier-server
- atelier-shape
- atelier-sheet-model
- atelier-slide-model
- atelier-text-layout
- atelier-text-model
- atelier-unicode
- atelier-xml
- atelier-zip
- Mixed UTF-8 Corpus
- Repetitive Corpus
- Development Documentation
- US-080 — Liste d’affichage par page
- CI Workflow
- Technical Architecture
- Atelier Vision
- Transaction Journal
- Atomic Save
- Accessibility Mirror

## God Nodes (most connected - your core abstractions)
1. `SPEC.md v0.2` - 80 edges
2. `DisplayListError` - 52 edges
3. `Point` - 40 edges
4. `GeometryError` - 32 edges
5. `IndexedTree<T>` - 29 edges
6. `CodecError` - 29 edges
7. `Rect` - 23 edges
8. `decode_command()` - 19 edges
9. `US-001 — Initialiser le workspace` - 18 edges
10. `encode_command()` - 16 edges

## Surprising Connections (you probably didn't know these)
- `US-001 — Initialiser le workspace` --references--> `Zero Dependency Rule (D11)`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `check_tree()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs
- `create_symlink()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs
- `visit()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs
- `check()` --references--> `Path`  [EXTRACTED]
  tools/check-workspace/src/main.rs → crates/geom/src/lib.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **ATDL Serialization and Validation** — atelier_render_displaylist, atelier_render_atdl_format, atelier_render_limits [EXTRACTED 1.00]
- **Text Encoding API** — docs_us_014_encodings_encoding, docs_us_014_encodings_detect_encoding, docs_us_014_encodings_decode, docs_us_014_encodings_encode [EXTRACTED 1.00]
- **Geometry Primitives** — docs_us_060_geometry_point, docs_us_060_geometry_rect, docs_us_060_geometry_path, docs_us_060_geometry_affinetransform [EXTRACTED 1.00]

## Communities (104 total, 42 thin omitted)

### Community 0 - "model/src/lib.rs / .new()"
Cohesion: 0.06
Nodes (55): Checkpoint, ArenaSlot, assert_matches(), deleting_a_two_child_node_preserves_successor_identity(), deterministic_pseudo_random_transactions_round_trip(), empty_page(), failed_operation_application_rolls_back_prior_mutations(), generic_non_clone_state_rolls_back_when_apply_fails_mid_transaction() (+47 more)

### Community 1 - "render/src/lib.rs / DisplayListError"
Cohesion: 0.07
Nodes (65): arbitrary_bytes_never_panic(), binary_header(), Color, Command, complex(), cumulative_path_and_glyph_budgets_are_enforced(), decode_command(), decoder_checks_cumulative_budgets_before_allocating() (+57 more)

### Community 2 - "String / bench/src/lib.rs"
Cohesion: 0.06
Nodes (62): BTreeSet, String, FnMut, Into, Item, Iterator, accepts_exactly_ten_percent_and_flags_more(), BenchmarkCase (+54 more)

### Community 3 - "Point / geom/src/lib.rs"
Cohesion: 0.08
Nodes (45): affine_composition_order_and_inverse_are_explicit(), AffineTransform, assert_flat_curve(), checked(), close_then_line_quadratic_or_cubic_starts_a_new_open_subpath(), coordinates(), coordinates_cubic(), cubic_derivative_roots() (+37 more)

### Community 4 - "compress/src/lib.rs / .push()"
Cohesion: 0.07
Nodes (48): adler32(), BitWriter, BitWriter<'a>, Code, CODE_ORDER, CodeOp, compress(), compress_with_format() (+40 more)

### Community 5 - "US-104 — Bibliothèque de composants d'interface / US-212 — Pagination"
Cohesion: 0.05
Nodes (66): §6.14 — Accessibilité, §6.16 — Objectifs de performance (machine de référence : PC grand public de 2020, 8 Go), §6.5 — Mise en page (traitement de texte), D13 — Accessibilité requise (cible RGAA 4.1 / WCAG 2.1 AA)., F-COM-018 — Interface à ruban (onglets par module), barre d'état, panneaux latéraux repliables, F-COM-019 — Thème clair / sombre / suivre le système ; respect du mode contraste élevé Windows, F-COM-020 — Raccourcis clavier calqués sur ceux de MS Office en français ; affectations de référence figées dans une table unique sans conflit (US-231), dont Ctrl+Maj+V pour coller le texte seul et Ctrl+Alt+C / Ctrl+Alt+V pour copier/appliquer la mise en forme, F-COM-021 — Navigation complète au clavier, compatibilité lecteurs d'écran (NVDA, Narrateur) (+58 more)

### Community 6 - "Result / CodecError"
Cohesion: 0.12
Nodes (14): BinaryValue, CodecError, Decoder, Decoder<'a>, Encoder, i32, Operation, Formatter (+6 more)

### Community 7 - "US-113 — Pont natif ↔ JS / §6.8 — Shell desktop Windows"
Cohesion: 0.13
Nodes (24): §1 — Vision, §10 — Risques, §6.8 — Shell desktop Windows, §7.2 — Desktop, Zero Dependency Rule (D11), D6 — Distribution : exécutable **portable** d'abord ; installeur et mise à jour automatique plus tard., D9 — Shell desktop : **couche maison Win32 + WebView2** (pas de Tauri)., F-COM-002 — Ouvrir un fichier (boîte de dialogue native, glisser-déposer sur la fenêtre, argument en ligne de commande / association de fichier) (+16 more)

### Community 8 - "check-workspace/tests/cli.rs / .new()"
Cohesion: 0.24
Nodes (18): executable_accepts_workspace_and_local_dependency_packages(), executable_refuses_external_packages_without_changing_lock_bytes(), executable_refuses_git_source_and_preserves_lock_bytes(), executable_refuses_lock_that_needs_resolution_without_changing_bytes(), executable_refuses_non_member_path_packages_and_invalid_locks(), executable_refuses_registry_source_even_when_name_and_version_match_member(), executable_rejects_excluded_path_dependency_without_changing_lock(), executable_rejects_extra_arguments() (+10 more)

### Community 9 - "§6.13 — Robustesse et protection des données / §5.1 — Matrice"
Cohesion: 0.15
Nodes (22): §5.1 — Matrice, §6.13 — Robustesse et protection des données, D2 — Format natif OOXML (docx/xlsx/pptx). Lecture des formats binaires anciens (doc/xls/ppt) et ODF (odt/ods/odp)., F-COM-017 — Propriétés du document (titre, auteur, sujet, mots-clés, dates), F-TXT-065 — Protection : ouverture par mot de passe (chiffrement ECMA-376 Agile), restriction de modification, US-004 — Fuzzer maison, US-014 — Encodages de texte, US-022 — Lecture ZIP (+14 more)

### Community 10 - "unicode/src/lib.rs / encode()"
Cohesion: 0.15
Nodes (15): decode(), decode_single_byte(), decode_utf16(), detect_encoding(), encode(), EncodeError, Encoding, encoding_reports_character_and_utf8_byte_offset() (+7 more)

### Community 11 - "§6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout"
Cohesion: 0.16
Nodes (21): §6 — Architecture technique, §6.12 — PDF, §6.15 — Internationalisation, §6.4 — Texte et polices, D12 — Interface en français en MVP, toutes les chaînes externalisées dès le départ., F-COM-014 — Export PDF (texte sélectionnable, polices incorporées en sous-ensemble, liens, signets depuis les titres), US-010 — Générateur de tables Unicode, US-011 — Segmentation en graphèmes et en mots (+13 more)

### Community 12 - "check-license/src/main.rs / check_tree()"
Cohesion: 0.18
Nodes (18): accepts_a_valid_header(), check_tree(), create_symlink(), ignores_generated_and_agent_work_directories(), main(), rejects_missing_and_incorrect_headers_and_reports_the_paths(), rejects_symlinked_files_and_directories_when_supported(), reports_invalid_utf8_instead_of_succeeding() (+10 more)

### Community 13 - "SPEC.md v0.2 / §5.2 — Principes de fidélité"
Cohesion: 0.13
Nodes (16): §5.2 — Principes de fidélité, §9 — Licence et propriété, SPEC.md v0.2, F-COM-001 — Créer un document vierge ou depuis un modèle, F-COM-004 — Liste des documents récents, F-COM-012 — Zoom 10 %–500 %, ajustement page/largeur, F-COM-023 — Onglets ou fenêtres multiples (plusieurs documents ouverts), F-TXT-006 — Statistiques (mots, caractères, paragraphes, pages) (+8 more)

### Community 14 - "US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)"
Cohesion: 0.22
Nodes (17): §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe), §8 — Qualité et tests, F-TXT-032 — Images PNG, JPEG, GIF, BMP : insertion, redimensionnement, habillage (aligné sur le texte, carré, haut et bas, derrière, devant), US-001 — Initialiser le workspace, US-002 — Intégration continue, US-003 — Tests par images de référence, US-020 — Décompression deflate, US-021 — Compression deflate (+9 more)

### Community 15 - "US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles"
Cohesion: 0.21
Nodes (16): §5 — Formats de fichiers, F-COM-003 — Enregistrer, Enregistrer sous (choix du format), F-COM-005 — Enregistrement atomique : jamais de fichier corrompu en cas de crash pendant l'écriture (§6.13), F-TXT-023 — Listes à puces, numérotées, multiniveaux ; redémarrer/continuer la numérotation, F-TXT-040 — Objets non gérés (SmartArt, graphiques, OLE) : affichage de l'image de repli contenue dans le fichier et préservation à l'enregistrement, US-252 — Listes à puces et numérotées, US-253 — Listes multiniveaux, US-274 — Objets non pris en charge (+8 more)

### Community 16 - "US-200 — Modèle de document texte / US-071 — Opérations et transactions"
Cohesion: 0.24
Nodes (13): §6.3 — Modèle de document et opérations, F-COM-010 — Rechercher / remplacer (texte, casse, mot entier), F-COM-016 — Presse-papiers : texte brut, HTML, format interne riche (copie entre documents sans perte), F-TXT-034 — Liens hypertexte et signets, US-070 — Arbre de nœuds indexé, US-071 — Opérations et transactions, US-073 — Modèle de sélection, US-200 — Modèle de document texte (+5 more)

### Community 17 - "bench/tests/cli.rs / stdout_csv()"
Cohesion: 0.27
Nodes (10): executable_list_and_case_selection_return_only_requested_rows(), executable_rejects_extreme_iteration_counts_without_panicking(), executable_rejects_same_output_and_reference_without_changing_baseline(), executable_stdout_is_csv_with_and_without_reference(), Output, PathBuf, Vec, run() (+2 more)

### Community 18 - "bench_indexed_tree.rs / measure()"
Cohesion: 0.25
Nodes (10): fixture(), FIXTURE_SIZE, insertion_positions(), INSERTIONS, LIMIT, main(), measure(), REQUIRED_SAMPLES (+2 more)

### Community 19 - "US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base"
Cohesion: 0.29
Nodes (11): F-TXT-035 — Champs : PAGE, NUMPAGES, DATE, TIME, F-TXT-050 — Format de papier, orientation, marges, F-TXT-051 — Sauts de page et de section, sections avec mise en page différente, F-TXT-052 — En-têtes et pieds de page (première page différente, pages paires/impaires), F-TXT-053 — Numérotation des pages (format, point de départ), US-280 — Format, orientation et marges, US-281 — Sections, US-282 — En-têtes et pieds de page (+3 more)

### Community 20 - "US-220 — Saisir du texte / US-072 — Pile annuler / rétablir"
Cohesion: 0.24
Nodes (10): F-COM-009 — Annuler / rétablir illimités (bornés par la mémoire), y compris après enregistrement, F-TXT-001 — Saisie de texte avec gestion complète du clavier français (touches mortes, AltGr, IME), F-TXT-004 — Insertion de caractères spéciaux, espaces insécables (insertion automatique avant `: ; ! ?` en français, paramétrable), F-TXT-005 — Correction automatique de base (guillemets français « », majuscule en début de phrase), US-072 — Pile annuler / rétablir, US-220 — Saisir du texte, US-226 — Annuler et rétablir, US-227 — Caractères spéciaux (+2 more)

### Community 21 - "§6.6 — Rendu / US-060 — Géométrie 2D"
Cohesion: 0.36
Nodes (9): §6.6 — Rendu, F-COM-013 — Impression (choix imprimante, pages, copies, recto-verso selon pilote), US-060 — Géométrie 2D, US-061 — Rasteriseur logiciel, US-080 — Format de liste d'affichage, US-081 — Backend SVG, US-082 — Backend raster, US-101 — Rendu canvas (+1 more)

### Community 22 - "US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)"
Cohesion: 0.43
Nodes (7): §6.1 — Vue d'ensemble, §6.7 — Pont WASM ↔ JS (écrit à la main), US-090 — ABI WASM, US-091 — Protocole de messages, US-092 — Moteur dans un Web Worker, US-100 — Interface Host, US-103 — Capture de la saisie

### Community 23 - "Point / Path"
Cohesion: 0.40
Nodes (6): AffineTransform, De Casteljau Subdivision, MAX_COORDINATE, Path, Point, Rect

### Community 24 - "DisplayList / atelier-geom"
Cohesion: 0.40
Nodes (5): atelier-geom, ATDL Binary Format, Command, DisplayList, atelier_render::limits

### Community 25 - "tables.rs / CP437"
Cohesion: 0.40
Nodes (4): CP437, ISO_8859_1, MAC_ROMAN, WINDOWS_1252

### Community 26 - "US-074 — Journal de transactions / US-325 — Récupération automatique"
Cohesion: 0.50
Nodes (5): §11 — Points ouverts (non bloquants pour la validation), F-COM-006 — Sauvegarde de récupération automatique autonome (instantané de base + journal, intervalle paramétrable, défaut 2 min) et proposition de restauration au redémarrage après crash ; récupération jusqu'au dernier point durable (§6.13), US-074 — Journal de transactions, US-325 — Récupération automatique, US-363 — Gestion des plantages

### Community 27 - "Encoding Enum / decode"
Cohesion: 0.50
Nodes (4): decode, detect_encoding, encode, Encoding Enum

### Community 28 - "IndexedTree<T> / bench_indexed_tree"
Cohesion: 0.67
Nodes (3): bench_indexed_tree, IndexedTree<T>, NodeId

## Knowledge Gaps
- **95 isolated node(s):** `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA`, `DIST_BASE`, `DIST_EXTRA` (+90 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 205 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **42 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `String` connect `String / bench/src/lib.rs` to `render/src/lib.rs / DisplayListError`, `unicode/src/lib.rs / encode()`, `check-license/src/main.rs / check_tree()`, `Result / CodecError`?**
  _High betweenness centrality (0.274) - this node is a cross-community bridge._
- **Why does `parse_lock()` connect `String / bench/src/lib.rs` to `compress/src/lib.rs / .push()`?**
  _High betweenness centrality (0.086) - this node is a cross-community bridge._
- **Why does `Command` connect `render/src/lib.rs / DisplayListError` to `String / bench/src/lib.rs`, `Point / geom/src/lib.rs`?**
  _High betweenness centrality (0.082) - this node is a cross-community bridge._
- **What connects `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA` to the rest of the system?**
  _95 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `model/src/lib.rs / .new()` be split into smaller, more focused modules?**
  _Cohesion score 0.0629004076878276 - nodes in this community are weakly interconnected._
- **Should `render/src/lib.rs / DisplayListError` be split into smaller, more focused modules?**
  _Cohesion score 0.07232323232323232 - nodes in this community are weakly interconnected._
- **Should `String / bench/src/lib.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.05692883895131086 - nodes in this community are weakly interconnected._
Références AST externes non résolues exclues du graphe : 43 (détail dans unresolved-structural-references.json). Le graphe simple peut fusionner plusieurs relations entre les mêmes symboles ; les relations brutes restent dans .graphify_extract.json.

## Périmètre de cette mise à jour

La carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --cargo` a réussi après mise en cache complète des documents et exclusion du runtime agent ; les API locales de Graphify régénèrent ce rapport et ses libellés. Cargo.toml est couvert par les métadonnées de ses 31 paquets, même si son extraction AST produit zéro nœud. Le coût LLM de cette mise à jour est nul ; le coût historique reste dans cost.json.
