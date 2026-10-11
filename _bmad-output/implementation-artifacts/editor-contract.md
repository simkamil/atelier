# Contrat du démonstrateur texte brut — 2026-10-11

Baseline : `3cb335442bc3b199b9e25ab177bcce396cf2cc55`.
Une session document par instance WASM. Rust possède texte, sélection, historique,
mise en page et état modifié. JS possède DOM, viewport et transport. Les chemins et
tickets restent exclusivement natifs. Aucun nouveau paquet externe.

## Enveloppe et commandes

ATLM v1 conserve ses règles. Ajouter `Editor = 0x0020` (command) et
`EditorResult = 0x8020` (event). Une commande Editor reçoit EditorResult ou Failed,
avec le même requestId u64. Les anciennes commandes restent des sondes d'écho ;
ne pas modifier l'ABI ni leurs tests en les convertissant silencieusement.
Failed contient uniquement un code ASCII stable commençant par `EDITOR_`.

Toute charge Editor commence par `ATED`, u16 version=1, u16 opcode. Entiers et
f64 sont little-endian. Refuser versions, opcodes, drapeaux, longueurs ou nombres
non finis invalides ; aucun octet final inutilisé. Les opérations sont FIFO.

| Op | Corps après les 8 octets | Effet |
|---|---|---|
| 0 | vide | État courant ; document vide créé au premier usage |
| 1 | octets UTF-8 du fichier | Charger, préserver BOM/CRLF, vider historique, sélection 0, état propre |
| 2 | u64 now_ms, puis UTF-8 | Insérer/remplacer la sélection courante |
| 3 / 4 | u64 now_ms | Retour arrière / supprimer en avant, par graphème |
| 5 / 6 | vide | Annuler / rétablir |
| 7 | u32 anchor, u32 focus | Sélection dirigée en offsets UTF-8 hors BOM |
| 8 | u8 direction, u8 extend (0/1), u16 reserved=0 | Déplacement : 0 gauche, 1 droite, 2 haut, 3 bas, 4 début ligne, 5 fin ligne, 6 début document, 7 fin document, 8 page précédente, 9 page suivante |
| 9 | u32 page, f64 x, f64 y, u8 extend (0/1) | Clic/hit-test en points locaux à la page ; sélection la plus proche |
| 10 | vide | Export exact UTF-8 avec BOM éventuel |
| 11 | u64 exported_revision | Après sauvegarde native réussie, marquer propre uniquement si révision inchangée ; sinon EDITOR_STALE_SAVE |

La révision augmente avec chaque changement de contenu, jamais avec la sélection.
L'état modifié compare le contenu à la dernière version chargée/enregistrée, donc
annuler jusqu'à cette version redevient propre. Toute commande rejetée conserve
contenu, sélection, historique et état de sauvegarde. Prévalider le rendu avant
publication d'un nouveau contenu ; le dépassement de budget ne publie pas un
document invisible. Les allocations d'ABI sont réservées avant de publier l'état.

## Réponse d'état

Toutes les opérations sauf Export renvoient ce format dans EditorResult :

| Champ | Type |
|---|---|
| magic/version/flags | `ATES`, u16=1, u16 bits undo=1 redo=2 dirty=4 bom=8 |
| revision | u64 |
| anchor/focus/text_len/page_count | quatre u32 |
| caret_page/caret_x/caret_y/caret_height | u32 puis trois f64 |
| selection_rect_count | u32 ; fin du header à l'offset 64 |
| text | text_len octets UTF-8 hors BOM |
| pages | page_count fois : u32 len puis octets ATDL v1 |
| selection rectangles | count fois : u32 page puis f64 x/y/width/height |

Export renvoie `ATEX`, u16=1, u16 reserved=0, u64 revision, puis octets du fichier.
Les pages sont A4 595.276 × 841.89 points, marges 54 pt, Carlito Regular 11 pt,
interligne fixe 14 pt, sens latin gauche→droite. CRLF forme un seul saut de ligne,
LF/CR seuls sautent aussi une ligne ; tabulations rendues comme espaces documentés.
Le layout simplifié coupe aux graphèmes, conserve les positions pour hit-test et
navigation verticale. Utiliser les vrais contours Carlito via font/shape/geom et
ATDL FillPath ; aucun `fillText` ni dépendance à une police système. JS rejoue ATDL
avec CanvasRenderer. Le curseur clignotant et la sélection sont des overlays ;
la composition montre un aperçu souligné sans modifier le modèle avant commit.

Limites initiales explicites : 128 Kio de texte hors BOM, historique 4 Mio,
réponse d'état 32 Mio, au plus 1024 pages. Appliquer avant allocations/commit et
signaler une erreur traduisible. Recalcul et transport complets sont permis pour
ce démonstrateur ; aucune revendication de latence 300 pages, démarrage 1,5 s,
rich-text, bidi ou validation clavier/IME physique dans ce lot.

## Host fichiers natif

Messages chaîne uniquement depuis la source exacte `https://app.atelier/` :
`us120-file|1|ID|open|`, `us120-file|1|ID|save|BASE64`,
`us120-file|1|ID|dirty|0` ou `...|dirty|1`.
ID ASCII décimal positif, au plus 20 chiffres, sans réutilisation pendant la page.
Décodage base64 canonique, au plus 131075 octets (texte + BOM) ; queue de 4
requêtes, budget total borné. Refuser tout champ supplémentaire ou format inconnu.
Le callback COM copie/valide/enfile seulement ; dialogues et I/O exécutés sur STA
par le gestionnaire de fenêtre. Révoquer tous les tickets, succès comme échec.

Réponses `us120-file|1|ID|ok|BASE64` (open), `...|ok|` (save/dirty),
`...|cancelled|`, ou `...|error|MESSAGE_KEY`. Aucune réponse ne contient de chemin,
ticket ou message système brut. Filtres TXT, sauvegarde toujours via dialogue
« Enregistrer sous » dans cette première tranche. SharedBuffer reste ouvert.
La fenêtre avertit avant quitter avec modifications ; la première version propose
Quitter sans enregistrer / Annuler. L'UI demande confirmation avant Nouveau/Ouvrir
si modifié, attend la sauvegarde avant de marquer propre, restaure le focus après
dialogue et conserve l'état en cas d'annulation/erreur. Les boutons restent
désactivés avant initialisation et pendant un dialogue/une opération de fichier.

## Propriété des fichiers pendant le développement parallèle

- Moteur : core-wasm Cargo/src/tests et docs/us-120-editor-engine.md.
- Interface : web JS/HTML/CSS/catalogue, tests UI/Worker/protocole JS et
  docs/us-120-editor-ui.md. Respecter le résultat sémantique seulement pour Editor ;
  valider requestId/type/longueurs. Les compteurs de progression autorisent une
  réponse plus grande que l'entrée tout en conservant les bornes ABI.
- Natif : desktop-win/src/webview*.rs, win32.rs, nouveau file-bridge.rs et tests
  natifs associés, docs/us-113-file-bridge.md. Le coordinateur possède resources.rs.
- Coordinateur : resources.rs, packager/provenance/lock, CI, validation globale,
  specs et suivi des US, graphify. Aucun agent ne commit/push ni lance graphify.
