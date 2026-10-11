# Graph Report - mon-bureau  (2026-10-11)

## Corpus Check
- Large corpus: 178 files · ~922,790 words. Semantic extraction will be expensive (many Claude tokens). Consider running on a subfolder.

## Summary
- 2442 nodes · 6205 edges · 210 communities (93 shown, 51 thin omitted)
- Extraction: 99% EXTRACTED · 1% INFERRED · 0% AMBIGUOUS · INFERRED: 52 edges (avg confidence: 0.85)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- xml/src/lib.rs / XmlError
- zip/src/lib.rs / .new()
- render/src/lib.rs / DisplayListError
- Point / Path
- src/layout.rs / LayoutError
- compress/src/lib.rs / .push()
- src/manager.rs / ManagerError
- generated.rs / canonical_combining_class_DATA
- svg.rs / render_svg()
- ucd-gen/src/lib.rs / err()
- outline.rs / OutlineError
- NodeId / selection.rs
- CodecError / Result
- core-wasm/src/lib.rs / AllocationRegistry
- SPEC.md v0.2 / US-113 — Pont natif ↔ JS
- bench/src/lib.rs / .new()
- US-212 — Pagination / US-201 — Moteur de styles
- check-workspace/src/main.rs / .value()
- outlines.rs / table_range()
- history.rs / .apply()
- protocol.rs / encode()
- model/src/lib.rs / .new()
- ShapeError / .finish()
- tests/wasm-protocol.mjs / hex()
- §6.13 — Robustesse et protection des données / §5.1 — Matrice
- wasm-abi.mjs / binary
- §6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout
- journal.rs / snapshot_state()
- T / IndexedTree
- unicode/src/lib.rs / encode()
- font/src/lib.rs / FontFace
- decode_bmp() / bmp.rs
- properties.rs / lookup()
- check-workspace/tests/cli.rs / .new()
- web/wasm-protocol.mjs / wasm-protocol-boundaries.mjs
- src/png.rs / decode_png()
- gif_bmp.rs / bmp_header()
- Result / ResourceState
- wasm-loader.mjs / dispatch()
- ImageError / decode_gif()
- Error / Result
- .new() / snapshot()
- IndexedTree<T> / .node()
- US-200 — Modèle de document texte / US-220 — Saisir du texte
- check-license/src/main.rs / check_tree()
- u16_at() / Option
- src/segmentation.rs / grapheme_boundaries()
- Hash / ucd_reference.rs
- bench/src/main.rs / run()
- US-104 — Bibliothèque de composants d'interface / US-106 — Ruban
- US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)
- Operation / .apply()
- Preparation of the Demonstrator - 2026-10-11 / GPT-6 Luna
- US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles
- .parse_collection() / fixture()
- Option / mapped()
- OffsetError / BoundaryIndex<'a>
- US-240 — Mettre en forme les caractères depuis le ruban / §6.15 — Internationalisation
- font-layout-reference/manifest.json / cases
- FontError / parse_cmap()
- tests/png.rs / zlib()
- History<O, Selection> / HistoryError
- tests/layout.rs / put16()
- Item / src/line_break.rs
- bench/tests/cli.rs / stdout_csv()
- opentype_fixtures.rs / FONT_0_12
- bench_indexed_tree.rs / measure()
- US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base
- String / run()
- latin.rs / shaper()
- Segments / Option
- atelier-compress / atelier-font
- image/src/lib.rs / DecodeLimits
- line_breaks() / tests/line_break.rs
- 17.0.0/manifest.json / license
- F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule) / US-261 — Mise en page des tableaux : largeurs et cellules
- manager_reference.rs / os2_variant()
- IndexedTreeError / OperationError
- unicode-reference/manifest.json / checksum_algorithm
- US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)
- font-outline-reference/manifest.json / cases
- format-reference/manifest.json / deterministic
- TransactionalState / Checkpoint
- reference.rs / complete_harfbuzz_corpus_matches_shaper_at_fractional_size_and_scale()
- fonts/manifest.json / asset_count
- Point / Path
- tests/embedded.rs / catalog_has_each_family_weight_and_slant_once()
- check-wasm-size.mjs / artifactPath
- image-reference/manifest.json / cases
- JournalError<E> / .fmt()
- Encoding Enum / decode
- Journal / SnapshotCodec
- Selection / PositionMap
- .fmt() / Formatter
- NameAlias / NAME_ALIASES
- IndexedTree<T> / bench_indexed_tree
- check-assets.ps1 / Assert-Asset()
- check-ucd.ps1 / Require()
- ATOP Binary Format / US-071 Transactions
- DECOMPOSITIONS / Option
- atelier-geom / GeometryError
- Font Layout Reference README / Test Resources README
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
- atelier-lua
- atelier-raster
- atelier-server
- atelier-sheet-model
- atelier-slide-model
- atelier-text-layout
- atelier-text-model
- atelier-ucd-gen
- atelier-xml
- Mixed UTF-8 Corpus
- Repetitive Corpus
- Caladea Font License
- Carlito Font License
- Liberation Font License
- UCD README
- Development Documentation
- US-011: Segmentation Unicode
- US-053: Crénage et Ligatures
- shape_glyphs
- US-080 — Liste d’affichage par page
- FontTools
- CI Workflow
- Technical Architecture
- Atelier Vision
- UAX #29
- Transaction Journal
- Atomic Save
- Accessibility Mirror

## God Nodes (most connected - your core abstractions)
1. `SPEC.md v0.2` - 80 edges
2. `DisplayListError` - 52 edges
3. `XmlError` - 49 edges
4. `Point` - 45 edges
5. `LayoutError` - 40 edges
6. `CodecError` - 39 edges
7. `GeometryError` - 32 edges
8. `ImageError` - 30 edges
9. `IndexedTree<T>` - 29 edges
10. `Reader<R>` - 29 edges

## Surprising Connections (you probably didn't know these)
- `csv_field()` --references--> `String`  [EXTRACTED]
  tools/bench/src/lib.rs → crates/model/src/lib.rs
- `US-001 — Initialiser le workspace` --references--> `Zero Dependency Rule (D11)`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `US-309 — Modèles et documents à macros` --references--> `§5.1 — Matrice`  [EXTRACTED]
  US-V0-V1.md → SPEC.md
- `InflateError` --implements--> `Error`  [EXTRACTED]
  crates/compress/src/lib.rs → tools/ucd-gen/src/lib.rs
- `CompressError` --implements--> `Error`  [EXTRACTED]
  crates/compress/src/lib.rs → tools/ucd-gen/src/lib.rs

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Foundation Milestone Components** — us_010, us_051, us_090_wasm_abi [EXTRACTED 0.90]
- **Text Bridge Milestone Components** — us_011, us_053, us_091 [EXTRACTED 0.90]
- **ATDL Serialization and Validation** — atelier_render_displaylist, atelier_render_atdl_format, atelier_render_limits [EXTRACTED 1.00]
- **Text Encoding API** — docs_us_014_encodings_encoding, docs_us_014_encodings_detect_encoding, docs_us_014_encodings_decode, docs_us_014_encodings_encode [EXTRACTED 1.00]
- **Geometry Primitives** — docs_us_060_geometry_point, docs_us_060_geometry_rect, docs_us_060_geometry_path, docs_us_060_geometry_affinetransform [EXTRACTED 1.00]
- **Latin Shaping Pipeline** — docs_us_054_latin_shaping_latinshaper, docs_us_054_latin_shaping_shape_text, docs_us_054_latin_shaping_shape_glyphs [EXTRACTED 1.00]
- **OFL Licensed Fonts** — data_fonts_licenses_caladea_ofl, data_fonts_licenses_carlito_ofl, data_fonts_licenses_liberation_ofl [EXTRACTED 1.00]
- **Atelier Model Core Logic** — atelier_model_selection_selection, atelier_model_selection_positionmap, atelier_model_journal_journal, atelier_model_journal_snapshotcodec [INFERRED 0.80]

## Communities (210 total, 51 thin omitted)

### Community 0 - "xml/src/lib.rs / XmlError"
Cohesion: 0.06
Nodes (78): all_byte_boundaries_of_utf16_and_bom_are_incremental(), Attribute, bounded_push(), comments_and_pi_normalize_crlf_and_cr(), constructs_and_dtd_do_not_expand(), Decoder, Decoder<R>, Diagnostic (+70 more)

### Community 1 - "zip/src/lib.rs / .new()"
Cohesion: 0.06
Nodes (80): FontFace, atelier-unicode, applies_limits_before_read_all_and_detects_partial_archive(), Archive, Archive<'a>, canonical_name(), CENTRAL, central_entry_count() (+72 more)

### Community 2 - "render/src/lib.rs / DisplayListError"
Cohesion: 0.07
Nodes (67): arbitrary_bytes_never_panic(), binary_header(), Color, Command, complex(), cumulative_path_and_glyph_budgets_are_enforced(), decode_command(), decoder_checks_cumulative_budgets_before_allocating() (+59 more)

### Community 3 - "Point / Path"
Cohesion: 0.06
Nodes (59): assert_same_contours(), assign_contour(), close_enough(), embedded_outlines_match_fonttools_reference_geometry(), FLATTEN_TOLERANCE, normalized_points(), parse_reference_path(), read_point() (+51 more)

### Community 4 - "src/layout.rs / LayoutError"
Cohesion: 0.11
Nodes (64): add(), apply_gpos_lookup(), apply_gsub_lookup(), apply_legacy_kern(), b16(), Budget, class_def(), coverage() (+56 more)

### Community 5 - "compress/src/lib.rs / .push()"
Cohesion: 0.07
Nodes (48): adler32(), BitWriter, BitWriter<'a>, Code, CODE_ORDER, CodeOp, compress(), compress_with_format() (+40 more)

### Community 6 - "src/manager.rs / ManagerError"
Cohesion: 0.07
Nodes (35): be_u16(), Entry, face_style(), family_names(), FontId, FontManager, FontManager<'a>, FontManagerLimits (+27 more)

### Community 7 - "generated.rs / canonical_combining_class_DATA"
Cohesion: 0.03
Nodes (60): canonical_combining_class_DATA, canonical_combining_class_PAGES, canonical_combining_class_VALUES, CANONICAL_COMPOSITIONS, CASE_FOLD, case_ignorable_DATA, case_ignorable_PAGES, case_ignorable_VALUES (+52 more)

### Community 8 - "svg.rs / render_svg()"
Cohesion: 0.10
Nodes (44): active_clip_id(), attribute_values(), BASE64, base64_images_and_escaped_metadata_are_inert_and_valid_xml(), base64_len(), begin_draw(), BoundedString, ClipDef (+36 more)

### Community 9 - "ucd-gen/src/lib.rs / err()"
Cohesion: 0.13
Nodes (40): ConditionalMapping, cp(), Data, emit(), emit_page_table(), err(), fields(), generate() (+32 more)

### Community 10 - "outline.rs / OutlineError"
Cohesion: 0.12
Nodes (33): atelier-geom, ATDL Binary Format, Command, DisplayList, atelier_render::limits, be16(), be32(), Budget (+25 more)

### Community 11 - "NodeId / selection.rs"
Cohesion: 0.11
Nodes (28): IndexedTreeOperation, Node<T>, NodeId, Add, Affinity, concrete_selection_snapshots_restore_through_history_undo_and_redo(), delete_collapses_inclusive_end_and_shifts_after(), insert_respects_affinity_and_caret_defaults_after() (+20 more)

### Community 12 - "CodecError / Result"
Cohesion: 0.13
Nodes (11): BinaryValue, CodecError, Decoder, Decoder<'a>, Encoder, i32, Result, Self (+3 more)

### Community 13 - "core-wasm/src/lib.rs / AllocationRegistry"
Cohesion: 0.12
Nodes (29): alloc(), allocation_limits_are_checked_before_allocation(), AllocationRegistry, dispatch(), dispatch_inner(), exact_pointer_and_length_are_required_for_release(), exact_take_transfers_ownership_and_updates_totals(), free() (+21 more)

### Community 14 - "SPEC.md v0.2 / US-113 — Pont natif ↔ JS"
Cohesion: 0.08
Nodes (37): §1 — Vision, §10 — Risques, §6.8 — Shell desktop Windows, §7.2 — Desktop, §9 — Licence et propriété, Zero Dependency Rule (D11), D6 — Distribution : exécutable **portable** d'abord ; installeur et mise à jour automatique plus tard., D9 — Shell desktop : **couche maison Win32 + WebView2** (pas de Tauri). (+29 more)

### Community 15 - "bench/src/lib.rs / .new()"
Cohesion: 0.13
Nodes (27): FnMut, accepts_exactly_ten_percent_and_flags_more(), BenchmarkCase, BenchmarkResult, compare(), csv_field(), default_registry(), MAX_ITERATIONS (+19 more)

### Community 16 - "US-212 — Pagination / US-201 — Moteur de styles"
Cohesion: 0.09
Nodes (33): §6.16 — Objectifs de performance (machine de référence : PC grand public de 2020, 8 Go), §6.5 — Mise en page (traitement de texte), F-TXT-002 — Sélection souris (clic, double-clic mot, triple-clic paragraphe, glisser, Maj+clic) et clavier (Maj+flèches, Ctrl+Maj+flèches…), F-TXT-003 — Glisser-déposer de texte dans le document, F-TXT-011 — Paragraphe : alignement (gauche, centré, droite, justifié), retraits, première ligne, interligne, espacement avant/après, tabulations (gauche, centre, droite, décimale, points de suite), F-TXT-012 — Paragraphe : bordures, trame de fond, veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant, F-TXT-020 — Styles de paragraphe, de caractère, de tableau, de liste ; héritage ; galerie de styles, F-TXT-021 — Jeu de styles par défaut équivalent à Word (Normal, Titre 1–9, Titre, Citation…) (+25 more)

### Community 17 - "check-workspace/src/main.rs / .value()"
Cohesion: 0.15
Nodes (17): as_array(), as_object(), as_string(), check(), Json, JsonParser, main(), Package (+9 more)

### Community 18 - "outlines.rs / table_range()"
Cohesion: 0.18
Nodes (24): EMBEDDED_FONTS, EmbeddedFont, EmbeddedFontStyle, carlito_regular(), composite_children(), contradictory_composite_transforms_return_typed_errors(), corrupted_loca_and_composite_cycles_fail_cleanly(), expanded_stats() (+16 more)

### Community 19 - "history.rs / .apply()"
Cohesion: 0.18
Nodes (22): apply_undo_redo_restore_state_and_selection(), budget_rejects_oversized_and_evicts_oldest_only_after_success(), EditKind, empty_edit_and_failed_edit_preserve_redo_and_selection(), Entry, grouped_undo_and_redo_failures_restore_state_and_preserve_history(), History, Op (+14 more)

### Community 20 - "protocol.rs / encode()"
Cohesion: 0.12
Nodes (21): all_types_round_trip_and_header_is_little_endian(), copies_payload_and_enforces_size_limit(), decode(), Direction, encode(), HEADER_BYTES, MAGIC, MAX_MESSAGE_BYTES (+13 more)

### Community 21 - "model/src/lib.rs / .new()"
Cohesion: 0.17
Nodes (23): assert_matches(), deleting_a_two_child_node_preserves_successor_identity(), deterministic_pseudo_random_transactions_round_trip(), failed_operation_application_rolls_back_prior_mutations(), identity_exhaustion_is_atomic(), insert_positions_and_stable_ids(), invalid_positions_do_not_mutate(), MAX_OPERATION_BYTES (+15 more)

### Community 22 - "ShapeError / .finish()"
Cohesion: 0.20
Nodes (17): GlyphRun, LatinShaper<'face, 'font>, PositionedGlyph, rounded_ratio(), Default, Display, Formatter, From (+9 more)

### Community 23 - "tests/wasm-protocol.mjs / hex()"
Cohesion: 0.08
Nodes (24): backing, bufferFrame, controlPayload, decodedBufferSubview, decodedSubview, encodedSubview, frameBacking, golden (+16 more)

### Community 24 - "§6.13 — Robustesse et protection des données / §5.1 — Matrice"
Cohesion: 0.11
Nodes (28): §11 — Points ouverts (non bloquants pour la validation), §5.1 — Matrice, §6.13 — Robustesse et protection des données, D2 — Format natif OOXML (docx/xlsx/pptx). Lecture des formats binaires anciens (doc/xls/ppt) et ODF (odt/ods/odp)., F-COM-006 — Sauvegarde de récupération automatique autonome (instantané de base + journal, intervalle paramétrable, défaut 2 min) et proposition de restauration au redémarrage après crash ; récupération jusqu'au dernier point durable (§6.13), F-COM-014 — Export PDF (texte sélectionnable, polices incorporées en sous-ensemble, liens, signets depuis les titres), F-COM-017 — Propriétés du document (titre, auteur, sujet, mots-clés, dates), F-TXT-065 — Protection : ouverture par mot de passe (chiffrement ECMA-376 Agile), restriction de modification (+20 more)

### Community 25 - "wasm-abi.mjs / binary"
Cohesion: 0.07
Nodes (25): binary, emptyEchoResult, growthDescriptor, growthDescriptorPtr, growthInput, growthPayload, growthPayloadLen, growthPayloadPtr (+17 more)

### Community 26 - "§6.4 — Texte et polices / US-120 — Démonstrateur de bout en bout"
Cohesion: 0.14
Nodes (27): §6 — Architecture technique, §6.12 — PDF, §6.4 — Texte et polices, §6.6 — Rendu, F-COM-012 — Zoom 10 %–500 %, ajustement page/largeur, F-TXT-006 — Statistiques (mots, caractères, paragraphes, pages), US-010 — Générateur de tables Unicode, US-011 — Segmentation en graphèmes et en mots (+19 more)

### Community 27 - "journal.rs / snapshot_state()"
Cohesion: 0.15
Nodes (25): C, DEFAULT_MAX_ENTRIES, DEFAULT_MAX_JOURNAL_BYTES, DEFAULT_MAX_SNAPSHOT_BYTES, encode_snapshot(), ENTRY_FIXED_BYTES, ENTRY_PREFIX_BYTES, FORMAT_VERSION (+17 more)

### Community 28 - "T / IndexedTree"
Cohesion: 0.20
Nodes (11): ArenaSlot, empty_page(), IndexedTree, IndexedTreeOperation<T>, Node, RadixArena, RadixArena<T>, RadixPage (+3 more)

### Community 29 - "unicode/src/lib.rs / encode()"
Cohesion: 0.12
Nodes (18): decode(), decode_single_byte(), decode_utf16(), detect_encoding(), encode(), EncodeError, Encoding, encoding_reports_character_and_utf8_byte_offset() (+10 more)

### Community 30 - "font/src/lib.rs / FontFace"
Cohesion: 0.10
Nodes (18): cmap_rank(), CmapSubtable, FontFace, is_unicode_scalar(), is_variation_selector(), MAX_CMAP_GROUPS, MAX_CMAP_RECORDS, MAX_CMAP_VALIDATION_WORK (+10 more)

### Community 31 - "decode_bmp() / bmp.rs"
Cohesion: 0.17
Nodes (17): allocate(), BI_BITFIELDS, BI_RGB, BI_RLE4, BI_RLE8, component(), decode_bmp(), decode_rle() (+9 more)

### Community 32 - "properties.rs / lookup()"
Cohesion: 0.15
Nodes (21): CONDITIONAL_CASE, canonical_combining_class(), conditional_casing_rules(), ConditionalCasingRule, east_asian_width(), general_category(), grapheme_cluster_break_property(), indic_conjunct_break_property() (+13 more)

### Community 33 - "check-workspace/tests/cli.rs / .new()"
Cohesion: 0.25
Nodes (18): executable_accepts_workspace_and_local_dependency_packages(), executable_refuses_external_packages_without_changing_lock_bytes(), executable_refuses_git_source_and_preserves_lock_bytes(), executable_refuses_lock_that_needs_resolution_without_changing_bytes(), executable_refuses_non_member_path_packages_and_invalid_locks(), executable_refuses_registry_source_even_when_name_and_version_match_member(), executable_rejects_excluded_path_dependency_without_changing_lock(), executable_rejects_extra_arguments() (+10 more)

### Community 34 - "web/wasm-protocol.mjs / wasm-protocol-boundaries.mjs"
Cohesion: 0.19
Nodes (19): decoded, frame, payload, checkDirection(), checkEmptyPayload(), checkId(), checkPayload(), checkType() (+11 more)

### Community 35 - "src/png.rs / decode_png()"
Cohesion: 0.19
Nodes (20): ADAM7, chunk_crc(), decode_png(), Header, inflate_exact(), MAX_PNG_FIELD, max_sample(), paeth() (+12 more)

### Community 36 - "gif_bmp.rs / bmp_header()"
Cohesion: 0.24
Nodes (20): bmp_16_and_32_bit_rgb_use_expected_opaque_conventions(), bmp_bitfields_preserve_explicit_alpha(), bmp_header(), bmp_indexed_depths_and_palette_indices(), bmp_rgb_24_bottom_up_and_top_down(), bmp_rle8_runs_and_rle4_absolute_padding(), bmp_rle_delta_and_end_of_line_place_bottom_up_pixels(), bmp_truncation_incompatible_modes_and_limits_fail_cleanly() (+12 more)

### Community 37 - "Result / ResourceState"
Cohesion: 0.18
Nodes (11): AddValue, ResourceCodec, ResourceState, Checkpoint, Decoder, Result, S, Self (+3 more)

### Community 38 - "wasm-loader.mjs / dispatch()"
Cohesion: 0.17
Nodes (14): directDispatch(), createWasmEngine(), dispatch(), resetAfterTrap(), EngineAbiError, EngineInputError, EngineMemoryError, EngineResetError (+6 more)

### Community 39 - "ImageError / decode_gif()"
Cohesion: 0.29
Nodes (12): allocate(), BitReader, decode_gif(), lzw(), pixel_len(), Reader, Reader<'a>, Option (+4 more)

### Community 40 - "Error / Result"
Cohesion: 0.17
Nodes (11): HistoryError<E>, OrderedOp, OrderedState, Decoder, Display, Formatter, Result, Self (+3 more)

### Community 41 - ".new() / snapshot()"
Cohesion: 0.26
Nodes (13): corrupt_tail_keeps_last_applied_state_and_stops(), empty_document_snapshot_and_empty_journal_restore_exact_base(), every_truncated_tail_is_bounded_and_never_applied_partially(), failed_application_keeps_previous_state(), insert(), Journal, mismatches_and_hostile_size_limits_are_rejected(), Infallible (+5 more)

### Community 42 - "IndexedTree<T> / .node()"
Cohesion: 0.34
Nodes (3): IndexedTree<T>, Default, Option

### Community 43 - "US-200 — Modèle de document texte / US-220 — Saisir du texte"
Cohesion: 0.15
Nodes (20): §6.3 — Modèle de document et opérations, F-COM-009 — Annuler / rétablir illimités (bornés par la mémoire), y compris après enregistrement, F-COM-016 — Presse-papiers : texte brut, HTML, format interne riche (copie entre documents sans perte), F-TXT-001 — Saisie de texte avec gestion complète du clavier français (touches mortes, AltGr, IME), F-TXT-004 — Insertion de caractères spéciaux, espaces insécables (insertion automatique avant `: ; ! ?` en français, paramétrable), F-TXT-005 — Correction automatique de base (guillemets français « », majuscule en début de phrase), F-TXT-034 — Liens hypertexte et signets, US-070 — Arbre de nœuds indexé (+12 more)

### Community 44 - "check-license/src/main.rs / check_tree()"
Cohesion: 0.19
Nodes (18): accepts_a_valid_header(), check_tree(), create_symlink(), ignores_generated_and_agent_work_directories(), main(), rejects_missing_and_incorrect_headers_and_reports_the_paths(), rejects_symlinked_files_and_directories_when_supported(), reports_invalid_utf8_instead_of_succeeding() (+10 more)

### Community 45 - "u16_at() / Option"
Cohesion: 0.18
Nodes (11): FontFace<'a>, FontMetrics, HorizontalMetric, i16_at(), lookup12(), lookup4(), lookup6(), Option (+3 more)

### Community 46 - "src/segmentation.rs / grapheme_boundaries()"
Cohesion: 0.21
Nodes (14): BoundaryIndex, grapheme_boundaries(), grapheme_break(), Self, Vec, word_boundaries(), word_break(), check_corpus() (+6 more)

### Community 47 - "Hash / ucd_reference.rs"
Cohesion: 0.16
Nodes (12): exhaustive_unicode_17_reference_fingerprints(), fingerprint_fields(), fingerprint_scalar(), Hash, OFFSET, PRIME, Display, Option (+4 more)

### Community 48 - "bench/src/main.rs / run()"
Cohesion: 0.20
Nodes (15): cli_lists_and_measures_multiple_real_cases(), cli_preserves_reference_and_does_not_create_output_for_invalid_reference(), cli_refuses_malformed_reference_without_panicking(), cli_reports_unwritable_output_and_incomplete_reference(), main(), Options, parse_args(), reject_same_file() (+7 more)

### Community 49 - "US-104 — Bibliothèque de composants d'interface / US-106 — Ruban"
Cohesion: 0.21
Nodes (17): §6.14 — Accessibilité, D13 — Accessibilité requise (cible RGAA 4.1 / WCAG 2.1 AA)., F-COM-018 — Interface à ruban (onglets par module), barre d'état, panneaux latéraux repliables, F-COM-019 — Thème clair / sombre / suivre le système ; respect du mode contraste élevé Windows, F-COM-020 — Raccourcis clavier calqués sur ceux de MS Office en français ; affectations de référence figées dans une table unique sans conflit (US-231), dont Ctrl+Maj+V pour coller le texte seul et Ctrl+Alt+C / Ctrl+Alt+V pour copier/appliquer la mise en forme, F-COM-021 — Navigation complète au clavier, compatibilité lecteurs d'écran (NVDA, Narrateur), US-104 — Bibliothèque de composants d'interface, US-105 — Thèmes (+9 more)

### Community 50 - "US-001 — Initialiser le workspace / §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe)"
Cohesion: 0.22
Nodes (17): §6.2 — Organisation du dépôt (workspace Cargo, aucune dépendance externe), §8 — Qualité et tests, F-TXT-032 — Images PNG, JPEG, GIF, BMP : insertion, redimensionnement, habillage (aligné sur le texte, carré, haut et bas, derrière, devant), US-001 — Initialiser le workspace, US-002 — Intégration continue, US-003 — Tests par images de référence, US-020 — Décompression deflate, US-021 — Compression deflate (+9 more)

### Community 51 - "Operation / .apply()"
Cohesion: 0.19
Nodes (7): generic_non_clone_state_rolls_back_when_apply_fails_mid_transaction(), Operation, PlainOp, PlainState, O, S, Transaction<O>

### Community 52 - "Preparation of the Demonstrator - 2026-10-11 / GPT-6 Luna"
Cohesion: 0.17
Nodes (16): Preparation of the Demonstrator - 2026-10-11, US-091: Binary Message Protocol v1, GPT-6 Luna, US-010: Unicode UCD Tables, US-011: Segmentation, US-012: Unicode Line Breaking, US-051: TrueType Outlines, US-053: Kerning and Ligatures (+8 more)

### Community 53 - "US-307 — Enregistrer en docx / US-301 — Lecture docx : texte, propriétés et styles"
Cohesion: 0.21
Nodes (16): §5 — Formats de fichiers, §5.2 — Principes de fidélité, F-COM-003 — Enregistrer, Enregistrer sous (choix du format), F-TXT-023 — Listes à puces, numérotées, multiniveaux ; redémarrer/continuer la numérotation, F-TXT-040 — Objets non gérés (SmartArt, graphiques, OLE) : affichage de l'image de repli contenue dans le fichier et préservation à l'enregistrement, US-056 — Polices embarquées, US-252 — Listes à puces et numérotées, US-253 — Listes multiniveaux (+8 more)

### Community 54 - ".parse_collection() / fixture()"
Cohesion: 0.24
Nodes (13): bounds_alias_amplification_in_format14(), conflicting_cmap(), fixture(), preferred_cmap_is_used_exclusively(), put16(), put32(), reads_cmap_metrics_and_repeated_horizontal_advance(), replace_table() (+5 more)

### Community 55 - "Option / mapped()"
Cohesion: 0.27
Nodes (15): canonical_composition(), case_fold(), case_fold_turkic(), decomposition(), full_lowercase(), full_titlecase(), full_uppercase(), mapped() (+7 more)

### Community 56 - "OffsetError / BoundaryIndex<'a>"
Cohesion: 0.34
Nodes (5): BoundaryIndex<'a>, OffsetError, Display, Formatter, Result

### Community 57 - "US-240 — Mettre en forme les caractères depuis le ruban / §6.15 — Internationalisation"
Cohesion: 0.16
Nodes (15): §6.15 — Internationalisation, D12 — Interface en français en MVP, toutes les chaînes externalisées dès le départ., F-COM-010 — Rechercher / remplacer (texte, casse, mot entier), F-TXT-010 — Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères, F-TXT-013 — Reproduire la mise en forme, F-TXT-014 — Effacer la mise en forme, US-013 — Casse et normalisation, US-102 — Internationalisation (+7 more)

### Community 58 - "font-layout-reference/manifest.json / cases"
Cohesion: 0.13
Nodes (14): cases, cluster_unit, direction, faces, features, file, fonttools_version, harfbuzz_version (+6 more)

### Community 59 - "FontError / parse_cmap()"
Cohesion: 0.38
Nodes (12): FontError, parse_cmap(), Display, Formatter, Result, u32_at(), validate12(), validate14() (+4 more)

### Community 60 - "tests/png.rs / zlib()"
Cohesion: 0.37
Nodes (12): applies_transparency_before_reducing_sample_precision(), chunk(), decodes_all_legal_color_depth_combinations_to_rgba8(), filtered_row(), png(), rejects_bad_palette_transparency_unknown_critical_and_filter(), rejects_dimensions_and_chunk_lengths_above_png_31_bit_bounds(), reverses_all_filters_and_adam7_passes() (+4 more)

### Community 61 - "History<O, Selection> / HistoryError"
Cohesion: 0.15
Nodes (7): History<O, Selection>, HistoryAction, HistoryError, E, Infallible, E, TransactionError

### Community 62 - "tests/layout.rs / put16()"
Cohesion: 0.38
Nodes (10): BASE, gdef_mark(), gpos_kern_feature_takes_precedence_over_legacy_kern(), gpos_pair(), gsub_liga(), legacy_kern(), ligature_merges_components_and_ignore_marks_keeps_the_mark(), put16() (+2 more)

### Community 63 - "Item / src/line_break.rs"
Cohesion: 0.27
Nodes (10): aksara(), BreakKind, east_asian(), is_alpha(), is_hard(), Item, LineBreak, permits() (+2 more)

### Community 64 - "bench/tests/cli.rs / stdout_csv()"
Cohesion: 0.27
Nodes (10): executable_list_and_case_selection_return_only_requested_rows(), executable_rejects_extreme_iteration_counts_without_panicking(), executable_rejects_same_output_and_reference_without_changing_baseline(), executable_stdout_is_csv_with_and_without_reference(), Output, PathBuf, Vec, run() (+2 more)

### Community 65 - "opentype_fixtures.rs / FONT_0_12"
Cohesion: 0.18
Nodes (7): FONT_0_12, FONT_0_4, FONT_0_6, FONT_1_12, FONT_1_4, FONT_1_6, TTC

### Community 66 - "bench_indexed_tree.rs / measure()"
Cohesion: 0.25
Nodes (10): fixture(), FIXTURE_SIZE, insertion_positions(), INSERTIONS, LIMIT, main(), measure(), REQUIRED_SAMPLES (+2 more)

### Community 67 - "US-304 — Lecture docx : sections, en-têtes, champs, liens / US-284 — Champs de base"
Cohesion: 0.29
Nodes (11): F-TXT-035 — Champs : PAGE, NUMPAGES, DATE, TIME, F-TXT-050 — Format de papier, orientation, marges, F-TXT-051 — Sauts de page et de section, sections avec mise en page différente, F-TXT-052 — En-têtes et pieds de page (première page différente, pages paires/impaires), F-TXT-053 — Numérotation des pages (format, point de départ), US-280 — Format, orientation et marges, US-281 — Sections, US-282 — En-têtes et pieds de page (+3 more)

### Community 68 - "String / run()"
Cohesion: 0.42
Nodes (9): direction(), main(), parse_hex(), ExitCode, Result, Vec, run(), to_hex() (+1 more)

### Community 69 - "latin.rs / shaper()"
Cohesion: 0.38
Nodes (9): fractional_scaling_rounds_ties_away_from_zero(), fractional_size_and_non_default_scale_match_independent_rational_result(), generic_glyphs_preserve_clusters_and_spacing_groups(), output_glyph_and_total_advance_overflow_are_reported(), round_away(), shaper(), spacing_is_added_between_graphemes_not_inside_combining_or_emoji_sequences(), text_maps_scalars_and_reports_utf8_grapheme_boundaries() (+1 more)

### Community 70 - "Segments / Option"
Cohesion: 0.24
Nodes (6): Iterator, Option, Segments, DoubleEndedIterator, ExactSizeIterator, FusedIterator

### Community 71 - "atelier-compress / atelier-font"
Cohesion: 0.22
Nodes (9): atelier-compress, atelier-font, atelier-geom, atelier-image, atelier-model, atelier-render, atelier-shape, atelier-unicode (+1 more)

### Community 72 - "image/src/lib.rs / DecodeLimits"
Cohesion: 0.22
Nodes (5): DecodeLimits, Image, Default, Self, Vec

### Community 73 - "line_breaks() / tests/line_break.rs"
Cohesion: 0.28
Nodes (6): line_breaks(), Vec, official_boundaries_survive_lb9_marks_in_all_rule_contexts(), long_context_runs_are_linear_and_ordered(), mandatory_controls_crlf_spaces_and_utf8_offsets(), unicode_17_line_break_test_corpus()

### Community 74 - "17.0.0/manifest.json / license"
Cohesion: 0.22
Nodes (8): archive_sha256, files, license, path, sha256, source_url, source_url, unicode_version

### Community 75 - "F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule) / US-261 — Mise en page des tableaux : largeurs et cellules"
Cohesion: 0.42
Nodes (9): F-TXT-030 — Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule), US-260 — Insérer un tableau, US-261 — Mise en page des tableaux : largeurs et cellules, US-262 — Mise en page des tableaux : pagination, US-263 — Modifier la structure d'un tableau, US-264 — Fusionner et fractionner des cellules, US-265 — Redimensionner lignes et colonnes, US-266 — Styles, bordures et propriétés de tableau (+1 more)

### Community 76 - "manager_reference.rs / os2_variant()"
Cohesion: 0.43
Nodes (7): css_search_uses_each_weight_interval_and_exact_match(), invalid_weight_and_face_index_fail_without_consuming_ids_or_bytes(), oblique_metadata_requires_version_four_and_does_not_come_from_name(), os2_variant(), regular(), Vec, uncovered_family_falls_through_before_source_and_style_matching()

### Community 77 - "IndexedTreeError / OperationError"
Cohesion: 0.36
Nodes (5): IndexedTreeError, OperationError, Display, Formatter, TransactionError<E>

### Community 78 - "unicode-reference/manifest.json / checksum_algorithm"
Cohesion: 0.25
Nodes (7): checksum_algorithm, fields, file, scalars, sha256, source, unicode_version

### Community 79 - "US-092 — Moteur dans un Web Worker / §6.7 — Pont WASM ↔ JS (écrit à la main)"
Cohesion: 0.43
Nodes (7): §6.1 — Vue d'ensemble, §6.7 — Pont WASM ↔ JS (écrit à la main), US-090 — ABI WASM, US-091 — Protocole de messages, US-092 — Moteur dans un Web Worker, US-100 — Interface Host, US-103 — Capture de la saisie

### Community 80 - "font-outline-reference/manifest.json / cases"
Cohesion: 0.29
Nodes (6): cases, faces, file, oracle, schema_version, sha256

### Community 81 - "format-reference/manifest.json / deterministic"
Cohesion: 0.29
Nodes (6): deterministic, fixtures, generated_by, schema_version, xml_semantics, zip_semantics

### Community 83 - "reference.rs / complete_harfbuzz_corpus_matches_shaper_at_fractional_size_and_scale()"
Cohesion: 0.40
Nodes (3): complete_harfbuzz_corpus_matches_shaper_at_fractional_size_and_scale(), REFERENCE, rounded()

### Community 84 - "fonts/manifest.json / asset_count"
Cohesion: 0.33
Nodes (5): asset_count, assets, license, schema_version, total_bytes

### Community 85 - "Point / Path"
Cohesion: 0.40
Nodes (6): AffineTransform, De Casteljau Subdivision, MAX_COORDINATE, Path, Point, Rect

### Community 87 - "check-wasm-size.mjs / artifactPath"
Cohesion: 0.40
Nodes (4): artifactPath, defaultArtifact, referencePath, repoRoot

### Community 88 - "image-reference/manifest.json / cases"
Cohesion: 0.40
Nodes (4): cases, oracle, pngsuite_archive_sha256, pngsuite_archive_url

### Community 89 - "JournalError<E> / .fmt()"
Cohesion: 0.50
Nodes (3): JournalError<E>, Display, Formatter

### Community 90 - "Encoding Enum / decode"
Cohesion: 0.50
Nodes (4): decode, detect_encoding, encode, Encoding Enum

### Community 91 - "Journal / SnapshotCodec"
Cohesion: 1.00
Nodes (3): Journal, SnapshotCodec, US-074: Recovery Journal

### Community 92 - "Selection / PositionMap"
Cohesion: 0.67
Nodes (3): PositionMap, Selection, US-073: Selection Model

### Community 94 - "NameAlias / NAME_ALIASES"
Cohesion: 0.67
Nodes (3): NAME_ALIASES, name_aliases(), NameAlias

### Community 96 - "IndexedTree<T> / bench_indexed_tree"
Cohesion: 0.67
Nodes (3): bench_indexed_tree, IndexedTree<T>, NodeId

## Knowledge Gaps
- **361 isolated node(s):** `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA`, `DIST_BASE`, `DIST_EXTRA` (+356 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 664 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **51 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Error` connect `Error / Result` to `xml/src/lib.rs / XmlError`, `zip/src/lib.rs / .new()`, `render/src/lib.rs / DisplayListError`, `Point / Path`, `src/layout.rs / LayoutError`, `compress/src/lib.rs / .push()`, `src/manager.rs / ManagerError`, `svg.rs / render_svg()`, `ucd-gen/src/lib.rs / err()`, `outline.rs / OutlineError`, `NodeId / selection.rs`, `CodecError / Result`, `history.rs / .apply()`, `protocol.rs / encode()`, `ShapeError / .finish()`, `journal.rs / snapshot_state()`, `T / IndexedTree`, `unicode/src/lib.rs / encode()`, `Result / ResourceState`, `ImageError / decode_gif()`, `.new() / snapshot()`, `Operation / .apply()`, `OffsetError / BoundaryIndex<'a>`, `FontError / parse_cmap()`, `String / run()`, `IndexedTreeError / OperationError`, `JournalError<E> / .fmt()`?**
  _High betweenness centrality (0.380) - this node is a cross-community bridge._
- **Why does `String` connect `String / run()` to `xml/src/lib.rs / XmlError`, `zip/src/lib.rs / .new()`, `render/src/lib.rs / DisplayListError`, `Point / Path`, `src/manager.rs / ManagerError`, `svg.rs / render_svg()`, `ucd-gen/src/lib.rs / err()`, `Error / Result`, `CodecError / Result`, `check-license/src/main.rs / check_tree()`, `bench/src/lib.rs / .new()`, `bench/src/main.rs / run()`, `check-workspace/src/main.rs / .value()`, `unicode/src/lib.rs / encode()`, `font/src/lib.rs / FontFace`?**
  _High betweenness centrality (0.157) - this node is a cross-community bridge._
- **Why does `ImageError` connect `ImageError / decode_gif()` to `src/png.rs / decode_png()`, `image/src/lib.rs / DecodeLimits`, `Error / Result`, `.fmt() / Formatter`, `decode_bmp() / bmp.rs`?**
  _High betweenness centrality (0.062) - this node is a cross-community bridge._
- **What connects `WINDOW`, `LENGTH_BASE`, `LENGTH_EXTRA` to the rest of the system?**
  _361 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `xml/src/lib.rs / XmlError` be split into smaller, more focused modules?**
  _Cohesion score 0.06273106738223018 - nodes in this community are weakly interconnected._
- **Should `zip/src/lib.rs / .new()` be split into smaller, more focused modules?**
  _Cohesion score 0.061275831087151844 - nodes in this community are weakly interconnected._
- **Should `render/src/lib.rs / DisplayListError` be split into smaller, more focused modules?**
  _Cohesion score 0.06988934187536401 - nodes in this community are weakly interconnected._
Références AST externes non résolues exclues du graphe : 119 (détail dans unresolved-structural-references.json). Le graphe simple peut fusionner plusieurs relations entre les mêmes symboles ; les relations brutes restent dans .graphify_extract.json.

Coût CLI cumulé enregistré : 11881 tokens en entrée et 8138 en sortie ; la génération locale de ce rapport par API utilise 0 token LLM.

Tentatives CLI dont les tokens ne sont pas rapportés : 1. Les compteurs ci-dessus couvrent uniquement les usages observés ; aucune estimation inventée n’est ajoutée.

## Périmètre de cette mise à jour

La carte combine les dépendances documentaires déclarées (158 US), l’AST des sources Rust et les métadonnées Cargo réellement présentes. Les crates vides ne représentent pas des fonctionnalités implémentées. La commande `py -m graphify . --cargo` a réussi après mise en cache des documents et exclusion du runtime agent ; les API locales de Graphify régénèrent ce rapport et ses libellés. Cargo.toml est couvert par les métadonnées de ses 32 paquets, même si son extraction AST produit zéro nœud. Cette génération locale par API n’utilise pas de LLM ; les coûts des extractions CLI restent dans cost.json.
