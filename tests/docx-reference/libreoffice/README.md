<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# Corpus LibreOffice (US-306)

`rich.docx` a été produit par LibreOffice 26.8 (Windows, mode sans interface) à partir de `rich.fodt` :
`soffice --headless --convert-to docx:"MS Word 2007 XML" rich.fodt`. Il contient un commentaire, une note de bas
de page, un lien, un signet, un suivi des modifications (insertion et suppression), une image et un tableau.
Ce n'est pas un fichier Word : Microsoft Word n'est pas installé sur la machine de livraison. Les entrées sont
compressées (pas la convention « stocké » de `basic.docx`) ; le fichier fait moins de 8 Ko. Il sert à
l'inventaire de complétude de `crates/fmt-ooxml/tests/docx_preserve.rs`.

`properties.docx` (US-328) vient de la meme maniere de `properties.fodt` (titre, sujet, auteur, mots-cles, commentaires avec accents et caracteres speciaux dans `office:meta`) ; script : `tools/check-docx-properties-libreoffice.py --regenerate`. Pas un fichier Word.
