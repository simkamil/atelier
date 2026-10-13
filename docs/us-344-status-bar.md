# US-344 — Barre d’état configurable

La barre affiche la page visible **X sur Y**, les mots document/sélection, la langue du document, la vue, le zoom et l’état d’enregistrement. Cliquer sur la page ouvre **Atteindre une page** ; un entier entre 1 et Y déplace uniquement le viewport. Défilement, zoom et navigation visuelle conservent sélection, révision, état enregistré et historique. Le numéro suit la page la plus proche du centre du viewport, avec les mêmes hauteurs et espaces fixes que Canvas.

Un clic droit ou Maj+F10 sur la barre ouvre six options cochables. Chaque champ peut être masqué puis rétabli pendant la session, y compris les compteurs et le zoom fournis par les modules existants. Les choix ont un indicateur visuel et `aria-checked` ; Échap ferme le menu et rend le focus. La configuration n’est pas persistée après fermeture de l’application.

Le TXT n’expose aucune métadonnée de langue : la barre indique **Langue : non définie**, indépendamment de la langue française de l’interface. L’aperçu DOCX affiche son mode en lecture seule, ses propres pages et son zoom ; ses mots et sa langue riche restent indisponibles. Aucun état d’enregistrement TXT n’est attribué à cet aperçu. **US-344 reste partielle pour ces métadonnées et mesures riches**, tandis que la configuration des champs est livrée.

Les nouvelles vues sont isolées de l’accès métier. `document-status.mjs` reçoit des snapshots et transmet les demandes ; `page-navigation.mjs` partage le calcul de viewport entre TXT et aperçu. Les labels proviennent du catalogue français. Un catalogue incomplet n’expose pas de commandes sans libellé ; le TXT reste éditable.

Voir la [spec et revue](../_bmad-output/implementation-artifacts/spec-us341-us344.md), le [reçu de validation](../_bmad-output/implementation-artifacts/replacement-status-validation.json) et le [graphe conservant les concepts](../_bmad-output/implementation-artifacts/replacement-status-graph.json). Les suites couvrent formulaire, bornes, menus, états, défilement, zoom, non-mutation, 420 px, thèmes et couleurs forcées. La certification clavier/lecteur d’écran physique reste distincte.
