# Développement

Le dépôt utilise un workspace Cargo local, Rust stable et l'édition 2024. Aucun crate de registre n'est autorisé. Les sources Rust du produit et des outils vivent sous `crates/` et `tools/` et portent l'identifiant SPDX `AGPL-3.0-or-later`. Les répertoires `web/`, `data/` et `tests/` accueillent respectivement le client, les ressources et les corpus. Les polices embarquées et les corpus de référence sont versionnés ; le client web dispose du chargeur ABI WASM, tandis que son interface reste à développer.

Les crates `atelier-unicode`, `atelier-compress`, `atelier-geom`, `atelier-model` et `atelier-render` fournissent les encodages de texte, DEFLATE/zlib, la géométrie, l'arbre indexé avec transactions atomiques, historique annuler/rétablir, sélections et journal de récupération, ainsi que la liste d'affichage et le backend SVG. `atelier-zip` et `atelier-xml` fournissent aussi les conteneurs ZIP et le XML en flux avec capture opaque ; `atelier-font` lit les tables de base OpenType et les cmap 4/6/12/14 et expose vingt polices embarquées ; `atelier-image` décode PNG, GIF et BMP en RGBA8 avec limites explicites. Les autres crates produit restent des squelettes. Le garde `atelier-check-workspace`, le vérificateur de licence et `atelier-bench` sont les outils internes disponibles. Pour les commandes de compilation, formatage, Clippy, contrôle de licence et mesure, voir le README racine.

## Crates autorisant FFI

`atelier-core-wasm`, `atelier-desktop-win` et `atelier-server` sont les seules crates exemptées de l'attribut `#![forbid(unsafe_code)]`. Aucun bloc `unsafe` n'est nécessaire dans ce socle ; toute future utilisation devra être isolée et commentée selon la spécification.

## Validation

### Preuve US-010, US-051 et US-090 — 2026-10-10

Les [tables Unicode](us-010-ucd-tables.md), les [contours TrueType](us-051-truetype-outlines.md) et l’[ABI WASM manuelle](us-090-wasm-abi.md) ont été développés en parallèle avec GPT-6 Luna moyen, relus par trois agents indépendants et validés. Le [suivi du démonstrateur](PROTOTYPE-PROGRESS.md) rassemble les résultats et les dépendances restantes. Le backlog compte **26 US terminées sur 158**.

**233 tests workspace et 43 tests release passent** dans les [21 contrôles locaux](../_bmad-output/implementation-artifacts/prototype-foundations-validation.json). Les 32 paquets restent locaux ; les nouveaux liens sont Unicode→générateur en build et font→geom. Les sources et le lock restent identiques pendant les contrôles. Le job WASM exerce les vrais exports dans Node, le panic/reset et la taille optimisée ; le job Linux exécute les tests musl, compilés seulement sur l’hôte Windows local.

L’[oracle Unicode indépendant](../_bmad-output/implementation-artifacts/prototype-ucd-oracle.json) compare 1 112 064 scalaires, quinze propriétés et dix mappings par scalaire. Un test Rust exhaustif compare les empreintes officielles en CI. FontTools valide [43 576 glyphes](../_bmad-output/implementation-artifacts/prototype-font-oracle.json), et [300 rendus](../_bmad-output/implementation-artifacts/prototype-outline-raster.json) concordent pixel par pixel. Les 20 000 mutations de fontes passent sans panic ; neuf erreurs CLI Unicode laissent les sorties intactes. La [revue triée](../_bmad-output/implementation-artifacts/prototype-foundations-review.md) ne laisse aucun constat différé.

### Validation distante du lot images et polices — 2026-10-10

Le commit `20a6a05bd69d22f4f004b7a907d2c9696b24c4ad` a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38076202790). Son [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-images-fonts.json) conserve la preuve du lot précédent.


### Preuve US-040, US-043 et US-056 — 2026-10-10

Trois US développées en parallèle avec GPT-6 Luna moyen, relues et validées : [PNG](us-040-png.md), [GIF/BMP](us-043-gif-bmp.md) et [polices embarquées](us-056-embedded-fonts.md). Le [suivi du lot](IMAGES-FONTS-PROGRESS.md) regroupe les contrats et les preuves. Le backlog compte **23 US terminées sur 158**.

**210 tests workspace et 34 tests release passent** (20 images, 14 polices). Les [quinze contrôles locaux](../_bmad-output/implementation-artifacts/images-fonts-validation.json) couvrent builds Windows/WASM/musl, compilation des tests musl, tests, Clippy hôte/WASM, format, SPDX, garde, intégrité des ressources et métadonnées des 31 paquets locaux. La seule nouvelle dépendance est locale : `atelier-image → atelier-compress`. Les sources et le lock restent identiques pendant la validation.

L’[oracle indépendant](../_bmad-output/implementation-artifacts/images-fonts-oracle.json) contrôle 201 images, dont 187 références de pixels et 14 PNG invalides, 149 728 mutations sans panic, onze inventaires altérés et vingt faces avec FontTools. Les [binaires des polices](../_bmad-output/implementation-artifacts/images-fonts-upstream.json) correspondent exactement aux sources officielles. La [revue triée](../_bmad-output/implementation-artifacts/images-fonts-review.md) ne laisse aucun constat différé. Les corpus et le garde d’intégrité fonctionnent en CI sans Python ni téléchargement ; le job Linux exécute les tests musl du commit publié.

### Preuve US-050, US-073 et US-074 — 2026-10-10

Trois US développées avec GPT-6 Luna moyen en parallèle, relues et validées : [tables OpenType](us-050-opentype-tables.md), [sélections](us-073-selection.md) et [journal de récupération](us-074-recovery-journal.md). Le [suivi du lot](MODEL-FONT-PROGRESS.md) décrit les garanties, les limites d’intégration et les preuves. Le backlog compte **20 US terminées sur 158**.

**187 tests workspace et 51 tests release passent** (40 modèle, 11 polices). Les [14 contrôles locaux](../_bmad-output/implementation-artifacts/model-font-validation.json) couvrent builds Windows/WASM/musl, compilation des tests musl, tests, Clippy hôte/WASM, format, SPDX, garde et métadonnées des 31 paquets locaux. Le modèle utilise CRC32 via la dépendance locale `atelier-compress` ; aucune dépendance externe n’est ajoutée. Les sources et le lock restent identiques pendant les contrôles.

Le [comparateur indépendant](../_bmad-output/implementation-artifacts/model-font-oracle.json) valide huit faces avec FontTools, 25 272 mutations/troncatures sans panique, 12 001 sélections et 267 cas de journal. Les fixtures synthétiques TTF/OTF/TTC sont également testées par Rust en CI. La [revue triée](../_bmad-output/implementation-artifacts/model-font-review.md) n’a aucun constat différé. La CI WASM inclut les polices ; le job Linux exécute les tests musl du commit publié, tandis que l’hôte local les compile seulement.

### Validation distante du lot édition — 2026-10-10

Le commit `77622b0c410ff9f5686325e81bca1e71cf383642` a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38068167771). Son [relevé archivé](../_bmad-output/implementation-artifacts/github-ci-editing.json) conserve la preuve du lot précédent ; il ne couvre pas les modifications du présent lot.

### Preuve US-032, US-072 et US-081 — 2026-10-10

Trois US développées en parallèle avec GPT-6 Luna moyen, puis soumises à trois revues indépendantes : [XML opaque](us-032-opaque-xml.md), [historique annuler/rétablir](us-072-undo-history.md) et [backend SVG](us-081-svg-backend.md). Le [suivi du lot](EDITING-PROGRESS.md) rassemble les preuves, les garanties et les limites ; la [revue triée](../_bmad-output/implementation-artifacts/editing-review.md) enregistre chaque constat et sa disposition.

L’[oracle indépendant](../_bmad-output/implementation-artifacts/editing-oracle.json) valide 24 captures XML dans trois encodages, 258 contrôles de namespaces, 10 000 étapes d’historique et les données SVG incorporées. Le [rendu SVG](../_bmad-output/implementation-artifacts/editing-svg-visual.json) passe huit contrôles de pixels avec Sharp/libvips/librsvg et a été inspecté visuellement. La vérification navigateur reste indisponible, comme indiqué dans le suivi du lot.

**160 tests du workspace et 76 tests release passent** (24 modèle, 19 rendu, 33 XML). Aucun constat de revue différé. La [validation globale](../_bmad-output/implementation-artifacts/editing-validation.json) couvre les quatorze contrôles locaux, dont les tests debug/release, Clippy hôte/WASM, formatage, licence, garde du lock, builds Windows/WASM/musl et compilation des tests musl. Les 31 paquets restent locaux, sans dépendance de production externe ; `atelier-render → atelier-xml` est ajouté uniquement pour les tests. Les tests musl sont compilés sur Windows ; le [workflow distant](https://github.com/simkamil/mon-bureau/actions/workflows/ci.yml) les exécute sur Linux.

### Validation distante ZIP/XML — 2026-10-10

Le commit `3aa4d19bef7e8e917b90136f80252bd988e1f7b0` a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38050195393), dont l’exécution effective des tests Linux musl. Le [relevé distant](../_bmad-output/implementation-artifacts/github-ci-formats.json) conserve cette preuve du lot précédent ; les modifications suivantes demandent leur propre validation distante.

### Preuve US-022/023 et US-030/031 — 2026-10-10

Quatre US développées avec GPT-6 Luna moyen en parallèle, relues par trois agents indépendants puis validées et terminées dans le backlog : [ZIP](us-022-023-zip.md) et [XML en flux](us-030-031-xml.md). Le [suivi du lot](FORMATS-PROGRESS.md) détaille les garanties, la récupération bornée et les preuves.

**140 tests du workspace passent**, dont 21 ZIP et 28 XML; ces 49 tests passent aussi en release. Les treize contrôles globaux réussissent : garde avant Cargo, builds Windows/WASM/musl, tests Windows et portables natifs, Clippy hôte/WASM, rustfmt, SPDX, métadonnées locales et compilation des tests musl. Les 31 paquets sont locaux, sans dépendance externe; les contrôles ne changent pas le lock. La CI couvre désormais ZIP et XML sur WASM et teste les deux sorties Writer connues conformes dans les tests Rust. Les tests musl sont compilés localement sur Windows; leur exécution Linux est assurée par le job distant du [workflow GitHub](https://github.com/simkamil/mon-bureau/actions/workflows/ci.yml).

L'oracle indépendant compare 105 lectures et 105 réécritures XML à ElementTree, vérifie 20 rejets stricts, lit 52 ZIP Python avec Rust et relit 48 ZIP Rust avec Python. Les quatre formes de descripteur 32/64 bits, avec/sans signature, passent. Le gestionnaire ZIP de l'Explorateur reconnaît et extrait les variantes classique/Zip64. Preuves : [validation](../_bmad-output/implementation-artifacts/formats-validation.json), [interopérabilité](../_bmad-output/implementation-artifacts/format-oracle.json), [Explorer](../_bmad-output/implementation-artifacts/explorer-zip.json) et [revue triée](../_bmad-output/implementation-artifacts/formats-review.md). Aucun constat différé.

### Validation distante du socle — 2026-10-10

Le commit `b9297cd3a94293a913b2b79ba66c06e3acf4024c`, publié sur [simkamil/mon-bureau](https://github.com/simkamil/mon-bureau), a réussi les trois jobs de la [CI GitHub](https://github.com/simkamil/mon-bureau/actions/runs/38032559875) : Windows MSVC avec tests et qualité, compilation/lint WASM avec tests natifs, puis build et exécution des tests Linux musl. Cette exécution lève les réserves sur la CI distante et l'exécution Linux mentionnées dans les relevés locaux historiques ci-dessous. Elle couvre les six US du socle ; les modifications suivantes doivent obtenir leur propre validation. Le [relevé distant](../_bmad-output/implementation-artifacts/github-ci-foundations.json) conserve le SHA, les jobs et leurs conclusions.

### Preuve US-020/021, US-071 et US-080 — 2026-10-10

Quatre US développées en parallèle avec GPT-6 Luna, raisonnement moyen, puis validées et marquées terminées dans le backlog :

- [US-020/021](us-020-021-compression.md) : inflate incrémental DEFLATE/zlib plafonné, blocs stockés/fixes/dynamiques, CRC32/Adler32 et encodeur LZ77/Huffman dynamique à deux niveaux. Cinq tests Rust incluent les vecteurs indépendants et seize sorties de référence vérifiées en CI. L'oracle Python zlib réussit **2 463 cas sur 75 entrées** : 300 encodages, 1 314 décodages et 849 refus de flux invalides ou dépassant le plafond. L'encodeur conserve des données proportionnelles à l'entrée ; le décodeur garde une fenêtre de 32 Kio et des tables bornées.
- [US-071](us-071-transactions.md) : opérations génériques inversibles, transaction atomique par checkpoint, adaptateur réel de l'arbre et codec ATOP borné. Six tests nouveaux couvrent notamment erreur après mutation, rollback exact avec compteur, suppression à deux enfants et 200 transactions de plusieurs opérations comparées à un modèle Vec. Une annulation réussie conserve le compteur monotone des IDs publiés ; le checkpoint de l'adaptateur clone l'arène et les valeurs.
- [US-080](us-080-display-list.md) : commandes indépendantes des backends et codec ATDL 1.0 ; treize tests couvrent toutes les commandes, les troncatures, les valeurs invalides et les budgets. Les compteurs forgés ne provoquent pas de réservation disproportionnée et la taille de sortie est vérifiée avant allocation. Clips rectangulaires en coordonnées de page, piles indépendantes de clips et transformations ; les backends restent à développer.

**91 tests du workspace passent**. Les treize contrôles globaux réussissent : garde avant Cargo, builds MSVC/WASM/musl, tests natifs, Clippy hôte/WASM avec `-D warnings`, rustfmt, SPDX, métadonnées locales et compilation des tests musl. Les 31 paquets restent locaux, sans dépendance externe. La seule modification volontaire du lock ajoute `atelier-render → atelier-geom` ; les contrôles ne modifient ensuite aucun de ses octets. La CI WASM inclut désormais compression et rendu. La [CI du commit `ff7f3a3097ae23792afda188c3b5d0b739b76b5d`](https://github.com/simkamil/mon-bureau/actions/runs/38034970016) a ensuite réussi ses trois jobs Windows, WASM et Linux musl, avec exécution effective des tests Linux. Le [relevé distant](../_bmad-output/implementation-artifacts/github-ci-codecs.json) conserve les conclusions de ce commit.

Non-régression du banc AVL release : 100 000 blocs, 10 000 insertions, cinq échantillons après chauffe ; médiane **17,8943 ms**, sous le seuil de 50 ms, sur le même hôte i9-13900KF/64 Gio/Windows que le relevé du socle. Ce résultat reste propre à l'hôte et à sa charge.

Trois revues parallèles sont triées, sans constat différé. Preuves : [validation globale](../_bmad-output/implementation-artifacts/next-batch-validation.json), [revue et corrections](../_bmad-output/implementation-artifacts/next-batch-review.md), [interopérabilité et ratios](../_bmad-output/implementation-artifacts/compression-oracle.json), [contrôles complémentaires et banc](../_bmad-output/implementation-artifacts/next-batch-extra-checks.json). Les outils Python de validation sont réservés aux agents ; la CI produit utilise Rust.

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
