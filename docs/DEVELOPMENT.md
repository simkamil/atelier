# Développement

Le dépôt utilise un workspace Cargo local, Rust stable et l'édition 2024. Aucun crate de registre n'est autorisé. Les sources Rust du produit et des outils vivent sous `crates/` et `tools/` et portent l'identifiant SPDX `AGPL-3.0-or-later`. Les répertoires `web/`, `data/` et `tests/` accueillent respectivement le client, les ressources et les corpus ; ils sont actuellement des emplacements documentés.

Les crates `atelier-unicode`, `atelier-geom` et `atelier-model` fournissent respectivement les encodages de texte, les primitives géométriques et un arbre de nœuds indexé. Les autres crates produit restent des squelettes. Le garde `atelier-check-workspace`, le vérificateur de licence et `atelier-bench` sont les outils internes disponibles. Pour les commandes de compilation, formatage, Clippy, contrôle de licence et mesure, voir le README racine.

## Crates autorisant FFI

`atelier-core-wasm`, `atelier-desktop-win` et `atelier-server` sont les seules crates exemptées de l'attribut `#![forbid(unsafe_code)]`. Aucun bloc `unsafe` n'est nécessaire dans ce socle ; toute future utilisation devra être isolée et commentée selon la spécification.

## Validation

### Preuve US-060 et US-070 — 2026-10-10

La [géométrie 2D](us-060-geometry.md) et l’[arbre de nœuds indexé](us-070-indexed-tree.md) sont développés et validés localement, chacun avec neuf tests unitaires. La géométrie couvre les rectangles, les matrices affines, les extrema de Bézier, les sous-chemins/fermetures et l’aplatissement borné. Une vérification indépendante compare 200 courbes à 200 200 échantillons Bernstein et vérifie 200 inversions de transformations. La reprise d’un segment après fermeture et la distinction coordonnées/dimensions ont été corrigées pendant la revue.

L’arbre combine un AVL positionnel, un index BTreeMap et une arène radix à pages fixes. La croissance ne recopie plus tous les nœuds ; les accès aux slots sont bornés par la largeur de usize, et les slots libres sont réutilisés. Les tests vérifient rotations, parents, tailles, hauteurs, index d’identifiants, suppressions à deux enfants, frontières de pages et épuisement des compteurs. L’oracle indépendant valide 240 000 opérations mixtes sur douze graines. Les identifiants restent propres à leur arbre.

Banc release final : 100 000 blocs, 10 000 insertions aléatoires, chauffe 16.4412ms, cinq échantillons, médiane **16.6464ms**, sous le seuil de 50 ms sur Intel Core i9-13900KF (24 cœurs/32 threads), environ 64 Gio, Windows 11 Pro build 26300, Rust 1.99.0. Cette mesure ne garantit pas le même résultat sur un autre matériel ou sous une autre charge.

Après les corrections, **67 tests du workspace** passent. Les builds MSVC, WASM et musl, Clippy hôte/WASM avec `-D warnings`, rustfmt, SPDX et le garde du lock réussissent hors ligne. Les 31 paquets restent locaux, sans dépendance externe, et les octets de Cargo.lock sont inchangés. Les tests musl sont compilés seulement ; l’exécution Linux et le workflow GitHub restent à confirmer. Trois revues parallèles et une revue ciblée du stockage sont triées, sans constat différé. Preuves : [validation globale](../_bmad-output/implementation-artifacts/foundations-validation.json), [revue](../_bmad-output/implementation-artifacts/foundations-review.md), [oracle géométrique](../_bmad-output/implementation-artifacts/geometry-oracle.json), [oracle arbre](../_bmad-output/implementation-artifacts/tree-oracle.json) et [banc final](../_bmad-output/implementation-artifacts/tree-benchmark.json).

### Preuve US-014 — 2026-10-10

Les [encodages de texte](us-014-encodings.md) sont développés et validés localement : six tests unitaires passent et 5 028 cas sont comparés indépendamment aux codecs de la bibliothèque standard Python. Ce contrôle couvre les 1 024 octets des quatre encodages historiques, 4 000 séquences UTF-8 valides ou malformées, les BOM UTF-16 avec paires de surrogates et l’offset d’erreur après un caractère multi-octet. Les tables sont aussi protégées par des empreintes fixes issues des références Unicode/Apple et les tests ne requièrent pas le réseau.

Les builds Windows MSVC, WASM et musl, Clippy hôte/WASM, rustfmt et SPDX passent sans dépendance externe. Les tests musl sont compilés sur cet hôte Windows, sans exécution Linux ; le workflow GitHub n’a pas été exécuté ici. La CI WASM compile et lint désormais les trois nouvelles crates portables, et exécute leurs tests sur l’hôte natif. Preuves : [encoding-oracle.json](../_bmad-output/implementation-artifacts/encoding-oracle.json) et [foundations-validation.json](../_bmad-output/implementation-artifacts/foundations-validation.json).

### Preuve US-002 — 2026-10-10

Le garde `atelier-check-workspace` est un outil sans dépendances externes. Il lit `Cargo.lock` sans l'écrire, vérifie ses paquets contre les membres retournés par `cargo metadata --format-version 1 --locked --offline`, et refuse les sources registry/git ainsi que les paquets locaux absents du workspace. Il prend en charge le sous-ensemble Cargo.lock format 4 généré par Cargo ; les variantes manuelles non prises en charge, dont les tableaux inline non vides et les escapes TOML Unicode, sont refusées. Son bootstrap est volontairement indépendant de Cargo : depuis la racine, créer `target`, compiler avec `rustc --edition=2024 tools/check-workspace/src/main.rs -o target/check-workspace.exe`, contrôler `$LASTEXITCODE`, puis exécuter `target/check-workspace.exe .` avant toute commande Cargo. Sur Linux, omettre `.exe` et lancer `./target/check-workspace .`. Le code 1 et un diagnostic stderr signalent tout lock absent, invalide, non pris en charge ou contenant un paquet interdit.

Après l'ajout volontaire de membres ou de dépendances locales, actualiser explicitement le lock avec `cargo generate-lockfile --offline`, puis compiler et exécuter le garde. Cette commande de maintenance n'est jamais exécutée par la CI, qui ne fait que vérifier le lock présent.

Après le garde, les commandes Cargo documentées dans le README passent `--locked --offline`. La CI GitHub Actions couvre Windows MSVC, la compilation WASM et Linux musl. Les tests du moteur WASM tournent sur l'hôte Windows natif ; WASM est compilé sans prétendre que l'OS exécute ses binaires. Les tests Linux tournent sur le runner Linux avec la cible musl. Les contrôles locaux ne constituent pas une preuve d'exécution distante : les résultats Linux et ceux de GitHub ne sont confirmés qu'après exécution effective de ces jobs.

Validation indépendante après revue : bootstrap `rustc`, garde et build Windows réussis ; 43 tests du workspace passent, dont 18 pour le garde (6 unitaires et 12 CLI). Build WASM, Clippy Windows et WASM avec `-D warnings`, rustfmt et SPDX réussissent hors ligne. Cargo et le lock contiennent 31 membres locaux, sans dépendance externe. Le YAML est relu et vérifié : garde avant Cargo, modes verrouillé/hors ligne et étapes distinctes pour propager chaque échec.

Les tests couvrent les dépendances locales conformes, les sources registry/git, les paquets hors workspace, une collision de nom/version entre membre et dépendance extérieure, les locks absents, incomplets ou malformés et les échecs de metadata. Les refus conservent les octets du lock. La revue a corrigé l'acceptation de TOML malformé, la résolution incomplète de metadata et la propagation des erreurs PowerShell. Les preuves détaillées sont dans [us002-validation.json](../_bmad-output/implementation-artifacts/us002-validation.json).

La cible musl a été compilée sur Windows avec `rust-lld` et ses tests ont été compilés (`--no-run`), mais pas exécutés sur cet hôte. L'exécution des tests musl sur le runner Linux et celle du workflow GitHub restent à confirmer ; aucun résultat distant n'est déclaré ici. Le moteur est encore un squelette sans test propre ; ses futures fonctions devront enrichir les tests natifs et le contrôle WASM.

Avant d'accepter US-001, exécuter en mode hors ligne les commandes du README racine, y compris les tests du vérificateur avec les cas conformes, absents et illisibles.

### Preuve US-001 — 2026-10-09

Validé sous Windows avec Rust stable 1.99.0 : Cargo charge 29 paquets locaux (28 crates et `atelier-check-license`) sans dépendance externe ; `cargo build --workspace --offline`, `cargo test --workspace --offline`, Clippy avec `-D warnings`, rustfmt et le vérificateur de licence réussissent. `atelier-core-wasm` compile aussi pour `wasm32-unknown-unknown`. Les tests du vérificateur couvrent l'en-tête valide, les identifiants absent et incorrect, l'UTF-8 invalide, l'exclusion des dossiers générés, les liens symboliques quand la plateforme autorise leur création, et le diagnostic avec code non nul de la CLI si les racines source manquent.

Ce contrôle valide uniquement le socle vide. Les builds Linux musl et le comportement des futures fonctionnalités ne sont pas couverts par US-001.

La revue a corrigé les faux succès sur racines manquantes et le saut silencieux des liens symboliques. Vérification indépendante : les 28 crates correspondent à SPEC §6.2, les 29 paquets et Cargo.lock restent locaux, les commandes ci-dessus passent après correction. Le test des liens symboliques est conditionnel : ce poste refuse leur création (WinError 1314), donc son scénario réel reste à exercer sur un hôte autorisé.

## Bancs de performance — US-005

`atelier-bench` est un outil Rust local, sans dépendance de registre. L'API `Registry` accepte des cas de fonctions/closures, les mesure avec `Instant` et `black_box`, puis calcule la médiane en nanosecondes. La closure retourne `()` : son implémentation doit rendre le travail observable, par exemple en passant le résultat à `std::hint::black_box(result)`. Le CSV `case,iterations,median_ns` peut être relu comme référence. Le comparateur exige exactement les mêmes cas dans la mesure et la référence ; une hausse strictement supérieure à 10 % retourne le code 2, tandis qu'une hausse exactement égale à 10 % est acceptée. Les erreurs d'argument, lecture/écriture, CSV invalide, cas manquant ou dupliqué et médiane nulle sont diagnostiquées avec le code 1. Les mesures acceptent au plus 1 000 000 itérations.

Cas réellement exécutés dans le socle : parcours Unicode avec `char` et tri d'un tableau `u64` avec `sort_unstable`. Ils valident le parcours multi-cas mais ne prétendent pas mesurer les scénarios ni satisfaire les objectifs de `SPEC.md` §6.16. Les durées sous la résolution effective de `Instant` sont rapportées à 1 ns minimum. Mesurer en profil release, consigner CPU, mémoire et système, garder charge et conditions identiques, puis écarter une première exécution de chauffe. Les résultats restent sensibles à l'ordonnanceur, à la fréquence CPU et aux tâches de fond.

### Preuve US-005 — 2026-10-09

Validation indépendante après corrections : build Windows du workspace, build de `atelier-core-wasm` pour WASM, 25 tests du workspace (dont 19 pour le banc), Clippy avec `-D warnings`, rustfmt et contrôle SPDX réussissent hors ligne. Cargo et Cargo.lock contiennent exactement 30 membres locaux : les 28 crates produit de SPEC §6.2 et deux outils, sans dépendance externe. La limite d'itérations, les médianes paires/impaires et proches de `u128::MAX`, les références invalides, la préservation de la référence, les arguments OS non Unicode, la sélection et stdout sont couverts. Le test du chemin de sortie non inscriptible utilise des composants de chemin portables ; les tests du banc et Clippy ont été relancés après cette correction.

Essai release : deux cas mesurés sur 100 itérations, CSV relu et comparaison sans régression (code 0). Une référence contrôlée à 1 ns déclenche bien le code 2 ; les tests déterministes vérifient aussi 110 accepté et 111 refusé pour une référence de 100. La sortie standard reste un CSV valide avec une comparaison, une référence identique à la sortie est préservée et refusée (code 1), et les entrées extrêmes retournent un diagnostic sans panique. Ces mesures valident le fonctionnement de l'outil, pas les performances des futurs éditeurs.
