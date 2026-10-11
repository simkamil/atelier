# Images et polices embarquées — 2026-10-10

Lot US-040, US-043 et US-056 développé par trois agents GPT-6 Luna moyen.
Les trois US sont développées, relues et validées. Le backlog compte
**23 US terminées sur 158**.

- [PNG](us-040-png.md) : tous les modes couleur/profondeur, filtres et Adam7,
  transparence à la précision source, CRC et budgets.
- [GIF/BMP](us-043-gif-bmp.md) : première image GIF, palettes, LZW, offsets,
  entrelacement; BMP RGB, bitfields, RLE4/RLE8 et orientations admises.
- [Polices](us-056-embedded-fonts.md) : vingt TTF originaux, cinq familles et
  quatre styles, textes OFL complets et provenance immuable. Les données
  représentent 7 426 636 octets avant compression du paquet.

Le [corpus versionné](../tests/image-reference/README.md) contient 201 entrées,
187 valides comparées aux pixels de référence et 14 PNG invalides rejetés.
Les tests Rust consomment ces références sans Python ni réseau.

**210 tests du workspace et 34 tests release passent** (20 images, 14 polices).
Les quinze contrôles locaux couvrent Windows MSVC, WebAssembly, compilation
Linux musl, tests, Clippy hôte/WASM, formatage, licences, garde du workspace et
intégrité des ressources. Les 31 paquets restent locaux, sans dépendance Rust
externe. Les contrôles laissent les sources et le lock inchangés.

L’oracle indépendant régénère les 187 références à l’identique, soumet
149 728 mutations/troncatures sans panic et vérifie les vingt faces avec
FontTools. Onze inventaires volontairement altérés sont rejetés par le garde
des ressources. Les vingt binaires de police correspondent octet pour octet
à neuf téléchargements officiels. La revue comporte seize constats : douze
corrigés, quatre faux positifs et aucun différé.

Preuves du lot : [validation globale](../_bmad-output/implementation-artifacts/images-fonts-validation.json),
[oracle indépendant](../_bmad-output/implementation-artifacts/images-fonts-oracle.json),
[sources officielles des polices](../_bmad-output/implementation-artifacts/images-fonts-upstream.json),
[revue](../_bmad-output/implementation-artifacts/images-fonts-review.md).

Le lot précédent, commit `6e09261054a7cbb91bc63d270762ae15fea1131b`, a réussi
les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38072851039).
Le [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-model-font.json)
identifie ce commit. Le dernier [relevé de CI](../_bmad-output/implementation-artifacts/github-ci.json)
identifie séparément le commit publié et ses jobs. Le job Linux y exécute
les tests musl ; l’hôte Windows les compile seulement.

## Prochain jalon testable

Un premier [éditeur texte brut paginé](us-120-editor-ui.md) est maintenant intégré. Le [suivi courant](PROTOTYPE-PROGRESS.md) précise son lancement, ses preuves et ses limites ; les travaux Unicode et typographiques décrits ci-dessous correspondent aux lots historiques. L’US-120 complète reste ouverte pour les essais physiques et les mesures de performance.

Les prochains lots doivent faire avancer en parallèle Unicode (US-010 puis
US-011/012), polices et texte (US-051/053/055 puis US-054), et intégration
WASM/Windows (US-090 et US-110, puis leurs dépendants). Les contours TrueType
US-051 sont nécessaires au rendu Path2D demandé par US-101 ; cette
dépendance technique est désormais explicite dans le backlog. La validation
du prototype exigera un parcours réel de saisie, affichage et sauvegarde ;
le nombre d’US terminées ne mesure pas sa proximité.
