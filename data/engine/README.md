# Moteur WASM incorporé au shell

`atelier_core_wasm.wasm` est le véritable moteur ABI/protocole compilé depuis
`crates/core-wasm`, sous AGPL-3.0-or-later. Il est versionné pour qu'un checkout
neuf puisse compiler le shell Windows sans construction imbriquée de Cargo ni
fichier préalable dans `target/`. Les anciennes sondes conservent l'écho de
protocole ; les commandes `Editor` disposent d'une session de texte brut séparée.

Régénération depuis la racine, avec la cible Rust `wasm32-unknown-unknown` :

```powershell
py -X utf8 tools/package-engine.py
node tests/check-embedded-engine.mjs
node tests/check-embedded-engine.mjs --compare-artifact
```

Le manifeste contient le SHA-256 de l'artefact, le compilateur, le profil et les
empreintes de la fermeture des dépendances locales normales et de build,
de leurs sources Rust, scripts de build, ressources incorporées et corpus Unicode
épinglé. Les fichiers textuels sont normalisés de CRLF en LF pour les checkouts
Windows/Linux ; les ressources binaires sont hachées octet pour octet. L'inventaire
est conservateur et couvre aussi les ressources inutilisées de branches cfg/test.
Le contrôle refuse des sources
obsolètes, un module incorrect ou des exports ABI absents ; il exerce un vrai Ping
via le loader JS. `--compare-artifact` vérifie en plus l'égalité avec le résultat
Cargo local : cette reproductibilité binaire concerne le compilateur enregistré,
et n'est pas supposée entre versions de Rust.

Le packager et le vérificateur calculent indépendamment cet inventaire depuis les
métadonnées Cargo et les macros d'incorporation statiques. Le build Unicode produit
ses tables à partir du corpus vérifié ; ce cas de génération est inclus
explicitement. Toute nouvelle génération de données doit être ajoutée à cette
politique avant publication. Les tests ABI, Worker et éditeur du moteur reconstruit
restent des contrôles séparés en CI.
