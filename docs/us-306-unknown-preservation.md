<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-306 — Préservation des éléments inconnus à l'import docx

`import_docx` ne perd plus aucun élément ni attribut XML des parties qu'il
interprète (`document.xml`, `styles.xml`, `settings.xml`). Ce qu'il ne lit pas
est copié comme sous-arbre opaque (US-032) pendant l'analyse, rattaché à son
nœud avec sa position, et le résultat est rendu dans `ImportedDocx::preserved`.
Il n'existe pas encore d'écrivain (US-307) : cette story livre la **capture** et
la preuve qu'elle est complète. Le code est dans
`crates/fmt-ooxml/src/docx/preserve.rs` ; `docx.rs` et `tables.rs` n'ont que les
points d'appel.

## Règle unique

`preserve::classify(niveau, élément)` dit, pour un enfant et le type de son
parent (`Level`), s'il est **interprété** (`Known`), **traversé** (`Transparent` :
`w:hyperlink`, `w:ins`, `w:moveTo`, `w:smartTag`, `w:customXml`, `w:sdt` +
`w:sdtContent`, `w:fldSimple`, `w:dir`, `w:bdo`, selon le niveau) ou **capturé**.
Les éléments capturés sont copiés par `Reader::capture_opaque` pendant
`parse_xml` (`XmlNode::opaque`) avec leurs espaces de noms effectifs ; leurs
descendants restent dans l'arbre analysé (pour les avis) mais ne sont jamais
interprétés. La liste `preserve::interpreted()` (exposée par
`interpreted_elements()`) est la déclaration de ce que l'import lit ; les tests
la comparent aux bras `match` de `parse_char_attrs`, `parse_para_props` et
`parse_paragraph` : ajouter un élément géré sans le déclarer, ou l'inverse, fait
échouer `every_interpreted_element_has_an_effect_and_the_code_matches_the_declaration`.

## Ce qui est capturé et où

| Contenu | Emplacement |
|---|---|
| Élément en ligne inconnu d'un `w:p` ou d'un `w:r` (`w:drawing`, `w:pict`, `w:object`, `w:fldChar`, `w:instrText`, `w:commentRange*`, `w:commentReference`, `w:footnoteReference`, `w:endnoteReference`, `w:proofErr`, `w:bookmarkStart/End`, `w:del`, `w:moveFrom`, `m:oMath`, `mc:AlternateContent`, éléments étrangers…) | `Paragraph::anchored` (`kind` = nom qualifié, `data` = XML UTF-8), décalage en octets ramené au début du graphème (`floor_boundary`) |
| Bloc inconnu du corps ou d'une cellule (`w:bookmarkStart`, `w:altChunk`, `w:del`, `w:moveFrom`, `mc:AlternateContent`, `w:permStart`, éléments étrangers…) | `Node::Opaque` à sa position dans la suite des blocs (section ou cellule) |
| `w:sectPr` final du corps ; `w:background` ; autres enfants de la racine | `PreservedChild::Opaque` dans le squelette |
| Enfants inconnus de `w:pPr`, `w:rPr` (paragraphe, run, style, valeurs par défaut) | `PreservedChild::Opaque` dans le squelette de la propriété, **entre** les éléments connus, dans l'ordre de la source (`w:keepNext`, `w:numPr`, `w:lang`, `w14:*`, `w:rPr`/`w:sectPr` d'un `pPr`…) |
| Attributs inconnus ou partiels sur éléments connus (`w:rsidR`, `w14:paraId`, `w:eastAsia`, `w:themeShade`, `w:clear`…) | `PreservedElement::attrs` : **tous** les attributs de l'élément, interprétés ou non, valeurs d'origine |
| Enveloppes (`w:hyperlink`, `w:ins`, `w:sdt`, `w:customXml`, `w:smartTag`, `w:fldSimple`…) | `PreservedElement` imbriqué dans le squelette ; leurs propriétés (`w:sdtPr`, `w:smartTagPr`, `w:fldData`…) en `Opaque` dans l'enveloppe ; le texte visible qu'elles contiennent est importé |
| `w:tbl`, `w:tr`, `w:tc` : attributs, enfants inconnus, enveloppes, lignes supprimées, cellules fusionnées absorbées | `Preserved::tables` (par id de table, ligne, cellule) ; `tblPr`, `tblGrid`, `tblPrEx`, `trPr`, `tcPr` restent dans les emplacements `opaque` du modèle (`slot = true`) |
| Styles : `w:name`, `w:next`, `w:uiPriority`, `w:qFormat`, `w:tblPr`…, `w:tblStylePr`, `w:latentStyles` | `Preserved::styles` (racine, `w:style` avec ses attributs, enfants en ordre) |
| `settings.xml` hors `w:defaultTabStop` (dont `w:compat`, lu avec perte) | `Preserved::settings` |
| Parties non interprétées | le paquet OPC (`ImportedDocx::package`), octet pour octet |

Le squelette (`PreservedElement`) reproduit la source : nom qualifié, attributs,
`node` (nœud du modèle), `range` (plage de texte d'un `w:t`, `w:tab`, `w:br`,
`w:sym`… ou d'un run entier) et `children` dans l'ordre (`Element`, `Opaque`,
`Block(id)` pour un bloc du modèle, `Anchored(id)` pour un objet ancré). Les
squelettes de paragraphes sont dans `Preserved::paragraphs`, ceux de la racine
de chaque partie dans `PreservedPart::root`.

Copies redondantes : un élément lu **avec perte** est aussi capturé en entier
(`w:compat`, `w:latentStyles`, éléments de style de tableau lus par US-303).

## Ordre et position

* Ordre des enfants d'une propriété : celui de la source (une entrée par enfant).
  L'écrivain réémet dans cet ordre. Pour une propriété ajoutée ou modifiée, il
  la place avec `rpr_rank`, `ppr_rank` ou `sect_pr_rank` (séquences CT_RPr,
  CT_PPr et CT_SectPr d'ECMA-376) par rapport à ses voisins.
* Position dans le texte : les objets ancrés suivent le texte (le modèle les
  déplace et les supprime avec lui). L'ordre exact entre marques d'un même
  décalage est celui du squelette, pas celui des identifiants. Un décalage au
  milieu d'un graphème (accent combinant dans un autre run) est ramené à son
  début ; le texte n'est jamais modifié.
* Blocs : `Node::Opaque` occupe un rang dans la liste des blocs ; le `Block(id)`
  du squelette garde l'ordre relatif aux enveloppes.

## Conteneurs (`w:sdt`, `w:customXml`, `w:ins`…)

Choix : **contenu importé et enveloppe mémorisée**. Les paragraphes, tableaux et
runs qu'ils contiennent sont dans le modèle (le texte visible n'est jamais perdu) ;
l'enveloppe (nom, attributs, propriétés opaques) est un `PreservedElement` du
squelette qui contient les `Block`/runs. Un conteneur inconnu qui n'est pas dans
la liste ci-dessus est capturé en bloc : son contenu n'est pas importé (changement
par rapport à l'import antérieur, qui descendait dans tout élément WordprocessingML
inconnu ; cela ne concerne que des documents invalides).

## Suivi des modifications

Le texte visible est inchangé (état final : `w:ins` et `w:moveTo` importés,
`w:del` et `w:moveFrom` non). Ce qui était abandonné est désormais capturé à sa
position : `w:del`/`w:moveFrom` en ligne → objet ancré ; au niveau bloc →
`Node::Opaque` ; `w:rPrChange`/`w:pPrChange`/`w:sectPrChange` et `w:rPr/w:ins|del` →
enfants opaques de la propriété ; ligne supprimée (`w:trPr/w:del`) →
`PreservedChild::Opaque` de la table. Conséquence pour l'écrivain : un document
non modifié se réécrit avec son suivi intact ; si l'utilisateur édite à
l'intérieur d'une insertion suivie, l'enveloppe `w:ins` du squelette ne suit plus
les plages de texte et doit être régénérée ou abandonnée (décision d'US-307).

## Compatibilité de balisage (`mc:`)

* Déclarations : `PreservedPart::declarations` liste tous les `xmlns` de la racine
  avec leurs préfixes (le lecteur XML les cache : la balise racine est relue).
* `mc:Ignorable` (racine, ou n'importe quel élément) : `PreservedPart::ignorable`
  donne les espaces de noms. Ils sont conservés ; leurs éléments (capturés) ne
  produisent aucun avis. Un élément capturé d'un autre espace de noms étranger
  produit un `UnsupportedElement`.
* `mc:AlternateContent` : choix par `preserve::choose_branch` (premier `mc:Choice`
  dont tous les préfixes de `Requires` sont des espaces de noms compris : w, mc, r,
  xml ; sinon `mc:Fallback`). L'élément entier (Choice et Fallback) est capturé et
  réémettable tel quel ; au niveau paragraphe et bloc, le contenu de la branche choisie
  est en plus importé dans le modèle (texte visible) sous un `PreservedElement`
  `shadow = true`. L'écrivain réémet l'objet opaque et **ignore** les éléments `shadow`
  tant que ce contenu n'est pas modifié. Au niveau d'un run (`w:drawing`/`w:pict`),
  la branche n'est pas interprétée (US-305).
* `mc:MustUnderstand` avec un espace de noms inconnu : `UnsupportedProperty`
  (`local_name = "MustUnderstand"`), jamais un échec.

## Avis

`UnsupportedElement` et `UnsupportedProperty` disent toujours « non interprété,
non affiché ». Ils ne signifient plus « perdu » : le contenu est dans `preserved`,
le modèle ou le paquet. Aucune variante n'a été ajoutée à `ImportNotice`.

## Limites (`DocxLimits`, valeurs par défaut)

`max_opaque_bytes` 128 Mio et `max_opaque_items` 8 000 000 (tous les `parse_xml`
de la partie, erreurs `Limit("opaque capture bytes")` / `Limit("opaque capture
count")`), `max_preserved_nodes` 32 000 000 (`Limit("preserved nodes")`). La profondeur
est celle des limites XML (256). Coût linéaire : une capture par élément inconnu
de plus haut niveau, un second passage sur chaque capture pour garder l'arbre
analysé. Le squelette conserve des noms qualifiés par nœud (mémoire de l'ordre de
l'arbre XML).

## Ce que l'écrivain (US-307) devra faire

1. Parcourir `PreservedPart::root` ; pour `Block(id)` émettre le nœud du modèle
   (paragraphe depuis `Preserved::paragraphs`, tableau/ligne/cellule depuis
   `Preserved::tables`, `Node::Opaque` tel quel) ; pour `Anchored(id)` l'objet ancré
   du paragraphe ; pour `Opaque` les octets ; sauter `shadow`.
2. Un paragraphe modifié : régénérer runs et texte depuis le modèle, réémettre
   attributs (`attrs`) et propriétés dans l'ordre mémorisé, objets ancrés à leur
   décalage courant.
3. Déclarer sur la racine toutes les `declarations` et réécrire `mc:Ignorable`.
4. Ne pas réécrire `slot = true` depuis le squelette : prendre le `opaque` du nœud.
5. Rappel : `w:p`/`w:r` perdent leurs enveloppes de suivi si leur texte est édité.

## Emplacements à ajouter au modèle

`Paragraph` : propriétés opaques au niveau du paragraphe (aujourd'hui dans le
squelette, car `ParaProps::opaque` modifierait l'égalité des propriétés et les
plages de runs). `Section::opaque` n'est pas alimenté (le `sectPr` est opaque dans
le squelette, US-304). `Style` : éléments inconnus du style (aujourd'hui dans
`Preserved::styles`). `RichDocument` : enveloppes de conteneurs (`w:sdt`…) comme
nœuds de groupe.

## Preuves

`crates/fmt-ooxml/tests/docx_preserve.rs`, 12 tests : décalages exacts des objets
ancrés ; graphèmes ; ordre des propriétés et attributs ; blocs opaques ; tables ;
styles/réglages ; `mc:` ; parties non interprétées identiques octet pour octet
(commentaires, notes, glossaire, customXml, VBA, webSettings, fontTable, thème,
médias, miniature, propriétés personnalisées, relations) ; déclaration `interpreted`
contre le code ; limites typées ; campagne de mutations/troncatures à graine fixe.
**Inventaire** : un parcours indépendant (`Reader`) liste éléments et attributs de
chaque partie interprétée, puis vérifie l'égalité (multi-ensemble) avec ce que
retrouve le squelette et les captures ; sur 6 documents (`basic.docx`, un docx
LibreOffice, 4 construits) : 506 éléments, 342 attributs, 0 orphelin. Une
**reconstruction** de chaque partie à partir du modèle et des captures est comparée
à la source normalisée (préfiguration de l'écrivain).

Provenance : `basic.docx` est synthétique ; `tests/docx-reference/libreoffice/rich.docx`
vient de LibreOffice 26.8 ; les autres sont construits à la main. **Aucun fichier
Word** n'a été testé (Word n'est pas installé).
