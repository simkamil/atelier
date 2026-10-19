# Données embarquées

Les [polices redistribuables](fonts/README.md) de US-056 sont disponibles sous
`fonts/` : Carlito, Caladea, Liberation Sans, Serif et Mono, quatre styles par
famille. Les binaires amont inchangés, leurs licences OFL intégrales et leurs
empreintes sont versionnés. Le catalogue `atelier_font::embedded::EMBEDDED_FONTS`
expose les octets et les notices sans accès réseau ou disque.

Les 71 fichiers officiels UCD 17.0.0 et la licence Unicode V3 sont figés sous
`ucd/17.0.0/`, avec manifeste SHA-256. Le générateur local produit les
[tables Unicode versionnées](../docs/us-010-ucd-tables.md) hors ligne.
Les dictionnaires restent à ajouter avec leurs stories.

Les [modèles de documents](templates/) (lettre, CV, rapport, compte rendu) sont de
simples DOCX fournis avec l'exe comme ressources hors réseau (`/templates/*.docx`).
Ils s'ouvrent dans l'éditeur riche comme des documents sans nom ; leur ouverture et
leur réenregistrement sont vérifiés par `node tests/file-templates.mjs`.
Modèles : contenu original, AGPL-3.0-or-later.
