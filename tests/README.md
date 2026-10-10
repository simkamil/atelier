# Ressources de test

`format-reference/` contient des petits corpus ZIP et XML indépendants, leurs
sémantiques attendues et leurs empreintes SHA-256. Les tests Rust consomment les
fichiers versionnés sans dépendance Python. Le générateur de références est un
outil d'agent sous `_bmad-output/implementation-artifacts/`.

Les octets sont conservés tels quels par `.gitattributes`, notamment les
encodages malformés, BOM UTF-16 et fins de ligne utilisés pour tester XML.
Les corpus de documents complets et images de référence seront ajoutés avec
les stories correspondantes.

Deux archives Writer classiques/Zip64 ont été validées par Python zipfile (noms, contenus et CRC); leurs octets figés sont comparés aux sorties du writer dans les tests Rust CI. Le manifeste conserve les entrées, options et SHA-256, et le générateur reproduit ces références.
