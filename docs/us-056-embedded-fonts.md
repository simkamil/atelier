# US-056 — Polices embarquées

Le crate `atelier-font` expose les faces intégrées dans `atelier_font::embedded::EMBEDDED_FONTS`. Chaque entrée donne le nom de famille OpenType, le poids (400 ou 700), le style (`Normal` ou `Italic`), les octets TTF statiques et le texte complet de licence (`license`). Les données sont incluses à la compilation : aucun accès disque ni réseau n’est nécessaire à l’exécution.

Le catalogue contient les quatre faces Regular, Bold, Italic et Bold Italic des familles Carlito, Caladea, Liberation Sans, Liberation Serif et Liberation Mono. Les faces peuvent être ouvertes avec `FontFace::parse(font.data)` et leurs tables `name`/`OS/2` contrôlées par les API du crate.

Les fichiers TTF totalisent 7 426 636 octets (environ 7,08 Mio) avant compression de l’application. Cette taille contribue au binaire ou à son paquet final; la taille observée dépend de la configuration de l’éditeur de liens et du conditionnement. Les binaires restent les octets amont inchangés.

La provenance immuable, la version interne des faces, le chemin upstream, les empreintes SHA-256 et les tailles se trouvent dans [`data/fonts/manifest.json`](../data/fonts/manifest.json). Les notices OFL 1.1 intégrales avec copyrights et noms réservés sont dans [`data/fonts/licenses/`](../data/fonts/licenses/) et restent consultables via `EmbeddedFont::license`. Le README des fontes donne les sources, versions, procédure de vérification et taille cumulée.

US-056 fournit les fichiers redistribuables au gestionnaire de polices US-055. Le choix de la face, le repli famille/graisse/style et l’affichage utilisateur relèvent du gestionnaire et de l’interface, pas de ce catalogue.
