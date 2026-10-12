<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->
# US-215 — Bordures et trames des paragraphes paginés

Cette implémentation fournit une politique de mise en page typée, une géométrie de bordures locale à chaque page et un adaptateur vers les commandes de liste d’affichage existantes. Elle n’ajoute aucun champ à `ParaProps` et ne modifie pas le format binaire de la liste d’affichage. L’US-215 reste partielle.

## API et unités

`atelier_text_layout::borders` expose `ParagraphBorderPolicy`, `BorderLine`, `BorderStyle`, `ColumnBounds`, `ParagraphBorderInput`, `plan_paragraph_borders` ainsi que les types de plan et de primitives. Les politiques sont fournies séparément pour chaque paragraphe par l’appelant; elles ne sont ni persistées dans le modèle ni intégrées à l’interface. Les couleurs RGB réutilisent le type de couleur du modèle de texte riche. L’épaisseur et la distance au texte sont des entiers en unités de 1/64 de point typographique, comme dans la mise en page et la pagination. L’épaisseur doit être positive et l’espacement non négatif. Une bordure double de moins de trois unités est refusée, car elle ne peut contenir deux traits d’épaisseur positive à cette précision.

Chaque entrée indique explicitement la coordonnée x et la largeur disponible de la colonne du paragraphe, en coordonnées locales à la page. Le planificateur ne déduit pas la colonne de la largeur des lignes : une ligne courte dans une colonne large conserve un cadre et une trame de pleine largeur.

Le planificateur valide les plages de pages, les indices de page et de placement, la conservation des lignes, leur ordre source, les placements manquants ou dupliqués, les hauteurs positives et les opérations arithmétiques sur les coordonnées. `BorderPlanLimits` borne le travail, le nombre de fragments et celui des primitives générées. Une erreur ne renvoie aucun plan partiel.

## Regroupement et pages

Des paragraphes source consécutifs sur une même page partagent un cadre si toutes leurs bordures, y compris `between`, et leurs bornes horizontales sont identiques. La trame ne participe pas à cette égalité : chaque paragraphe garde sa propre primitive de remplissage. Un paragraphe sans bordure ou sans ligne interrompt le groupe. Le haut et le bas du cadre sont tracés une seule fois; les côtés relient les axes des traits supérieur et inférieur. Lorsqu’un côté existe, les extrémités du haut et du bas rejoignent l’axe de ce côté, en tenant compte de son espacement et de la moitié de son épaisseur. Sans côté, elles restent aux bornes de la colonne. Une bordure `between` apparaît uniquement entre deux fragments de paragraphes distincts, jamais entre les lignes d’un même paragraphe.

Un groupe réparti sur plusieurs pages devient un cadre fermé sur chaque page. Chaque fragment de page reçoit son propre haut et bas, ainsi que ses côtés continus. C’est la règle de pagination documentée par Atelier; elle n’a pas été comparée à Word. La trame couvre la largeur explicite de colonne et l’étendue des boîtes de lignes du paragraphe, sans couvrir l’espace vertical entre les paragraphes.

La bordure `between` est placée après la dernière boîte de ligne du paragraphe précédent, à la distance configurée plus la moitié de son épaisseur. L’espace doit aussi contenir l’autre moitié de l’épaisseur avant la boîte suivante; si la distance ou l’épaisseur dépasse l’espace disponible, le plan renvoie une erreur typée au lieu de tracer sur le contenu du paragraphe suivant.

## Styles et rendu

`Solid` produit un trait. `Double` produit deux traits parallèles dont l’enveloppe est au plus égale à l’épaisseur demandée; leur largeur vaut un tiers entier de celle-ci, avec un décalage arrondi vers l’intérieur en unités de 1/64 de point. `Dashed` utilise un tiret de cinq épaisseurs suivi d’un vide de deux épaisseurs. `Dotted` produit des marques rondes séparées par un vide visible de deux épaisseurs, soit une distance de trois épaisseurs entre leurs centres. Les motifs pointillés et tiretés sont développés en primitives sous les limites de travail et de sortie, car `StrokeStyle` ne sait pas représenter les tirets. Pour les épaisseurs divisibles par trois, l’enveloppe du double trait égale l’épaisseur demandée; pour les autres, elle est réduite d’au plus une unité.

`atelier_render::paragraph_borders::paragraph_border_commands` convertit le plan d’une page en commandes `FillPath` et `StrokePath` validées. Les trames sont placées avant les bordures. La conversion des coordonnées de 1/64 de point vers les points a lieu uniquement à cette frontière; les points sont rendus par des chemins cubiques ronds. L’appelant n’ajoute le vecteur retourné qu’après réussite, puis utilise l’encodeur `DisplayList` et les moteurs SVG ou raster habituels.

## État et limites

Les tests unitaires et d’intégration couvrent la largeur de colonne distincte de celle du texte, les cadres partagés, les trames indépendantes, les ruptures de groupe, les changements de page, les styles de ligne, les géométries malformées, les limites de ressources, l’encodage/décodage de la liste d’affichage et la sortie SVG. Ils vérifient les valeurs géométriques et le contrat de rendu, sans établir de fidélité visuelle à Word. Il n’existe pas de corpus de comparaison authentique avec Word. L’intégration à l’éditeur riche, l’import OOXML, la persistance, l’interface utilisateur et la fidélité à Word ne sont pas encore réalisés ou validés. L’US-215 demeure donc partielle.
