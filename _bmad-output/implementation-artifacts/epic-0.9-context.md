# Epic 0.9 Context: Liste d’affichage

<!-- Compiled from planning artifacts. Edit freely. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Fournir une représentation graphique ordonnée, validée et indépendante des backends, puis permettre de la rejouer pour l’impression et les sorties visuelles. Les pages partagent les mêmes chemins, glyphes, images et états graphiques afin que les backends puissent rester cohérents sans charger eux-mêmes les ressources du document.

## Stories

- US-080 : Format de liste d’affichage
- US-081 : Backend SVG
- US-082 : Backend raster

## Requirements & Constraints

- Une liste par page conserve l’ordre de peinture, les bornes de page, les chemins, glyphes positionnés, images, clips, transformations, liens et marqueurs de structure.
- Les entrées et ressources sont non fiables. Valider les listes et valeurs, borner comptes, profondeur d’état et octets avant allocation ou croissance, et retourner des erreurs typées sans panique ni résultat partiel.
- Le format de page est versionné et strict : les versions/opcodes inconnus, flux tronqués et octets surnuméraires sont refusés. Les budgets déjà établis par la liste sont de 64 Mio par flux, un million de commandes, éléments de chemin et glyphes cumulés, et 128 niveaux par pile.
- Les formes doivent conserver remplissage non-zéro ou pair-impair, contours et paramètres de trait, couleurs RGBA avec alpha, ainsi que les courbes quadratiques et cubiques. Les backends doivent préserver les liens et rôles de structure sans traiter leurs valeurs comme du balisage.
- Le backend vectoriel est destiné à la vue d’impression. Le rendu raster produit des miniatures et images de référence avec une résolution paramétrable.
- Le workspace reste sans dépendance externe de production. Pas de code `unsafe` dans les crates ordinaires, ni de chargement implicite de ressources, réseau ou fichiers système.
- Les mêmes unités de page sont utilisées par tous les backends; l’alpha des couleurs non prémultipliées est conservé dans la liste, puis interprété par le backend selon son format. Aucun rendu n’altère les données source de la page.

## Technical Decisions

- `atelier-render` possède le modèle de liste et les backends; `atelier-geom` fournit les primitives validées. Les références de police et d’image restent opaques dans la liste; les ressources sont fournies par l’appelant du backend.
- Les chemins réutilisent `Point`, `Rect` et les règles de construction de `atelier-geom`. La géométrie refuse les coordonnées non finies ou au-delà de la plage documentée et remonte les débordements numériques comme erreurs. Les sorties de backend restent dans le crate render plutôt que d’étendre geom avec des concepts de format.
- Les coordonnées sont des unités de page et des `f64` finis. Les matrices utilisent des vecteurs-colonnes et `x' = a*x + c*y + e`, `y' = b*x + d*y + f`. La composition applique d’abord la matrice courante, puis la nouvelle (`current.then(next)`).
- Les piles de clips et transformations sont indépendantes et équilibrées séparément. Les clips rectangulaires sont déjà exprimés en coordonnées page et s’intersectent avec leur parent; ils ne suivent donc pas une transformation ultérieure. Les liens sont également en coordonnées page. Les marqueurs de structure ne changent pas l’état graphique.
- Les formats de sortie sont déterministes et bornés. Les rôles/liens restent des métadonnées correctement échappées. Toute donnée de ressource est validée avant son incorporation; aucune exécution de script ni référence réseau n’est nécessaire.
- Les erreurs distinguent les valeurs/chemins invalides, l’état non équilibré et le dépassement de limite. Les constructeurs et décodeurs valident avant d’exposer une liste; les backends gardent des erreurs explicites pour les ressources qu’ils ne peuvent pas résoudre.

## UX & Interaction Patterns

La liste est rejouée dans l’ordre de peinture. L’impression suit un chemin vectoriel par page, réutilisable par le shell desktop et le navigateur; aucun contrôle d’interface ne s’ajoute à cette tranche. Les liens et la structure sémantique voyagent avec le dessin pour les consommateurs aval, sans devenir des instructions exécutables du backend.

## Cross-Story Dependencies

US-080 dépend de la géométrie US-060. US-081 et US-082 dépendent du contrat validé de US-080; US-082 réutilise le rasteriseur US-061. Les outils de comparaison visuelle US-003 dépendent du backend raster US-082.
