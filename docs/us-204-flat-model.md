# US-204 — Modèle riche éditable à positions plates

Tranche R1 du [plan de bascule](rich-editor-migration.md), contre le contrat figé d'[US-203](us-203-rich-editor-protocol.md) (`crates/text-model/src/rich/flat.rs` : `FlatIndex`, `FlatEdit`, `LayoutContext`, `EditOutcome`, `PageSetup`, `FlatError`). Aucune signature du contrat n'a changé.

## Type et fichiers

Les trois traits sont implémentés sur `RichDocument` (`atelier_text_model::rich`). Le document porte désormais, hors de l'égalité (qui compare toujours le contenu) :

- l'index plat, construit à la première requête et tenu à jour par les éditions plates ;
- l'historique d'annulation (deux piles de `RichOp` et un compteur de révision) ;
- le catalogue de styles et le thème servis par `LayoutContext` (`set_styles`, appelé par l'import DOCX).

Un clone a le même contenu et les mêmes styles mais un historique vide ; toute opération passée par l'ancienne voie (`RichDocument::apply`, une transaction) abandonne l'index et l'historique plat.

| Fichier | Rôle |
|---|---|
| `rich/flat_tree.rs` | Arbre implicite (treap) des paragraphes, sommes préfixes en octets |
| `rich/flat_impl.rs` | `FlatIndex` et `LayoutContext`, bornes de graphèmes et de mots, compteurs de liste, conversions nœud + décalage |
| `rich/edit.rs` | `FlatEdit` : planification d'une édition, application en place, historique |
| `rich/paragraph.rs`, `rich/node.rs`, `rich/attrs.rs` | éléments en ligne `Break` et `Symbol`, `char_style`, `BlockList::position` |
| `examples/flat_edit_benchmark.rs` | banc du critère « moins de 1 ms » |
| `fmt-ooxml/src/docx/inline.rs`, `docx/write/legacy.rs` | import et écriture DOCX de ces champs |

## Index

Chaque nœud du treap est un paragraphe de l'index plat (corps, cellules de tableau ligne par ligne, bloc opaque = un paragraphe d'un U+FFFC) et porte le nombre de paragraphes et d'octets de son sous-arbre (longueur plate plus un séparateur). Les opérations suivantes sont en `O(log n)` : `locate`, `position`, `paragraph_start`, lecture d'une entrée, remplacement d'un paragraphe par un autre ou par plusieurs. Les priorités viennent d'un xorshift à graine fixe : la forme de l'arbre est déterministe. Chaque entrée garde l'identifiant du nœud, sa nature (corps, cellule avec ses identifiants de tableau, ligne et cellule, opaque), son conteneur (section ou cellule), sa section et sa longueur plate.

Une requête coûte `O(log n)` plus la longueur des paragraphes lus : les bornes sont calculées sur le texte plat d'un seul paragraphe (coupé à chaque U+FFFC, un `\n` de séparation étant à lui seul un graphème et un mot). `text` est linéaire dans les octets rendus. Un paragraphe de 1 Mo coûte donc 1 Mo par requête de borne : le contrat l'accepte, la saisie sur un document normal ne le voit pas.

Les compteurs de liste (`list_counters`) sont calculés en avançant depuis le point de contrôle le plus proche (un par 256 paragraphes) ; un changement à l'indice `i` supprime les points de contrôle au-delà de `i`. L'appartenance à une liste est la référence directe du paragraphe, sinon celle de son style (ou d'un style dont il hérite), sinon le défaut du document. Pour un paragraphe, `before[l]` est le nombre de paragraphes précédents de la même liste au niveau `l` depuis le dernier paragraphe d'un niveau inférieur ; le paragraphe lui-même n'est pas compté.

`LayoutContext::resolved` applique le style du tableau le plus interne qui contient le paragraphe (à défaut le style de tableau par défaut du catalogue). L'héritage d'un tableau imbriqué par son parent n'est pas repris (`StyleEngine::resolve` le fait en parcourant tout le document).

## Conversions

- plate ↔ (paragraphe, décalage) : `locate` et `position`, un séparateur appartient au paragraphe qu'il termine ;
- plate ↔ (paragraphe, décalage dans `Paragraph::text`) : `flat_to_text`, `text_to_flat` du contrat ;
- plate ↔ (identifiant de nœud, décalage) : `RichDocument::node_position` et `RichDocument::flat_position` (le journal de récupération garde le couple, jamais la position plate). Le décalage est celui de `Paragraph::text` ; les positions situées avant ou après les éléments en ligne d'un même décalage ont le même couple, `after_items` choisit le côté au retour.

Les tests comparent l'index à la double `VecParagraphs` (index linéaire reconstruit du texte entier) sur des documents aléatoires à objets, sauts, symboles, tableaux et blocs opaques : toutes les positions, tous les mouvements de graphème et de mot, `word_at`, `snap_back`, `check_position`, `text`.

## Édition

`replace_range(start, end, text)` valide d'abord (ordre : `InvalidRange`, `OutOfRange`, `InvalidText`, `NotBoundary`), puis transforme l'édition en `RichOp` appliquées directement au document, sans transaction (une transaction prend un point de contrôle en clonant le document). L'inverse de chaque opération est calculé juste avant son application et conservé : l'annulation applique les inverses à rebours, le rétablissement les inverses de ceux-ci. Un échec du modèle en cours de route défait ce qui était fait avant de rendre l'erreur (`Limit` pour des identifiants ou une capacité épuisés, `Style`, soit `RICH_INTERNAL`, pour un échec inattendu).

Règles, `P1` étant le paragraphe de `start` et `P2` celui de `end` :

- même paragraphe : suppression de l'intervalle (texte et éléments en ligne dont le U+FFFC est dedans), puis insertion ; le texte inséré hérite des attributs directs du caractère qui précède ;
- `P1` et `P2` dans la même suite de blocs (même corps de section ou même cellule) : les blocs entre eux sont retirés (tableaux et blocs opaques compris), la fin de `P1` et le début de `P2` sont supprimés et les deux paragraphes fusionnent, `P1` gardant identifiant et propriétés (`MergeParagraphs`). Supprimer un `\n` est ce cas ;
- sinon (intervalle à cheval sur des cellules, une frontière de tableau ou un bloc opaque) la structure est conservée : le texte de chaque paragraphe de l'intervalle est effacé, rien n'est fusionné ni retiré ;
- `\n` dans le texte scinde le paragraphe. Le modèle n'a pas de « style suivant » (style du paragraphe suivant de Word) : les nouveaux paragraphes prennent les propriétés du paragraphe scindé, donc son style. Le critère « Entrée scinde avec le style suivant » est tenu à ce sens près ;
- du texte tapé sur un bloc opaque (avant ou après son U+FFFC) devient de nouveaux paragraphes avant ou après le bloc ; supprimer le bloc lui-même ne change rien.

`EditOutcome` décrit toujours l'intervalle remplacé : `start`, `removed_bytes = end - start`, `inserted_bytes` (déduit de la longueur du document), les paragraphes avant (`first_paragraph`, `removed_paragraphs`) et après (`inserted_paragraphs`). Ce qui précède `start` et ce qui suit l'intervalle sont inchangés (vérifié par les tests). Annulation et rétablissement rendent le même intervalle dans le sens de l'appel ; la sélection est `(start, end)` après une annulation et le point d'insertion après une édition ou un rétablissement. Une édition qui laisse le document identique (par exemple remplacer une espace par une espace) n'est pas appliquée : sortie à zéro octet, révision inchangée. `layout_scope` vaut toujours `Paragraphs` (aucune opération plate ne touche styles ou sections).

L'historique garde au plus 10 000 entrées par pile ; l'inverse d'une suppression est la copie du paragraphe touché.

## Éléments en ligne

`Inline::Break` et `Inline::Symbol` (3 octets chacun dans le texte plat) et `CharAttrs::char_style` (0 octet) sont des champs du modèle. `Paragraph` les décale avec les éditions de texte (`insert_text`, `delete_text`, `split_off`, `append`), `validate_local` les contrôle (tri par décalage et identifiant, décalage sur une frontière de scalaire, police et point de code des symboles), `char_style` est pris en compte par `overlay`, `clear` et `validate`. Les décalages des objets, sauts et symboles n'ont besoin que d'une frontière de scalaire : une marque combinante tapée juste après un objet forme son propre graphème dans le texte plat, alors qu'elle se colle à la lettre précédente dans `Paragraph::text`. Les bornes des runs, champs, liens et signets restent sur des graphèmes de `text` ; une édition qui en laisse une dans un graphème fusionné la ramène au bord du graphème (début en arrière, fin en avant).

### Import DOCX

Le lecteur de `w:p` construit toujours le texte de la source (un `\n` par `w:br` ou `w:cr`, le caractère de chaque `w:sym`), car les plages du squelette conservé (`PreservedElement::range`) le comptent. `inline::strip` le transforme ensuite en texte du modèle : ces caractères sortent du texte et deviennent des éléments, les autres décalages suivent. Les identifiants sont attribués dans l'ordre du document, ce qui donne l'ordre des éléments à un décalage. `w:br` sans type et `w:cr` sont des sauts de ligne, `w:br` de type `page` ou `column` ce que dit le type. `w:rStyle` devient `char_style` du run. Le `w:sectPr` du premier paragraphe qui en porte un, à défaut celui qui clôt le corps, alimente `Section::opaque` : `pgSz.w`, `pgSz.h`, `pgSz.orient`, `pgMar.top`, `.right`, `.bottom`, `.left`, `.header`, `.footer`, `.gutter`, espace de noms WordprocessingML, valeur = texte de l'attribut ; `PageSetup::from_section` les lit et normalise. Le modèle n'a qu'une section : la géométrie des suivantes n'est pas portée.

`ImportedDocx::breaks`, `symbols` et `run_styles` subsistent comme vue dérivée (décalages du texte du modèle) tant que `RichSession` (`core-wasm`) les lit ; plus rien dans le modèle ne dépend d'elles et aucune n'est remappée par `position_map`.

### Écriture DOCX

L'écrivain retrouve le texte de la source depuis le modèle (`source_text`, public : `docx/write/legacy.rs`) pour comparer paragraphe courant, paragraphe de l'import et squelette comme avant. Les sauts et symboles ajoutés au modèle (ou déplacés) sortent en `w:br` (avec le type) et `w:sym` à leur place, dans le chemin du squelette comme dans la reconstruction depuis le modèle ; `char_style` est écrit en `w:rStyle` (nouvelle famille de propriétés de run, conservée telle quelle si elle n'a pas changé). Le test `docx_roundtrip_corpus` (67 documents, zéro perte) et les campagnes d'édition d'US-307 passent.

## Banc

`cargo run --release -p atelier-text-model --example flat_edit_benchmark` : 300 pages de 40 paragraphes (12 001 paragraphes, 985 306 octets de texte plat, un paragraphe sur dix avec un objet, un saut ou un symbole), puis dix fois plus. Positions tirées d'une graine fixe dans la moitié centrale du document ; 2 000 appels par ligne ; Windows, build release, machine partagée avec d'autres compilations.

| Opération | médiane | 99e centile | maximum |
|---|---:|---:|---:|
| saisir un caractère | 11,5 µs | 44,1 µs | 127,3 µs |
| Entrée (scinder) | 18,8 µs | 56,7 µs | 245,2 µs |
| supprimer un séparateur (fusion) | 19,8 µs | 61,2 µs | 164,8 µs |
| supprimer un graphème | 15,4 µs | 58,0 µs | 125,5 µs |
| remplacer 20 octets par 3 lignes | 28,3 µs | 83,7 µs | 260,3 µs |
| annuler | 13,7 µs | 52,0 µs | 75,2 µs |
| rétablir | 12,9 µs | 32,6 µs | 381,0 µs |
| `locate` + `check_position` | 1,9 µs | 4,7 µs | 11,3 µs |

Index construit en 5,0 ms (une fois). Avec 3 000 pages (120 001 paragraphes, 9,9 Mo) : médianes de 13,6 à 31,6 µs et 99e centile au plus 84,1 µs : le coût ne suit pas la taille du document (le seul maximum au-dessus de la milliseconde, 1,9 ms à chaque exécution, est l'agrandissement du tableau de nœuds de l'arbre une fois ses 120 001 emplacements pleins ; à 300 pages le maximum est de 0,4 ms). Le critère (moins de 1 ms côté modèle) est tenu sur les médianes et les 99e centiles avec un facteur supérieur à dix.

## Tests

- `tests/flat_index.rs` : l'index contre `VecParagraphs` (hôte seulement, la double n'existe pas en wasm32), refus des positions dans un objet ou un graphème, `LayoutContext` (sections, styles, géométrie, ressources, compteurs de liste) ;
- `tests/flat_edit.rs` : scénarios (Entrée, fusion, objets, marque combinante après un objet, cellules, bloc opaque, refus), 400 séquences contre un modèle par chaîne de caractères, 300 séquences structurées (invariants, index reconstruit identique, intervalle de l'`EditOutcome`), annulation puis rétablissement à chaque étape, et **10 000 séquences à graine fixe** qui reviennent à l'état initial après inversion ;
- `fmt-ooxml/tests/docx_inline_items.rs` : import des sauts, symboles, styles de run et du `sectPr`, aller-retour avec et sans `regenerate_all`, éléments ajoutés au modèle, éditions plates autour des éléments, `char_style` posé par le modèle.

## Limites connues

- Les consommateurs qui lisent `Paragraph::text` seul ne voient pas les sauts et symboles : la recherche (`ParagraphSource`) et les statistiques comptent « a⏎b » comme un seul mot ; `rich_to_text` les rend (saut = fin de ligne, symbole = son caractère). L'aperçu DOCX et `RichSession` de `core-wasm` lisaient le saut comme `\n` dans le texte ; la mise en page d'US-216 doit lire les éléments.
- Les champs, liens et signets ne sont pas produits par l'import ; pour un paragraphe qui en porte, l'écrivain reconstruit depuis le modèle comme avant.
- Une opération hors édition plate (`RichDocument::apply`, styles, tableaux) vide l'historique plat : annuler ces opérations avec les éditions plates relève de la couche qui les empile (US-217).
