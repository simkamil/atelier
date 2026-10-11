# US-113 — Pont natif de fichiers pour l’éditeur texte

Le shell Windows accepte `us120-file|1|ID|open|`, `save|BASE64` et
`dirty|0/1` seulement depuis la source exacte `https://app.atelier/`. Les ID
sont des décimaux positifs stricts, au plus 20 chiffres, et croissants pendant
la durée de vie de la page. Une navigation interne approuvée vide l’ancienne
queue et réinitialise cette séquence. Le protocole n’expose aucun chemin ni ticket.

Le callback WebView2 valide et copie les messages dans une queue limitée à
quatre opérations fichier (maximum 131075 octets décodés par opération). Le
dernier état `dirty` est coalescé dans un emplacement séparé borné. Le callback poste ensuite
la notification existante à la fenêtre. Le dépassement de capacité produit une
réponse `files.error.queue_full` dans une petite file de refus bornée. Le
gestionnaire de fenêtre dépile la
queue sur le STA, affiche un dialogue TXT ou exécute le broker, révoque chaque
ticket après lecture/écriture et poste une réponse chaîne corrélée. Les réponses
contiennent les octets encodés en Base64 pour l’ouverture ou une clé de message
stable pour les erreurs. L’annulation conserve l’état du document.

Le contenu texte est limité à 128 Kio UTF-8, hors BOM optionnel de trois octets.
Le premier document interne approuvé démarre l’époque de page 1. Les navigations
top-level suivantes sont annulées, y compris un rechargement propre : ce shell
ne prend en charge qu’une page par instance WebView, ce qui empêche un document
remplacé de réutiliser ses IDs ou de perdre un document modifié. L’API
`begin_file_work` doit rester acquise pendant toute la dépile et les dialogues
STA ; un message réentrant est différé puis réveillé au relâchement du garde.

Le transport conserve le plafond CSP de 768 caractères indépendamment du budget
du pont. La sauvegarde passe toujours par Enregistrer sous et reste atomique via
`FileBroker`. La vérification d’identité du fichier existant précède l’appel de
remplacement Windows ; la fenêtre de course externe entre cette vérification et
`ReplaceFileW` reste une limite du système de fichiers.

La fenêtre avertit lorsqu’un message `dirty|1` a été reçu et ne ferme qu’après
le choix explicite de quitter sans enregistrer. Les événements `dirty` sont
traités côté STA comme les autres messages. La propriété des révisions et la
décision de remettre l’état propre après une sauvegarde restent au moteur et à
l’interface, qui doivent attendre la réponse native et l’accusé de sauvegarde du
moteur.

Les tests natifs couvrent le parseur, la source, le rejeu, la capacité, le
Base64 canonique, le chemin broker avec sélection injectée en test, l’annulation,
les erreurs et la révocation des tickets. Ils ne lancent pas de dialogue.
SharedBuffer, les mesures de performance, l’intégration UI/Worker complète et
la vérification manuelle des dialogues natifs restent ouverts. Les messages de
validation native sont bornés et ne contiennent jamais de chemin ou de texte
système brut.

## Validation du premier éditeur intégré — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle le moteur, l’interface
et le pont fichiers, puis corrigé le lot après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/editor-integration-validation.json)
réussit **40 contrôles, 368 tests workspace et 55 tests natifs release**,
avec sources et lock inchangés. Le navigateur utilise le vrai Worker/WASM/Canvas
et un Host factice : saisie/sélection, pagination, composition, annulations,
sauvegarde obsolète, raccourcis de la vraie page et miroir textuel accessible.
L’ABI réelle vérifie aussi le rollback d’un candidat lorsque le registre de
128 Mio ne peut réserver sa réponse.

Les diagnostics WebView2 debug/release chargent les **17 ressources** et vérifient
saisie accentuée, pixels de glyphes, undo/redo, HTTP et six catégories CSP.
La [copie isolée](../_bmad-output/implementation-artifacts/editor-integration-portable.json)
fonctionne sans dossier web/data. Les tests de routage natif injectent des résultats
de dialogue contrôlés et passent par le broker réel ; ils n’ouvrent aucun dialogue.
La [revue](../_bmad-output/implementation-artifacts/editor-integration-review.md)
conserve les 12 constats séparément : cinq corrections retenues et trois fonctions
V1 reportées (presse-papiers, navigation par mot, colonne préférée), avec les rejets
motivés. Les limites de wrapping et de Tab sont également décrites dans le contrat UI.

Le premier parcours TXT est testable avec `cargo run -p atelier-desktop-win --release --locked --offline`. La tranche reste limitée à
128 Kio, latin gauche à droite et mise en page complète à chaque modification.
Les dialogues manuels, AZERTY/IME physique et mesures de 300 pages/démarrage restent
ouverts pour US-120. US-113 conserve SharedBuffer, polices/URL ; US-111 conserve
l’échec strict de handles. Les **35/158 US complètes** sont inchangées. Les tests
musl sont compilés localement ; leur exécution appartient à la CI Linux publiée.
