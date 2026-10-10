# Graph Report - mon-bureau  (2026-10-10)

## Corpus Check
- 125 files · ~109,406 words
- Verdict: corpus is large enough that graph structure adds value.
- Unclassified: 285 file(s) not represented in the graph (top: .rgba 187, .bin 29, .ttf 20)

## Summary
- 1652 nodes · 4409 edges · 146 communities (49 shown, 46 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 24 edges (avg confidence: 0.86)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- String / xml/src/lib.rs
- render/src/lib.rs / DisplayListError
- zip/src/lib.rs / .new()
- journal.rs / .new()
- Point / geom/src/lib.rs
- font/src/lib.rs / u16_at()
- compress/src/lib.rs / .push()
- bench/src/lib.rs / bench/src/main.rs
- svg.rs / render_svg()
- CodecError / Result
- SPEC.md v0.2 / US-113 — Pont natif ↔ JS
- check-workspace/src/main.rs / .value()
- US-212 — Pagination / US-201 — Moteur de styles
- history.rs / .apply()
- model/src/lib.rs / .new()
- decode_bmp() / bmp.rs
- NodeId / selection.rs
- §6.13 — Robustesse et protection des données / §5.1 — Matrice
- §6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout
- T / Option
- unicode/src/lib.rs / encode()
- check-workspace/tests/cli.rs / .new()
- IndexedTree<T> / .node()
- ImageError / decode_gif()
- src/png.rs / decode_png()
- gif_bmp.rs / bmp_header()
- Result / Op
- Operation / Error
- .mapped() / Result
- check-license/src/main.rs / check_tree()
- US-200 — Modèle de document texte / US-220 — Saisir du texte
- US-104 — Bibliothèque de composants d'interface / US-106 — Ruban
- US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)
- US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles
- US-240 — Mettre en forme les caractères depuis le ruban / §6.15 — Internationalisation
- tests/png.rs / zlib()
- US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base
- F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule) / US-261 — Mise en page des tableaux : largeurs et cellules
- History<O, Selection> / .retained_cost_bound()
- US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)
- format-reference/manifest.json / deterministic
- Transaction / Transaction<O>
- fonts/manifest.json / asset_count
- Point / Path
- atelier-compress / atelier-zip
- image/src/lib.rs / Image
- image-reference/manifest.json / cases
- Encoding Enum / decode
- Journal / SnapshotCodec
- Selection / PositionMap
- .fmt() / Formatter
- IndexedTree<T> / bench_indexed_tree
- check-assets.ps1 / Assert-Asset()
- FontFace / US-050: OpenType Table Reading
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
- atelier-lua
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
- Caladea Font License
- Carlito Font License
- Liberation Font License
- Development Documentation
- US-080 — Liste d’affichage par page
- FontTools
- CI Workflow
- Technical Architecture
- Atelier Vision
- Transaction Journal
- Atomic Save
- Accessibility Mirror

## God Nodes (most connected - your core abstractions)
1. `SPEC.md v0.2` - 80 edges
2. `DisplayListError` - 52 edges
3. `XmlError` - 49 edges
4. `Point` - 41 edges
5. `CodecError` - 39 edges
6. `GeometryError` - 32 edges
7. `ImageError` - 30 edges
8. `IndexedTree<T>` - 29 edges
9. `Reader<R>` - 29 edges
10. `NodeId` - 27 edges

## Surprising Connections (you probably didn't know these)
- `csv_field()` --references--> `String`  [EXTRACTED]
  tools/bench/src/lib.rs → crates/model/src/lib.rs
- `US-001 — Initialiser le workspace` --references--> `Zero Dependency Rule (D11)`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `US-309 — Modèles et documents à macros` --references--> `§5.1 — Matrice`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `check_tree()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs
- `create_symlink()` --references--> `Path`  [EXTRACTED]
  tools/check-license/src/main.rs → crates/geom/src/lib.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **ATDL Serialization and Validation** — atelier_render_displaylist, atelier_render_atdl_format, atelier_render_limits [EXTRACTED 1.00]
- **Text Encoding API** — docs_us_014_encodings_encoding, docs_us_014_encodings_detect_encoding, docs_us_014_encodings_decode, docs_us_014_encodings_encode [EXTRACTED 1.00]
- **Geometry Primitives** — docs_us_060_geometry_point, docs_us_060_geometry_rect, docs_us_060_geometry_path, docs_us_060_geometry_affinetransform [EXTRACTED 1.00]
- **OFL Licensed Fonts** — data_fonts_licenses_caladea_ofl, data_fonts_licenses_carlito_ofl, data_fonts_licenses_liberation_ofl [EXTRACTED 1.00]
- **Atelier Model Core Logic** — atelier_model_selection_selection, atelier_model_selection_positionmap, atelier_model_journal_journal, atelier_model_journal_snapshotcodec [INFERRED 0.80]

## Communities (146 total, 46 thin omitted)

### Community 0 - "String / xml/src/lib.rs"
Cohesion: 0.06
Nodes (82): String, attribute_values(), all_byte_boundaries_of_utf16_and_bom_are_incremental(), Attribute, bounded_push(), comments_and_pi_normalize_crlf_and_cr(), constructs_and_dtd_do_not_expand(), Decoder (+74 more)

### Community 1 - "render/src/lib.rs / DisplayListError"
Cohesion: 0.07
Nodes (65): arbitrary_bytes_never_panic(), binary_header(), Color, Command, complex(), cumulative_path_and_glyph_budgets_are_enforced(), decode_command(), decoder_checks_cumulative_budgets_before_allocating() (+57 more)

### Community 2 - "zip/src/lib.rs / .new()"
Cohesion: 0.07
Nodes (74): applies_limits_before_read_all_and_detects_partial_archive(), Archive, Archive<'a>, canonical_name(), CENTRAL, central_entry_count(), central_entry_disk_number_is_checked_independently(), central_has_zip64() (+66 more)

### Community 3 - "journal.rs / .new()"
Cohesion: 0.06
Nodes (68): C, crc32(), fixture(), FIXTURE_SIZE, insertion_positions(), INSERTIONS, LIMIT, main() (+60 more)

### Community 4 - "Point / geom/src/lib.rs"
Cohesion: 0.08
Nodes (45): affine_composition_order_and_inverse_are_explicit(), AffineTransform, assert_flat_curve(), checked(), close_then_line_quadratic_or_cubic_starts_a_new_open_subpath(), coordinates(), coordinates_cubic(), cubic_derivative_roots() (+37 more)

### Community 5 - "font/src/lib.rs / u16_at()"
Cohesion: 0.06
Nodes (63): EMBEDDED_FONTS, EmbeddedFont, EmbeddedFontStyle, bounds_alias_amplification_in_format14(), cmap_rank(), CmapSubtable, conflicting_cmap(), fixture() (+55 more)

### Community 6 - "compress/src/lib.rs / .push()"
Cohesion: 0.07
Nodes (48): adler32(), BitWriter, BitWriter<'a>, Code, CODE_ORDER, CodeOp, compress(), compress_with_format() (+40 more)

### Community 7 - "bench/src/lib.rs / bench/src/main.rs"
Cohesion: 0.06
Nodes (54): FnMut, accepts_exactly_ten_percent_and_flags_more(), BenchmarkCase, BenchmarkResult, compare(), csv_field(), default_registry(), MAX_ITERATIONS (+46 more)

### Community 8 - "svg.rs / render_svg()"
Cohesion: 0.09
Nodes (51): atelier-geom, ATDL Binary Format, Command, DisplayList, atelier_render::limits, active_clip_id(), BASE64, base64_images_and_escaped_metadata_are_inert_and_valid_xml() (+43 more)

### Community 9 - "CodecError / Result"
Cohesion: 0.15
Nodes (10): BinaryValue, CodecError, Decoder, Decoder<'a>, Encoder, i32, Result, Self (+2 more)

### Community 10 - "SPEC.md v0.2 / US-113 — Pont natif ↔ JS"
Cohesion: 0.08
Nodes (37): §1 — Vision, §10 — Risques, §6.8 — Shell desktop Windows, §7.2 — Desktop, §9 — Licence et propriété, Zero Dependency Rule (D11), D6 — Distribution : exécutable **portable** d'abord ; installeur et mise à jour automatique plus tard., D9 — Shell desktop : **couche maison Win32 + WebView2** (pas de Tauri). (+29 more)

### Community 11 - "check-workspace/src/main.rs / .value()"
Cohesion: 0.12
Nodes (18): BTreeSet, FAMILIES, as_array(), as_object(), as_string(), check(), Json, JsonParser (+10 more)

### Community 12 - "US-212 — Pagination / US-201 — Moteur de styles"
Cohesion: 0.09
Nodes (33): §6.16 — Objectifs de performance (machine de référence : PC grand public de 2020, 8 Go), §6.5 — Mise en page (traitement de texte), F-TXT-002 — Sélection souris (clic, double-clic mot, triple-clic paragraphe, glisser, Maj+clic) et clavier (Maj+flèches, Ctrl+Maj+flèches…), F-TXT-003 — Glisser-déposer de texte dans le document, F-TXT-011 — Paragraphe : alignement (gauche, centré, droite, justifié), retraits, première ligne, interligne, espacement avant/après, tabulations (gauche, centre, droite, décimale, points de suite), F-TXT-012 — Paragraphe : bordures, trame de fond, veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant, F-TXT-020 — Styles de paragraphe, de caractère, de tableau, de liste ; héritage ; galerie de styles, F-TXT-021 — Jeu de styles par défaut équivalent à Word (Normal, Titre 1–9, Titre, Citation…) (+25 more)

### Community 13 - "history.rs / .apply()"
Cohesion: 0.16
Nodes (23): apply_undo_redo_restore_state_and_selection(), budget_rejects_oversized_and_evicts_oldest_only_after_success(), EditKind, empty_edit_and_failed_edit_preserve_redo_and_selection(), Entry, grouped_undo_and_redo_failures_restore_state_and_preserve_history(), History, HistoryAction (+15 more)

### Community 14 - "model/src/lib.rs / .new()"
Cohesion: 0.16
Nodes (24): assert_matches(), deleting_a_two_child_node_preserves_successor_identity(), deterministic_pseudo_random_transactions_round_trip(), failed_operation_application_rolls_back_prior_mutations(), identity_exhaustion_is_atomic(), IndexedTreeOperation, insert_positions_and_stable_ids(), invalid_positions_do_not_mutate() (+16 more)

### Community 15 - "decode_bmp() / bmp.rs"
Cohesion: 0.13
Nodes (20): allocate(), BI_BITFIELDS, BI_RGB, BI_RLE4, BI_RLE8, component(), decode_bmp(), decode_rle() (+12 more)

### Community 16 - "NodeId / selection.rs"
Cohesion: 0.18
Nodes (18): Node<T>, NodeId, Affinity, delete_collapses_inclusive_end_and_shifts_after(), insert_respects_affinity_and_caret_defaults_after(), mapping_failure_is_atomic_for_every_endpoint(), MAX_SELECTION_RANGES, offsets_are_bytes_and_utf8_boundary_validation_is_deferred() (+10 more)

### Community 17 - "§6.13 — Robustesse et protection des données / §5.1 — Matrice"
Cohesion: 0.11
Nodes (28): §11 — Points ouverts (non bloquants pour la validation), §5.1 — Matrice, §6.13 — Robustesse et protection des données, D2 — Format natif OOXML (docx/xlsx/pptx). Lecture des formats binaires anciens (doc/xls/ppt) et ODF (odt/ods/odp)., F-COM-006 — Sauvegarde de récupération automatique autonome (instantané de base + journal, intervalle paramétrable, défaut 2 min) et proposition de restauration au redémarrage après crash ; récupération jusqu'au dernier point durable (§6.13), F-COM-014 — Export PDF (texte sélectionnable, polices incorporées en sous-ensemble, liens, signets depuis les titres), F-COM-017 — Propriétés du document (titre, auteur, sujet, mots-clés, dates), F-TXT-065 — Protection : ouverture par mot de passe (chiffrement ECMA-376 Agile), restriction de modification (+20 more)

### Community 18 - "§6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout"
Cohesion: 0.14
Nodes (27): §6 — Architecture technique, §6.12 — PDF, §6.4 — Texte et polices, §6.6 — Rendu, F-COM-012 — Zoom 10 %–500 %, ajustement page/largeur, F-TXT-006 — Statistiques (mots, caractères, paragraphes, pages), US-010 — Générateur de tables Unicode, US-011 — Segmentation en graphèmes et en mots (+19 more)

### Community 19 - "T / Option"
Cohesion: 0.23
Nodes (10): ArenaSlot, empty_page(), IndexedTreeOperation<T>, Node, RadixArena, RadixArena<T>, RadixPage, Box (+2 more)

### Community 20 - "unicode/src/lib.rs / encode()"
Cohesion: 0.11
Nodes (19): decode(), decode_single_byte(), decode_utf16(), detect_encoding(), encode(), EncodeError, Encoding, encoding_reports_character_and_utf8_byte_offset() (+11 more)

### Community 21 - "check-workspace/tests/cli.rs / .new()"
Cohesion: 0.24
Nodes (18): executable_accepts_workspace_and_local_dependency_packages(), executable_refuses_external_packages_without_changing_lock_bytes(), executable_refuses_git_source_and_preserves_lock_bytes(), executable_refuses_lock_that_needs_resolution_without_changing_bytes(), executable_refuses_non_member_path_packages_and_invalid_locks(), executable_refuses_registry_source_even_when_name_and_version_match_member(), executable_rejects_excluded_path_dependency_without_changing_lock(), executable_rejects_extra_arguments() (+10 more)

### Community 22 - "IndexedTree<T> / .node()"
Cohesion: 0.23
Nodes (4): IndexedTree<T>, Checkpoint, Default, TransactionalState

### Community 23 - "ImageError / decode_gif()"
Cohesion: 0.27
Nodes (13): allocate(), BitReader, decode_gif(), lzw(), pixel_len(), Reader, Reader<'a>, Option (+5 more)

### Community 24 - "src/png.rs / decode_png()"
Cohesion: 0.19
Nodes (20): ADAM7, chunk_crc(), decode_png(), Header, inflate_exact(), MAX_PNG_FIELD, max_sample(), paeth() (+12 more)

### Community 25 - "gif_bmp.rs / bmp_header()"
Cohesion: 0.24
Nodes (20): bmp_16_and_32_bit_rgb_use_expected_opaque_conventions(), bmp_bitfields_preserve_explicit_alpha(), bmp_header(), bmp_indexed_depths_and_palette_indices(), bmp_rgb_24_bottom_up_and_top_down(), bmp_rle8_runs_and_rle4_absolute_padding(), bmp_rle_delta_and_end_of_line_place_bottom_up_pixels(), bmp_truncation_incompatible_modes_and_limits_fail_cleanly() (+12 more)

### Community 26 - "Result / Op"
Cohesion: 0.17
Nodes (11): HistoryError<E>, Op, OrderedOp, OrderedState, Decoder, Display, Error, Formatter (+3 more)

### Community 27 - "Operation / Error"
Cohesion: 0.17
Nodes (11): generic_non_clone_state_rolls_back_when_apply_fails_mid_transaction(), IndexedTreeError, Operation, OperationError, PlainOp, PlainState, Display, Error (+3 more)

### Community 28 - ".mapped() / Result"
Cohesion: 0.18
Nodes (10): Add, concrete_selection_snapshots_restore_through_history_undo_and_redo(), PositionMap, Decoder, Display, Error, Formatter, Result (+2 more)

### Community 29 - "check-license/src/main.rs / check_tree()"
Cohesion: 0.18
Nodes (18): accepts_a_valid_header(), check_tree(), create_symlink(), ignores_generated_and_agent_work_directories(), main(), rejects_missing_and_incorrect_headers_and_reports_the_paths(), rejects_symlinked_files_and_directories_when_supported(), reports_invalid_utf8_instead_of_succeeding() (+10 more)

### Community 30 - "US-200 — Modèle de document texte / US-220 — Saisir du texte"
Cohesion: 0.15
Nodes (20): §6.3 — Modèle de document et opérations, F-COM-009 — Annuler / rétablir illimités (bornés par la mémoire), y compris après enregistrement, F-COM-016 — Presse-papiers : texte brut, HTML, format interne riche (copie entre documents sans perte), F-TXT-001 — Saisie de texte avec gestion complète du clavier français (touches mortes, AltGr, IME), F-TXT-004 — Insertion de caractères spéciaux, espaces insécables (insertion automatique avant `: ; ! ?` en français, paramétrable), F-TXT-005 — Correction automatique de base (guillemets français « », majuscule en début de phrase), F-TXT-034 — Liens hypertexte et signets, US-070 — Arbre de nœuds indexé (+12 more)

### Community 31 - "US-104 — Bibliothèque de composants d'interface / US-106 — Ruban"
Cohesion: 0.21
Nodes (17): §6.14 — Accessibilité, D13 — Accessibilité requise (cible RGAA 4.1 / WCAG 2.1 AA)., F-COM-018 — Interface à ruban (onglets par module), barre d'état, panneaux latéraux repliables, F-COM-019 — Thème clair / sombre / suivre le système ; respect du mode contraste élevé Windows, F-COM-020 — Raccourcis clavier calqués sur ceux de MS Office en français ; affectations de référence figées dans une table unique sans conflit (US-231), dont Ctrl+Maj+V pour coller le texte seul et Ctrl+Alt+C / Ctrl+Alt+V pour copier/appliquer la mise en forme, F-COM-021 — Navigation complète au clavier, compatibilité lecteurs d'écran (NVDA, Narrateur), US-104 — Bibliothèque de composants d'interface, US-105 — Thèmes (+9 more)

### Community 32 - "US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)"
Cohesion: 0.22
Nodes (17): §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe), §8 — Qualité et tests, F-TXT-032 — Images PNG, JPEG, GIF, BMP : insertion, redimensionnement, habillage (aligné sur le texte, carré, haut et bas, derrière, devant), US-001 — Initialiser le workspace, US-002 — Intégration continue, US-003 — Tests par images de référence, US-020 — Décompression deflate, US-021 — Compression deflate (+9 more)

### Community 33 - "US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles"
Cohesion: 0.21
Nodes (16): §5 — Formats de fichiers, §5.2 — Principes de fidélité, F-COM-003 — Enregistrer, Enregistrer sous (choix du format), F-TXT-023 — Listes à puces, numérotées, multiniveaux ; redémarrer/continuer la numérotation, F-TXT-040 — Objets non gérés (SmartArt, graphiques, OLE) : affichage de l'image de repli contenue dans le fichier et préservation à l'enregistrement, US-056 — Polices embarquées, US-252 — Listes à puces et numérotées, US-253 — Listes multiniveaux (+8 more)

### Community 34 - "US-240 — Mettre en forme les caractères depuis le ruban / §6.15 — Internationalisation"
Cohesion: 0.16
Nodes (15): §6.15 — Internationalisation, D12 — Interface en français en MVP, toutes les chaînes externalisées dès le départ., F-COM-010 — Rechercher / remplacer (texte, casse, mot entier), F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères, F-TXT-013 — Reproduire la mise en forme, F-TXT-014 — Effacer la mise en forme, US-013 — Casse et normalisation, US-102 — Internationalisation (+7 more)

### Community 35 - "tests/png.rs / zlib()"
Cohesion: 0.37
Nodes (12): applies_transparency_before_reducing_sample_precision(), chunk(), decodes_all_legal_color_depth_combinations_to_rgba8(), filtered_row(), png(), rejects_bad_palette_transparency_unknown_critical_and_filter(), rejects_dimensions_and_chunk_lengths_above_png_31_bit_bounds(), reverses_all_filters_and_adam7_passes() (+4 more)

### Community 36 - "US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base"
Cohesion: 0.29
Nodes (11): F-TXT-035 — Champs : PAGE, NUMPAGES, DATE, TIME, F-TXT-050 — Format de papier, orientation, marges, F-TXT-051 — Sauts de page et de section, sections avec mise en page différente, F-TXT-052 — En-têtes et pieds de page (première page différente, pages paires/impaires), F-TXT-053 — Numérotation des pages (format, point de départ), US-280 — Format, orientation et marges, US-281 — Sections, US-282 — En-têtes et pieds de page (+3 more)

### Community 37 - "F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule) / US-261 — Mise en page des tableaux : largeurs et cellules"
Cohesion: 0.42
Nodes (9): F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule), US-260 — Insérer un tableau, US-261 — Mise en page des tableaux : largeurs et cellules, US-262 — Mise en page des tableaux : pagination, US-263 — Modifier la structure d'un tableau, US-264 — Fusionner et fractionner des cellules, US-265 — Redimensionner lignes et colonnes, US-266 — Styles, bordures et propriétés de tableau (+1 more)

### Community 39 - "US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)"
Cohesion: 0.43
Nodes (7): §6.1 — Vue d'ensemble, §6.7 — Pont WASM ↔ JS (écrit à la main), US-090 — ABI WASM, US-091 — Protocole de messages, US-092 — Moteur dans un Web Worker, US-100 — Interface Host, US-103 — Capture de la saisie

### Community 40 - "format-reference/manifest.json / deterministic"
Cohesion: 0.29
Nodes (6): deterministic, fixtures, generated_by, schema_version, xml_semantics, zip_semantics

### Community 41 - "Transaction / Transaction<O>"
Cohesion: 0.47
Nodes (4): O, Vec, Transaction, Transaction<O>

### Community 42 - "fonts/manifest.json / asset_count"
Cohesion: 0.33
Nodes (5): asset_count, assets, license, schema_version, total_bytes

### Community 43 - "Point / Path"
Cohesion: 0.40
Nodes (6): AffineTransform, De Casteljau Subdivision, MAX_COORDINATE, Path, Point, Rect

### Community 44 - "atelier-compress / atelier-zip"
Cohesion: 0.40
Nodes (5): atelier-compress, atelier-image, atelier-model, atelier-unicode, atelier-zip

### Community 46 - "image-reference/manifest.json / cases"
Cohesion: 0.40
Nodes (4): cases, oracle, pngsuite_archive_sha256, pngsuite_archive_url

### Community 47 - "Encoding Enum / decode"
Cohesion: 0.50
Nodes (4): decode, detect_encoding, encode, Encoding Enum

### Community 48 - "Journal / SnapshotCodec"
Cohesion: 1.00
Nodes (3): Journal, SnapshotCodec, US-074: Recovery Journal

### Community 49 - "Selection / PositionMap"
Cohesion: 0.67
Nodes (3): PositionMap, Selection, US-073: Selection Model

### Community 51 - "IndexedTree<T> / bench_indexed_tree"
Cohesion: 0.67
Nodes (3): bench_indexed_tree, IndexedTree<T>, NodeId

## Knowledge Gaps
- **164 isolated node(s):** `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA`, `DIST_BASE`, `DIST_EXTRA` (+159 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 367 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **46 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `String` connect `String / xml/src/lib.rs` to `render/src/lib.rs / DisplayListError`, `zip/src/lib.rs / .new()`, `font/src/lib.rs / u16_at()`, `bench/src/lib.rs / bench/src/main.rs`, `svg.rs / render_svg()`, `CodecError / Result`, `check-workspace/src/main.rs / .value()`, `unicode/src/lib.rs / encode()`, `check-license/src/main.rs / check_tree()`?**
  _High betweenness centrality (0.387) - this node is a cross-community bridge._
- **Why does `BinaryValue` connect `CodecError / Result` to `String / xml/src/lib.rs`, `model/src/lib.rs / .new()`?**
  _High betweenness centrality (0.102) - this node is a cross-community bridge._
- **Why does `Command` connect `render/src/lib.rs / DisplayListError` to `String / xml/src/lib.rs`, `Point / geom/src/lib.rs`?**
  _High betweenness centrality (0.078) - this node is a cross-community bridge._
- **What connects `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA` to the rest of the system?**
  _164 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `String / xml/src/lib.rs` be split into smaller, more focused modules?**
  _Cohesion score 0.06194590954999439 - nodes in this community are weakly interconnected._
- **Should `render/src/lib.rs / DisplayListError` be split into smaller, more focused modules?**
  _Cohesion score 0.07232323232323232 - nodes in this community are weakly interconnected._
- **Should `zip/src/lib.rs / .new()` be split into smaller, more focused modules?**
  _Cohesion score 0.06787878787878789 - nodes in this community are weakly interconnected._
Références AST externes non résolues exclues du graphe : 64 (détail dans unresolved-structural-references.json). Le graphe simple peut fusionner plusieurs relations entre les mêmes symboles ; les relations brutes restent dans .graphify_extract.json.

## Périmètre de cette mise à jour

La carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --cargo` a réussi après mise en cache des documents et exclusion du runtime agent ; les API locales de Graphify régénèrent ce rapport et ses libellés. Cargo.toml est couvert par les métadonnées de ses 31 paquets, même si son extraction AST produit zéro nœud. Cette génération locale par API n’utilise pas de LLM ; les coûts des extractions CLI restent dans cost.json.
