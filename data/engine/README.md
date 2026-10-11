# Moteur WASM incorporé au shell

`atelier_core_wasm.wasm` est le véritable moteur ABI/protocole compilé depuis
`crates/core-wasm`, sous AGPL-3.0-or-later. Il est versionné pour qu'un checkout
neuf puisse compiler le shell Windows sans construction imbriquée de Cargo ni
fichier préalable dans `target/`. Il conserve encore l'écho de protocole ; les
handlers métier du document restent à intégrer.

Régénération depuis la racine, avec la cible Rust `wasm32-unknown-unknown` :

```powershell
py -X utf8 tools/package-engine.py
node tests/check-embedded-engine.mjs
node tests/check-embedded-engine.mjs --compare-artifact
```

Le manifeste contient le SHA-256 de l'artefact, le compilateur, le profil et les
empreintes des sources du crate sans dépendance (CRLF normalisés en LF pour les
checkouts Windows/Linux). Le contrôle refuse des sources
obsolètes, un module incorrect ou des exports ABI absents ; il exerce un vrai Ping
via le loader JS. `--compare-artifact` vérifie en plus l'égalité avec le résultat
Cargo local : cette reproductibilité binaire concerne le compilateur enregistré,
et n'est pas supposée entre versions de Rust.

Si le moteur acquiert des dépendances locales, étendre la liste de sources du
packager et du vérificateur à leur fermeture avant de republier l'artefact. Les
tests ABI et Worker du moteur reconstruit restent des contrôles séparés en CI.
