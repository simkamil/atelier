# Graph Report - mon-bureau  (2026-10-10)

## Corpus Check
- 90 files · ~70,877 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 68 file(s) not represented in the graph (top: .bin 29, .xml 14, .zip 11)

## Summary
- 1153 nodes · 3097 edges · 108 communities (35 shown, 39 thin omitted)
- Extraction: 100% EXTRACTED · 0% INFERRED · 0% AMBIGUOUS · INFERRED: 12 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- xml/src/lib.rs / XmlError
- render/src/lib.rs / DisplayListError
- zip/src/lib.rs / .new()
- String / bench/src/lib.rs
- Point / geom/src/lib.rs
- compress/src/lib.rs / .push()
- Result / CodecError
- SPEC.md v0.2 / US-113 — Pont natif ↔ JS
- model/src/lib.rs / .new()
- T / Option
- §6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout
- §6.13 — Robustesse et protection des données / §5.1 — Matrice
- US-212 — Pagination / F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule)
- IndexedTree<T> / .node()
- Error / .apply()
- check-workspace/tests/cli.rs / .new()
- US-200 — Modèle de document texte / US-220 — Saisir du texte
- unicode/src/lib.rs / encode()
- check-license/src/main.rs / check_tree()
- US-104 — Bibliothèque de composants d'interface / US-106 — Ruban
- US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)
- US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles
- US-201 — Moteur de styles / US-243 — Mettre en forme les paragraphes depuis le ruban
- bench/tests/cli.rs / stdout_csv()
- bench_indexed_tree.rs / measure()
- US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base
- US-240 — Mettre en forme les caractères depuis le ruban / F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères
- US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)
- manifest.json / deterministic
- Point / Path
- DisplayList / atelier-geom
- tables.rs / CP437
- Encoding Enum / decode
- atelier-zip / atelier-compress
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
- atelier-xml
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
3. `XmlError` - 42 edges
4. `Point` - 40 edges
5. `GeometryError` - 32 edges
6. `IndexedTree<T>` - 29 edges
7. `CodecError` - 29 edges
8. `Reader<R>` - 25 edges
9. `Rect` - 23 edges
10. `decode_command()` - 19 edges

## Surprising Connections (you probably didn't know these)
- `US-001 — Initialiser le workspace` --references--> `Zero Dependency Rule (D11)`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `US-309 — Modèles et documents à macros` --references--> `§5.1 — Matrice`  [EXTRACTED]
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
- **ATDL Serialization and Validation** — atelier_render_displaylist, atelier_render_atdl_format, atelier_render_limits [EXTRACTED 1.00]
- **Text Encoding API** — docs_us_014_encodings_encoding, docs_us_014_encodings_detect_encoding, docs_us_014_encodings_decode, docs_us_014_encodings_encode [EXTRACTED 1.00]
- **Geometry Primitives** — docs_us_060_geometry_point, docs_us_060_geometry_rect, docs_us_060_geometry_path, docs_us_060_geometry_affinetransform [EXTRACTED 1.00]

## Communities (108 total, 39 thin omitted)

### Community 0 - "xml/src/lib.rs / XmlError"
Cohesion: 0.07
Nodes (69): all_byte_boundaries_of_utf16_and_bom_are_incremental(), Attribute, bounded_push(), comments_and_pi_normalize_crlf_and_cr(), constructs_and_dtd_do_not_expand(), Decoder, Decoder<R>, Diagnostic (+61 more)

### Community 1 - "render/src/lib.rs / DisplayListError"
Cohesion: 0.07
Nodes (65): arbitrary_bytes_never_panic(), binary_header(), Color, Command, complex(), cumulative_path_and_glyph_budgets_are_enforced(), decode_command(), decoder_checks_cumulative_budgets_before_allocating() (+57 more)

### Community 2 - "zip/src/lib.rs / .new()"
Cohesion: 0.07
Nodes (74): applies_limits_before_read_all_and_detects_partial_archive(), Archive, Archive<'a>, canonical_name(), CENTRAL, central_entry_count(), central_entry_disk_number_is_checked_independently(), central_has_zip64() (+66 more)

### Community 3 - "String / bench/src/lib.rs"
Cohesion: 0.06
Nodes (62): BTreeSet, String, FnMut, accepts_exactly_ten_percent_and_flags_more(), BenchmarkCase, BenchmarkResult, compare(), csv_field() (+54 more)

### Community 4 - "Point / geom/src/lib.rs"
Cohesion: 0.08
Nodes (45): affine_composition_order_and_inverse_are_explicit(), AffineTransform, assert_flat_curve(), checked(), close_then_line_quadratic_or_cubic_starts_a_new_open_subpath(), coordinates(), coordinates_cubic(), cubic_derivative_roots() (+37 more)

### Community 5 - "compress/src/lib.rs / .push()"
Cohesion: 0.07
Nodes (48): adler32(), BitWriter, BitWriter<'a>, Code, CODE_ORDER, CodeOp, compress(), compress_with_format() (+40 more)

### Community 6 - "Result / CodecError"
Cohesion: 0.15
Nodes (10): BinaryValue, CodecError, Decoder, Decoder<'a>, Encoder, i32, Result, Self (+2 more)

### Community 7 - "SPEC.md v0.2 / US-113 — Pont natif ↔ JS"
Cohesion: 0.08
Nodes (37): §1 — Vision, §10 — Risques, §6.8 — Shell desktop Windows, §7.2 — Desktop, §9 — Licence et propriété, Zero Dependency Rule (D11), D6 — Distribution : exécutable **portable** d'abord ; installeur et mise à jour automatique plus tard., D9 — Shell desktop : **couche maison Win32 + WebView2** (pas de Tauri). (+29 more)

### Community 8 - "model/src/lib.rs / .new()"
Cohesion: 0.14
Nodes (26): assert_matches(), deleting_a_two_child_node_preserves_successor_identity(), deterministic_pseudo_random_transactions_round_trip(), failed_operation_application_rolls_back_prior_mutations(), identity_exhaustion_is_atomic(), insert_positions_and_stable_ids(), invalid_positions_do_not_mutate(), MAX_OPERATION_BYTES (+18 more)

### Community 9 - "T / Option"
Cohesion: 0.20
Nodes (15): ArenaSlot, empty_page(), IndexedTree, IndexedTreeOperation, IndexedTreeOperation<T>, Node, Node<T>, NodeId (+7 more)

### Community 10 - "§6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout"
Cohesion: 0.12
Nodes (29): §6 — Architecture technique, §6.12 — PDF, §6.15 — Internationalisation, §6.4 — Texte et polices, §6.6 — Rendu, F-COM-012 — Zoom 10 %–500 %, ajustement page/largeur, F-TXT-006 — Statistiques (mots, caractères, paragraphes, pages), US-010 — Générateur de tables Unicode (+21 more)

### Community 11 - "§6.13 — Robustesse et protection des données / §5.1 — Matrice"
Cohesion: 0.11
Nodes (28): §11 — Points ouverts (non bloquants pour la validation), §5.1 — Matrice, §6.13 — Robustesse et protection des données, D2 — Format natif OOXML (docx/xlsx/pptx). Lecture des formats binaires anciens (doc/xls/ppt) et ODF (odt/ods/odp)., F-COM-006 — Sauvegarde de récupération automatique autonome (instantané de base + journal, intervalle paramétrable, défaut 2 min) et proposition de restauration au redémarrage après crash ; récupération jusqu'au dernier point durable (§6.13), F-COM-014 — Export PDF (texte sélectionnable, polices incorporées en sous-ensemble, liens, signets depuis les titres), F-COM-017 — Propriétés du document (titre, auteur, sujet, mots-clés, dates), F-TXT-065 — Protection : ouverture par mot de passe (chiffrement ECMA-376 Agile), restriction de modification (+20 more)

### Community 12 - "US-212 — Pagination / F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule)"
Cohesion: 0.13
Nodes (26): §6.16 — Objectifs de performance (machine de référence : PC grand public de 2020, 8 Go), §6.5 — Mise en page (traitement de texte), F-TXT-002 — Sélection souris (clic, double-clic mot, triple-clic paragraphe, glisser, Maj+clic) et clavier (Maj+flèches, Ctrl+Maj+flèches…), F-TXT-003 — Glisser-déposer de texte dans le document, F-TXT-012 — Paragraphe : bordures, trame de fond, veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant, F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule), US-005 — Bancs de performance, US-210 — Mise en page des lignes (+18 more)

### Community 13 - "IndexedTree<T> / .node()"
Cohesion: 0.22
Nodes (4): Checkpoint, IndexedTree<T>, Default, TransactionalState

### Community 14 - "Error / .apply()"
Cohesion: 0.14
Nodes (14): generic_non_clone_state_rolls_back_when_apply_fails_mid_transaction(), IndexedTreeError, Operation, OperationError, PlainOp, PlainState, Display, Error (+6 more)

### Community 15 - "check-workspace/tests/cli.rs / .new()"
Cohesion: 0.24
Nodes (18): executable_accepts_workspace_and_local_dependency_packages(), executable_refuses_external_packages_without_changing_lock_bytes(), executable_refuses_git_source_and_preserves_lock_bytes(), executable_refuses_lock_that_needs_resolution_without_changing_bytes(), executable_refuses_non_member_path_packages_and_invalid_locks(), executable_refuses_registry_source_even_when_name_and_version_match_member(), executable_rejects_excluded_path_dependency_without_changing_lock(), executable_rejects_extra_arguments() (+10 more)

### Community 16 - "US-200 — Modèle de document texte / US-220 — Saisir du texte"
Cohesion: 0.13
Nodes (23): §6.3 — Modèle de document et opérations, F-COM-009 — Annuler / rétablir illimités (bornés par la mémoire), y compris après enregistrement, F-COM-010 — Rechercher / remplacer (texte, casse, mot entier), F-COM-016 — Presse-papiers : texte brut, HTML, format interne riche (copie entre documents sans perte), F-TXT-001 — Saisie de texte avec gestion complète du clavier français (touches mortes, AltGr, IME), F-TXT-004 — Insertion de caractères spéciaux, espaces insécables (insertion automatique avant `: ; ! ?` en français, paramétrable), F-TXT-005 — Correction automatique de base (guillemets français « », majuscule en début de phrase), F-TXT-034 — Liens hypertexte et signets (+15 more)

### Community 17 - "unicode/src/lib.rs / encode()"
Cohesion: 0.15
Nodes (15): decode(), decode_single_byte(), decode_utf16(), detect_encoding(), encode(), EncodeError, Encoding, encoding_reports_character_and_utf8_byte_offset() (+7 more)

### Community 18 - "check-license/src/main.rs / check_tree()"
Cohesion: 0.18
Nodes (18): accepts_a_valid_header(), check_tree(), create_symlink(), ignores_generated_and_agent_work_directories(), main(), rejects_missing_and_incorrect_headers_and_reports_the_paths(), rejects_symlinked_files_and_directories_when_supported(), reports_invalid_utf8_instead_of_succeeding() (+10 more)

### Community 19 - "US-104 — Bibliothèque de composants d'interface / US-106 — Ruban"
Cohesion: 0.18
Nodes (19): §6.14 — Accessibilité, D12 — Interface en français en MVP, toutes les chaînes externalisées dès le départ., D13 — Accessibilité requise (cible RGAA 4.1 / WCAG 2.1 AA)., F-COM-018 — Interface à ruban (onglets par module), barre d'état, panneaux latéraux repliables, F-COM-019 — Thème clair / sombre / suivre le système ; respect du mode contraste élevé Windows, F-COM-020 — Raccourcis clavier calqués sur ceux de MS Office en français ; affectations de référence figées dans une table unique sans conflit (US-231), dont Ctrl+Maj+V pour coller le texte seul et Ctrl+Alt+C / Ctrl+Alt+V pour copier/appliquer la mise en forme, F-COM-021 — Navigation complète au clavier, compatibilité lecteurs d'écran (NVDA, Narrateur), US-102 — Internationalisation (+11 more)

### Community 20 - "US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)"
Cohesion: 0.22
Nodes (17): §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe), §8 — Qualité et tests, F-TXT-032 — Images PNG, JPEG, GIF, BMP : insertion, redimensionnement, habillage (aligné sur le texte, carré, haut et bas, derrière, devant), US-001 — Initialiser le workspace, US-002 — Intégration continue, US-003 — Tests par images de référence, US-020 — Décompression deflate, US-021 — Compression deflate (+9 more)

### Community 21 - "US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles"
Cohesion: 0.21
Nodes (16): §5 — Formats de fichiers, §5.2 — Principes de fidélité, F-COM-003 — Enregistrer, Enregistrer sous (choix du format), F-TXT-023 — Listes à puces, numérotées, multiniveaux ; redémarrer/continuer la numérotation, F-TXT-040 — Objets non gérés (SmartArt, graphiques, OLE) : affichage de l'image de repli contenue dans le fichier et préservation à l'enregistrement, US-056 — Polices embarquées, US-252 — Listes à puces et numérotées, US-253 — Listes multiniveaux (+8 more)

### Community 22 - "US-201 — Moteur de styles / US-243 — Mettre en forme les paragraphes depuis le ruban"
Cohesion: 0.17
Nodes (16): F-TXT-011 — Paragraphe : alignement (gauche, centré, droite, justifié), retraits, première ligne, interligne, espacement avant/après, tabulations (gauche, centre, droite, décimale, points de suite), F-TXT-020 — Styles de paragraphe, de caractère, de tableau, de liste ; héritage ; galerie de styles, F-TXT-021 — Jeu de styles par défaut équivalent à Word (Normal, Titre 1–9, Titre, Citation…), F-TXT-022 — Créer, modifier, renommer et supprimer un style, F-TXT-024 — Volet de navigation (plan par titres), F-TXT-056 — Modes d'affichage : page, plan ; règles ; affichage des marques de mise en forme, US-201 — Moteur de styles, US-202 — Document par défaut (+8 more)

### Community 23 - "bench/tests/cli.rs / stdout_csv()"
Cohesion: 0.27
Nodes (10): executable_list_and_case_selection_return_only_requested_rows(), executable_rejects_extreme_iteration_counts_without_panicking(), executable_rejects_same_output_and_reference_without_changing_baseline(), executable_stdout_is_csv_with_and_without_reference(), Output, PathBuf, Vec, run() (+2 more)

### Community 24 - "bench_indexed_tree.rs / measure()"
Cohesion: 0.25
Nodes (10): fixture(), FIXTURE_SIZE, insertion_positions(), INSERTIONS, LIMIT, main(), measure(), REQUIRED_SAMPLES (+2 more)

### Community 25 - "US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base"
Cohesion: 0.29
Nodes (11): F-TXT-035 — Champs : PAGE, NUMPAGES, DATE, TIME, F-TXT-050 — Format de papier, orientation, marges, F-TXT-051 — Sauts de page et de section, sections avec mise en page différente, F-TXT-052 — En-têtes et pieds de page (première page différente, pages paires/impaires), F-TXT-053 — Numérotation des pages (format, point de départ), US-280 — Format, orientation et marges, US-281 — Sections, US-282 — En-têtes et pieds de page (+3 more)

### Community 26 - "US-240 — Mettre en forme les caractères depuis le ruban / F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères"
Cohesion: 0.32
Nodes (8): F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères, F-TXT-013 — Reproduire la mise en forme, F-TXT-014 — Effacer la mise en forme, US-240 — Mettre en forme les caractères depuis le ruban, US-241 — Dialogue Police, US-242 — Modifier la casse, US-247 — Reproduire la mise en forme, US-248 — Effacer la mise en forme

### Community 27 - "US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)"
Cohesion: 0.43
Nodes (7): §6.1 — Vue d'ensemble, §6.7 — Pont WASM ↔ JS (écrit à la main), US-090 — ABI WASM, US-091 — Protocole de messages, US-092 — Moteur dans un Web Worker, US-100 — Interface Host, US-103 — Capture de la saisie

### Community 28 - "manifest.json / deterministic"
Cohesion: 0.29
Nodes (6): deterministic, fixtures, generated_by, schema_version, xml_semantics, zip_semantics

### Community 29 - "Point / Path"
Cohesion: 0.40
Nodes (6): AffineTransform, De Casteljau Subdivision, MAX_COORDINATE, Path, Point, Rect

### Community 30 - "DisplayList / atelier-geom"
Cohesion: 0.40
Nodes (5): atelier-geom, ATDL Binary Format, Command, DisplayList, atelier_render::limits

### Community 31 - "tables.rs / CP437"
Cohesion: 0.40
Nodes (4): CP437, ISO_8859_1, MAC_ROMAN, WINDOWS_1252

### Community 32 - "Encoding Enum / decode"
Cohesion: 0.50
Nodes (4): decode, detect_encoding, encode, Encoding Enum

### Community 33 - "atelier-zip / atelier-compress"
Cohesion: 0.67
Nodes (3): atelier-compress, atelier-unicode, atelier-zip

### Community 34 - "IndexedTree<T> / bench_indexed_tree"
Cohesion: 0.67
Nodes (3): bench_indexed_tree, IndexedTree<T>, NodeId

## Knowledge Gaps
- **110 isolated node(s):** `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA`, `DIST_BASE`, `DIST_EXTRA` (+105 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 239 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **39 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `String` connect `String / bench/src/lib.rs` to `xml/src/lib.rs / XmlError`, `render/src/lib.rs / DisplayListError`, `zip/src/lib.rs / .new()`, `Result / CodecError`, `unicode/src/lib.rs / encode()`, `check-license/src/main.rs / check_tree()`?**
  _High betweenness centrality (0.433) - this node is a cross-community bridge._
- **Why does `Path` connect `Point / geom/src/lib.rs` to `check-license/src/main.rs / check_tree()`, `String / bench/src/lib.rs`, `check-workspace/tests/cli.rs / .new()`?**
  _High betweenness centrality (0.091) - this node is a cross-community bridge._
- **Why does `Command` connect `render/src/lib.rs / DisplayListError` to `String / bench/src/lib.rs`, `Point / geom/src/lib.rs`?**
  _High betweenness centrality (0.086) - this node is a cross-community bridge._
- **What connects `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA` to the rest of the system?**
  _110 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `xml/src/lib.rs / XmlError` be split into smaller, more focused modules?**
  _Cohesion score 0.06675981990374165 - nodes in this community are weakly interconnected._
- **Should `render/src/lib.rs / DisplayListError` be split into smaller, more focused modules?**
  _Cohesion score 0.07232323232323232 - nodes in this community are weakly interconnected._
- **Should `zip/src/lib.rs / .new()` be split into smaller, more focused modules?**
  _Cohesion score 0.06787878787878789 - nodes in this community are weakly interconnected._
Références AST externes non résolues exclues du graphe : 52 (détail dans unresolved-structural-references.json). Le graphe simple peut fusionner plusieurs relations entre les mêmes symboles ; les relations brutes restent dans .graphify_extract.json.

## Périmètre de cette mise à jour

La carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --cargo` a réussi après mise en cache des documents et exclusion du runtime agent ; les API locales de Graphify régénèrent ce rapport et ses libellés. Cargo.toml est couvert par les métadonnées de ses 31 paquets, même si son extraction AST produit zéro nœud. Cette génération locale par API n’utilise pas de LLM ; les coûts des extractions CLI restent dans cost.json.
