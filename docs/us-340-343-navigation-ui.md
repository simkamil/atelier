# US-340 / US-343 — Recherche et statistiques TXT

Dans un TXT ouvert, **Ctrl+F** ou **Accueil → Rechercher** ouvre le volet. La sélection initialise la requête. Casse, mots entiers, accents et espaces insécables utilisent les règles Unicode du moteur Rust. Entrée/Maj+Entrée parcourent les résultats avec bouclage ; Échap ferme le volet et rend le focus à l’éditeur. Cliquer un extrait sélectionne l’occurrence. Les résultats affichés sont surlignés ; le document et son historique restent intacts.

Le volet affiche au plus **1 000 occurrences**, avec un avertissement lorsqu’un plafond est atteint. Les rectangles et les extraits sont également bornés. Affiner la requête permet de retrouver des occurrences au-delà de cette liste. La recherche ne traverse pas les paragraphes. La requête est limitée à **1 024 octets UTF-8** : une sélection plus longue est réduite à ce plafond à l’ouverture ; une saisie ou un collage au-delà affiche une erreur explicite. Suivant démarre au curseur courant ; cliquer le premier résultat repart du début de la liste.

Le compteur inférieur affiche les mots du document, ou sélection/document. Un clic, ou **Affichage → Statistiques**, ouvre les six mesures : pages, mots, caractères sans/avec espaces, paragraphes et lignes. Le Rust compte le texte ; la mise en page fournit les pages et lignes, y compris pour la sélection. Sans sélection, la colonne sélection affiche « — » ; l’API renvoie des compteurs nuls. La case notes/zones de texte reste désactivée avec son explication.

Les réponses portent une révision ; les requêtes remplacées, modifications, fermetures et transitions d’aperçu invalident les réponses tardives. Les outils sont désactivés dans l’aperçu DOCX en lecture seule. Le raccordement au document riche complet, les notes/zones de texte et la comparaison à Word restent ouverts : **US-340 et US-343 ne sont pas clôturées**.

## Validation

`py _bmad-output/implementation-artifacts/validate-navigation.py` exécute les contrôles Rust, codec, navigateur réel avec Worker/WASM, ressources natives et budget WASM. Les tests couvrent les options Unicode, plages UTF-8, surlignage Canvas, navigation rapide, réponses tardives, sélection/statistiques, dialogues, aperçus et thèmes/contraste élevé. Les événements clavier sont synthétiques ; aucune certification clavier physique ou lecteur d’écran n’est revendiquée.

Les résultats et empreintes exactes sont consignés dans `navigation-validation.json` et `navigation-validation.log`, sous `_bmad-output/implementation-artifacts`. La revue indépendante est suivie dans `spec-search-statistics-ui.md`.
