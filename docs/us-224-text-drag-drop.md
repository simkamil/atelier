# US-224 — glisser-déposer dans l'éditeur TXT

Le démonstrateur permet de déplacer une sélection, de la copier en maintenant Ctrl au relâchement et d'insérer un dépôt `text/plain`. Un trait vert indique la position d'insertion. Le geste garde la sélection source jusqu'au dépôt; la sonde de position ne modifie ni sélection, ni révision, ni historique. Un clic sans déplacement replace le curseur. Maj+clic et les doubles/triples clics gardent leurs comportements de sélection.

Le déplacement, la copie et l'insertion externe sont chacun une transaction isolée de la frappe. Annuler restaure le texte et la sélection dirigée d'origine; Rétablir réapplique l'ensemble. Un déplacement sur la source ou ses bords ne crée aucune opération. Ctrl autorise une copie à l'intérieur de la source. Les offsets restent des octets UTF-8, avec frontières de graphème vérifiées par le moteur. BOM et CRLF sont conservés.

Le contrôleur capture les données du dépôt pendant l'événement, avant toute attente asynchrone. HTML et fichiers ne sont pas importés. Les sondes sont coalescées, leur résultat est gardé par révision et sélection, et le relâchement utilise sa position exacte. Les gestes sont annulés par Échap, perte de capture, fermeture, dialogue de fichiers ou composition. Le défilement aux bords reste borné.

Le protocole `ATED` v1 ajoute les commandes suivantes, tous les nombres étant little-endian:

| Opcode | Corps | Résultat |
|---|---|---|
| 15 ProbePoint | page u32, x f64, y f64 | `ATPT`, version u16=1, flags u16=0, révision u64, offset u32, page u32, x/y/hauteur f64; 48 octets |
| 16 DropText | révision u64, temps u64, source anchor/focus u32, cible u32, type u8, réservés 3 octets zéro, texte UTF-8 | `ATES` habituel |

Les types sont 0 déplacement, 1 copie, 2 externe. Les dépôts internes transmettent une sélection dirigée qui doit toujours être celle du moteur et aucun texte; le moteur lit la source. Un dépôt externe exige une source nulle. Révision périmée, offset invalide, drapeaux ou réservés inconnus et limites dépassées refusent le candidat entier. La limite de document reste 128 Kio et l'historique 4 Mio.

Les tests du contrôleur, du modèle, de l'ABI WASM et du navigateur couvrent les transactions et les événements synthétiques. Le navigateur utilise le vrai Worker/WASM; seule la capture matérielle du pointeur est simulée. **L'essai natif depuis une autre application Windows reste à faire**: sélectionner un texte dans une autre fenêtre, le déposer sur une page, vérifier l'export TXT, puis Annuler/Rétablir. La dernière case de l'US reste ouverte jusqu'à cet essai. Couper/copier/coller au clavier restent disponibles.
