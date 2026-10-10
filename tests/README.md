# Ressources de test

`format-reference/` contient des petits corpus ZIP et XML indépendants, leurs
sémantiques attendues et leurs empreintes SHA-256. Les tests Rust consomment les
fichiers versionnés sans dépendance Python. Le générateur de références est un
outil d'agent sous `_bmad-output/implementation-artifacts/`.

Les octets sont conservés tels quels par `.gitattributes`, notamment les
encodages malformés, BOM UTF-16 et fins de ligne utilisés pour tester XML.
Les corpus de documents complets restent à ajouter avec leurs stories.
Le corpus d’images de référence est décrit ci-dessous.

Deux archives Writer classiques/Zip64 ont été validées par Python zipfile (noms, contenus et CRC); leurs octets figés sont comparés aux sorties du writer dans les tests Rust CI. Le manifeste conserve les entrées, options et SHA-256, et le générateur reproduit ces références.

Les tests Rust de XML opaque, historique et SVG vivent dans leurs crates et
s’exécutent en CI sans runtime externe. Les comparaisons à ElementTree et à un
modèle d’historique, ainsi que la rasterisation SVG indépendante, sont des
contrôles complémentaires d’agent : voir le [suivi du lot](../docs/EDITING-PROGRESS.md).

Les fixtures synthétiques OpenType vivent dans `crates/font/tests/fixtures/` ;
les tests Rust du parseur, des sélections et du journal s’exécutent dans la CI
sans Python. Le contrôle complémentaire `check-model-font-oracle.py` compare
les faces à FontTools, les positions à un modèle Python et les enveloppes
binaires à `struct`/`zlib.crc32`. Ces outils restent réservés aux agents ; voir
le [suivi du lot](../docs/MODEL-FONT-PROGRESS.md).

Le [corpus d’images](image-reference/README.md) fournit PngSuite complet et des
GIF/BMP avec pixels RGBA de référence. Le test Cargo du corpus tourne dans les
jobs natifs de CI; les oracles Python sont des outils d’agent uniquement.
