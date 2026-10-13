# Préparation du démonstrateur — 2026-10-13

Le backlog compte désormais **51 US terminées sur 158**. Le dernier lot valide
les composants accessibles, les thèmes et le ruban (US-104/105/106), avec des agents GPT-6 Luna moyen.
L’ouverture DOCX donne toujours un aperçu A4 en lecture seule.
Le glisser-déposer TXT, les espaces françaises et le cache incrémental restent disponibles.
US-004 reste partielle pour la mémoire réelle; US-210/211/212 pour les preuves Word.

- [Unicode](us-010-ucd-tables.md) : 71 fichiers officiels Unicode 17.0.0,
  licence et SHA-256, générateur local et build reproductible. L’[oracle](../_bmad-output/implementation-artifacts/prototype-ucd-oracle.json)
  compare 1 112 064 scalaires, quinze propriétés, dix mappings par scalaire,
  159 801 noms, 5 914 décompositions et seize règles de casse conditionnelle.
- [Contours TrueType](us-051-truetype-outlines.md) : glyphes simples et composites,
  limites cumulées et erreurs typées. Les 43 576 glyphes des vingt faces
  [concordent avec FontTools](../_bmad-output/implementation-artifacts/prototype-font-oracle.json).
  Les [300 rendus comparés](../_bmad-output/implementation-artifacts/prototype-outline-raster.json)
  sont identiques pixel par pixel avec le même rasteriseur indépendant ;
  20 000 mutations/troncatures ciblées ne déclenchent [aucune panique](../_bmad-output/implementation-artifacts/prototype-outline-mutations.json).
- [ABI WASM](us-090-wasm-abi.md) : allocations suivies, échange binaire, copie et
  libération JS, erreur et nouvelle instance après un vrai panic. La référence
  optimisée mesure 52 323 octets, avec seuil de régression à 57 555 octets.
  Les tests tournent sur les vrais artefacts WASM dans Node.

Un premier [éditeur de texte brut intégré](us-120-editor-ui.md) est disponible :
`cargo run -p atelier-desktop-win --release --locked --offline` démarre la fenêtre,
avec Nouveau, Ouvrir, Enregistrer sous, saisie, sélection et annuler/rétablir.
Le [moteur](us-120-editor-engine.md) produit des pages A4 et des contours Carlito
11 pt ; le [pont natif](us-113-file-bridge.md) utilise les dialogues TXT et le broker
atomique. Texte UTF-8, BOM et fins de ligne sont conservés. Le plafond courant est
128 Kio de texte, avec budget de réponse de 32 Mio ; le façonnage simplifié est
latin gauche à droite, graphème par graphème.

Ce jalon permet des premiers essais utilisateur. US-120 reste ouverte pour
les dialogues natifs vérifiés manuellement, le clavier AZERTY/IME physique et
les mesures de latence sur 300 pages/démarrage à froid. US-101 conserve les
critères de cache et de fluidité matériels ; US-103 conserve les essais de clavier.
US-113 conserve SharedBuffer, polices/URL et les essais de dialogues. US-111
conserve son échec strict de handles. US-092 reste ouverte pour l’annulation
métier : le dispatch WASM synchrone n’est pas préemptible. US-114 est validée.

## Composants, thèmes et ruban — 2026-10-13

**US-104, US-105 et US-106 sont validées.** Le [contrat et le parcours de test](us-104-105-106-interface.md) décrivent les onze composants, les six onglets, les info-touches Alt, Ctrl+F1 et Affichage → Apparence. Les commandes TXT/DOCX existantes sont conservées; les commandes futures sont désactivées. Les palettes ne changent pas les pixels du document. US-102 reste attribuée à l’autre développeur; les libellés sont injectés depuis le catalogue existant.

Trois agents GPT-6 Luna moyen ont développé des fichiers distincts; trois autres ont effectué des revues indépendantes. Les défauts confirmés ont été corrigés, notamment focus, réactivation, menus, valeurs et contraste élevé. Le parent a intégré le lot, vérifié les captures et finalisé la cartographie après la limite d’usage de l’agent chargé du graphe. Aucun défaut confirmé de ce lot n’est reporté.

Le [reçu local](../_bmad-output/implementation-artifacts/ui-shell-validation.json) passe **17 contrôles**, avec **930 tests Rust réussis** (2 essais optionnels ignorés), **112 assertions UI normales**, **114 en contraste élevé**, **118 assertions éditeur** et **36 Canvas/saisie**. La dernière correction touchait uniquement CSS/harness : les résultats Rust sont conservés après comparaison de 964 entrées identiques, avec une nouvelle passe des tests natifs de ressources. La passe finale fige 1 042 entrées sans modification pendant les contrôles. Clippy, formatage, licences, garde Cargo, package WASM et build release passent.

La sonde réelle WebView2 sert les **28 ressources**, exerce Worker/WASM et saisie/undo/redo, vérifie les refus/CSP et libère ses callbacks (`0→0`). Le journal reste borné à 64 entrées; les 8 rotations du démarrage/probes sont rapportées, avec toutes les sondes obligatoires conservées. Le WASM emballé reflète aussi les imports déjà fusionnés avant ce lot : **3 977 451 octets**, SHA-256 `1da13e999f36c37f56bf6446122b5c618d07678826a3000d6ea4ec7093d0423a`, sous le seuil existant; aucune nouvelle référence de taille n’est fixée.

Les vues claire/sombre/étroite et contraste élevé ont été inspectées. Les essais physiques AZERTY/IME, NVDA/Narrator et les mesures matérielles restent distincts de ces tests automatiques. Le lot est publié sur une branche isolée pour la fusion avec les travaux parallèles; les résultats locaux ne valent pas résultat de CI distante.


CI distante du commit `3aeab6e` : les trois jobs [du run 38354586127](https://github.com/simkamil/mon-bureau/actions/runs/38354586127) n’ont pas démarré, bloqués par la facturation ou le plafond du compte GitHub. La [preuve des annotations](../_bmad-output/implementation-artifacts/ui-shell-ci.json) distingue ce blocage des 17 contrôles locaux réussis.

## Aperçu DOCX intégré — 2026-10-12

Le MVP ajoute **Ouvrir DOCX…**, un aperçu A4 en lecture seule et **Retour au texte** / Échap. Le document TXT, sa sélection, son historique et son état de sauvegarde sont préservés. Le rendu visible des styles, les erreurs/annulations, les réponses tardives, le défilement horizontal et le filtre natif DOCX sont vérifiés. Voir le [contrat et les limites](rich-docx-preview.md).

Trois agents GPT-6 Luna moyen ont développé des fichiers distincts, puis trois autres ont effectué la revue indépendante selon les [pratiques OpenAI appliquées](SUBAGENT-WORKFLOW.md). Les correctifs sont intégrés. Le backlog reste à **48/158 US complètes** : US-301/210 gagnent une intégration visible mais l'édition/enregistrement DOCX et les comparaisons Word restent ouverts.

Le [reçu](../_bmad-output/implementation-artifacts/rich-docx-preview-validation.json) passe **24 contrôles, 760 tests Rust Windows, 78 tests de layout release et 111 assertions navigateur**, sur 856 entrées immuables. Windows debug/release, WASM, Clippy, format, licences, package, ABI et Worker passent. Les tests musl sont compilés, sans prétendre avoir été exécutés sous Linux.

WASM : **3 883 549 octets**, SHA-256 `9137af551246ea89b340ce5acc7f3e063b76522e5cc986caf6dca0ee63189e33`. La précédente version distribuée mesurait 1 046 099 octets ; l'import ZIP/XML/OOXML, le layout riche et les trois faces Carlito supplémentaires expliquent cette croissance fonctionnelle. La nouvelle référence mesurée conserve une marge de régression de 10 %. Le binaire à tester est `target/release/atelier.exe`.

## Glisser-déposer, typographie française et performance — 2026-10-12

Trois agents GPT-6 Luna moyen ont développé ce lot en parallèle, puis trois
autres agents ont mené des revues indépendantes. **US-228 est validée dans
l'éditeur TXT : 48 US terminées sur 158.**

- [US-224](us-224-text-drag-drop.md) : déplacement, copie avec Ctrl, indicateur
  d'insertion et transaction unique annulable. Le dépôt `text/plain` est raccordé
  au vrai moteur; l'essai natif depuis une autre application Windows reste à
  effectuer, donc l'US reste partielle.
- [US-228](us-228-french-spacing.md) : espaces insécables françaises à la frappe,
  option accessible activée par défaut et préférence locale. Ctrl+Z immédiat
  annule seulement la substitution. Collage, dépôt et composition restent exacts.
  La saisie riche US-220 reste ouverte.
- [US-213](us-213-incremental-layout.md) : suppression de copies profondes du
  cache. Deux séries de sept éditions locales sur 300 pages donnent des médianes
  de **71,778 et 73,641 ms**, contre 268,214 ms lors du lot précédent. La mise à
  jour identique mesure 0,033–0,037 ms; 3 pages sont recalculées et 297 réutilisées.
  Le préflight global reste explicite et **la cible UI de 16 ms reste ouverte**.
  La référence répétée avant optimisation était bruitée (médiane 413,422 ms).

La [validation immuable](../_bmad-output/implementation-artifacts/editor-drag-spacing-perf-validation.json)
passe **23 contrôles, 749 tests workspace Windows, 78 tests text-layout
release et 82 assertions navigateur** : Clippy hôte/WASM, format, SPDX,
ABI réelle, Worker, application, compilation Windows/WASM/musl et tests musl
compilés seulement. Les 850 entrées et Cargo.lock restent inchangés pendant
les contrôles. Les 33 paquets sont locaux. Le WASM embarqué mesure 1 046 099 octets
(plafond inchangé : 1 180 429).

La [revue triée](../_bmad-output/implementation-artifacts/editor-drag-spacing-perf-review.json),
l'[audit des matrices](../_bmad-output/implementation-artifacts/editor-drag-spacing-perf-matrix.json),
les [mesures répétées](../_bmad-output/implementation-artifacts/editor-incremental-perf-benchmark.json)
et le [reçu Graphify](../_bmad-output/implementation-artifacts/editor-drag-spacing-perf-graph.json)
conservent les preuves et les limites.

Le code est publié dans [2c1f16c](https://github.com/simkamil/mon-bureau/commit/2c1f16cb1d7be958cd79359bc043ccfe109bc11c). La [CI de ce commit](https://github.com/simkamil/mon-bureau/actions/runs/38306301922) refuse les trois jobs avant démarrage pour paiement/plafond Actions. Aucun test distant n'a été exécuté. Voir le [reçu CI](../_bmad-output/implementation-artifacts/editor-drag-spacing-perf-ci.json).

## Document initial, pagination incrémentale et bordures — 2026-10-12

Trois agents GPT-6 Luna moyen ont développé les API des US-202, US-213 et
US-215 en parallèle; trois agents indépendants ont relu le lot et deux agents
ont renforcé les régressions après triage. **47 US terminées sur 158** : ces
trois US restent partielles pour leurs critères utilisateur.

- [Document initial](us-202-default-document.md) : profil A4, marges 25 mm,
  Carlito 11 pt, langue française et catalogue de 25 styles. Géométrie et
  ratio exact 1,08 fournis dans les métadonnées; Auto259/240 dans le modèle.
  Fabrique Rust validée, bouton Nouveau riche encore à raccorder.
- [Mise en page incrémentale](us-213-incremental-layout.md) : cache sémantique,
  invalidation par révision de police et reprise réelle du flux de pagination.
  Une édition au milieu de 300 pages recalcule 3 pages et en réutilise 297;
  l’égalité avec un calcul complet couvre aussi les séquences d’éditions.
- [Bordures et trames](us-215-paragraph-borders.md) : cadres partagés et
  multipages, coins fermés, traits entre paragraphes, doubles/tirets/points,
  trames indépendantes et commandes DisplayList/SVG. Politiques externes,
  sans persistance ni import OOXML ni raccordement à l’éditeur riche.

La [validation immuable](../_bmad-output/implementation-artifacts/rich-incremental-borders-validation.json)
passe **21 contrôles, 739 tests workspace Windows et 78 tests
text-layout release**, avec Clippy hôte/WASM, format, SPDX, moteurs WASM réel,
ABI/Worker/navigateur, compilation Windows/WASM/musl et tests musl compilés
seulement. 844 entrées et Cargo.lock restent inchangés.
Les 33 paquets restent locaux, sans dépendance de registre ni cycle Cargo.
Le WASM embarqué mesure 1 041 521 octets (plafond inchangé : 1 180 429).
La [revue](../_bmad-output/implementation-artifacts/rich-incremental-borders-review.md),
l’[audit des matrices](../_bmad-output/implementation-artifacts/rich-incremental-borders-matrix.json)
et le [reçu Graphify](../_bmad-output/implementation-artifacts/rich-incremental-borders-graph.json)
conservent les preuves.

Le benchmark CPU release mesure **1912.351 ms au premier calcul et
268.214 ms pour l’édition locale**, sur i9-13900KF/64 Gio. Il compte 16 500
unités de prévalidation globale et 217 unités de flux local. **La cible de
frappe à moins de 16 ms n’est pas atteinte ni certifiée.** Prévalidation et
copies de géométrie restent à optimiser. Les [mesures](../_bmad-output/implementation-artifacts/rich-incremental-borders-benchmark.json)
et la [machine](../_bmad-output/implementation-artifacts/rich-incremental-borders-host.json)
sont enregistrées. L’interface testable reste TXT; aucune fidélité Word
supplémentaire n’est établie.

Le code est publié dans [51034b3](https://github.com/simkamil/mon-bureau/commit/51034b31d2871d5f3245c6ef6aaab620b685ec92). La [CI de ce commit](https://github.com/simkamil/mon-bureau/actions/runs/38296298769)
refuse les trois jobs avant leur démarrage pour paiement/plafond Actions;
voir le [reçu CI](../_bmad-output/implementation-artifacts/rich-incremental-borders-ci.json).
Aucun test distant n’a été exécuté.

## Tabulations, pagination et curseur riche — 2026-10-12

Trois agents GPT-6 Luna, raisonnement moyen, ont développé US-211, US-212 et
US-214 en parallèle, puis trois autres agents ont relu le lot indépendamment.
**US-214 est validée : 47 US terminées sur 158.** US-211 et US-212 restent
partielles pour la comparaison authentique avec Word.

- [Tabulations](us-211-tabs.md) : gauche, centre, droite, décimal virgule,
  barres et trois points de suite; grille 1,25 cm, retraits et wrapping testés.
- [Pagination](us-212-pagination.md) : espacements contextuels, solidarités,
  veuves/orphelines, sauts explicites prioritaires et diagnostics des contraintes
  impossibles. Toutes les lignes sont conservées une fois, avec budgets vérifiés.
- [Curseur riche](us-214-caret-geometry.md) : position/point, graphèmes et
  clusters, sélection entre pages, navigation verticale préservant colonne
  et affinité aux coupures souples. Arrêts internes de ligatures interpolés
  uniformément sans GDEF. L'adaptateur `flow::caret_from_pagination` teste la
  chaîne avec les vraies polices, les tabs et les espacements du layout.

La [validation finale](../_bmad-output/implementation-artifacts/rich-page-geometry-validation.json)
passe **20 contrôles, 707 tests workspace et 57 tests text-layout release** :
builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM,
format, SPDX, garde, reproduction DOCX, ABI réelle, Worker et navigateur.
Les sources et le lock restent inchangés pendant ces contrôles. Les 33 paquets
restent locaux, sans dépendance de registre ni cycle Cargo. Le moteur embarqué
reste à 1 041 817 octets, sous le plafond inchangé de 1 180 429.
La [revue triée](../_bmad-output/implementation-artifacts/rich-page-geometry-review.md),
l'[audit des matrices](../_bmad-output/implementation-artifacts/rich-page-geometry-matrix.json)
et le [reçu Graphify](../_bmad-output/implementation-artifacts/rich-page-geometry-graph.json)
conservent les preuves et limites.

US-210/212 attendent la mesure de fidélité Word; US-211 le dépassement de marge
observé dans Word. L'interface testable reste TXT; ces API ne raccordent pas
encore DOCX ou le layout riche à l'éditeur. Les tests musl sont compilés ici,
sans exécution Linux locale. La [CI du code publié](https://github.com/simkamil/mon-bureau/actions/runs/38267149521)
refuse les trois jobs avant démarrage pour paiement/plafond Actions; voir le [reçu CI](../_bmad-output/implementation-artifacts/rich-page-geometry-ci.json).

## CFF, CFB, backend raster et fuzz — 2026-10-12

Les quatre branches livrées en parallèle ont été fusionnées en conservant leurs
historiques et les ajouts DOCX/layout. Trois agents GPT-6 Luna moyen ont corrigé
les modules, puis trois agents indépendants ont conduit la revue. **US-052,
US-082 et US-311 sont validées** : **46 US terminées sur 158**. **US-004 reste
partielle** : mutations, minimisation, durée et sauvegarde panic/timeout sont
testées, mais les allocations mémoire réelles ne sont pas mesurées.

- [CFF](us-052-cff-outlines.md) : charsets prédéfinis, chaînes standard,
  FontMatrix, `random`, `seac` et limites. Un accent `seac` imbriqué conserve
  maintenant le décalage parent ; UPEM nul est refusé. L'[oracle FontTools](../tests/font-cff-reference/README.md)
  compare trois contours synthétiques versionnés ; il ne couvre ni CID réel
  ni FontMatrix indépendamment.
- [CFB](us-311-cfb-container.md) : V3/V4, FAT/DIFAT/MiniFAT, répertoire,
  lectures partielles, cycles et métadonnées invalides. Appartenance des
  entrées et casse simple Unicode vérifiées ; [deux références natives Windows](../tests/cfb-reference/README.md)
  sont relues par Structured Storage puis par Rust.
- [Backend raster](us-082-raster-backend.md) : replay de listes d'affichage
  via US-061, résolution, tuiles, clips, texte, images et PNG. Les trois PNG
  versionnés sont des références de régression produites par le backend.
- [Fuzzer](us-004-fuzzer.md) : 17 cibles, dont DOCX et CFB. Un timeout pendant
  la minimisation sauvegarde la candidate exacte et arrête le runner ;
  `--inflight` précède aussi ces appels.

La [validation finale](../_bmad-output/implementation-artifacts/foundation-integration-validation.json)
réussit **25 contrôles, 671 tests workspace et 55 tests natifs release** :
builds Windows/WASM/musl, compilation des tests musl, Clippy hôte/WASM,
format, SPDX, garde, 33 paquets locaux, reproduction des références et
**17 000 exécutions fuzz sans défaillance détectée** (pas une preuve d'absence
de défaut). ABI éditeur, transferts du vrai Worker et navigateur passent.
Sources et lock restent inchangés pendant tous ces contrôles.

Le moteur embarqué mesure **1 041 817 octets**, sous le plafond inchangé de
1 180 429. `wasm-release` retire les sections de symboles ; sa provenance et
ses octets sont vérifiés. La [revue triée](../_bmad-output/implementation-artifacts/foundation-integration-review.md)
conserve chaque constat et sa décision ; la [carte Graphify](../_bmad-output/implementation-artifacts/foundation-integration-graph.json)
contrôle les dépendances et les 158 statuts.

US-210 attend toujours la comparaison Word. Le parcours utilisateur actuel
reste l'éditeur TXT ; ces API ne raccordent pas encore les documents riches
à l'interface. Les tests musl sont compilés ici, sans exécution Linux locale.
La [CI du code publié](https://github.com/simkamil/mon-bureau/actions/runs/38238015736)
a refusé les trois jobs avant démarrage pour paiement/plafond Actions. Aucun
build ni test distant exécuté ; le [reçu CI](../_bmad-output/implementation-artifacts/foundation-integration-ci.json)
conserve le diagnostic. Aucun réglage de facturation modifié.

## Import DOCX et lignes riches — 2026-10-12

Deux agents GPT-6 Luna, raisonnement moyen, ont développé les API en parallèle,
puis trois agents indépendants ont conduit la revue. **US-301 est validée**;
le backlog atteint **43 US terminées sur 158**. **US-210 reste partielle** :
les cinq critères fonctionnels sont testés, mais le seuil de 95 % de coupures
identiques à Word reste ouvert, Word étant absent de cet hôte.

- [Import DOCX](us-301-docx-import.md) : texte, propriétés, styles/thème,
  paramètres, styles latents, fontTable et polices ODTTF réellement utilisables.
  Les valeurs non représentées par le modèle sont préservées et signalées.
- [Lignes riches](us-210-rich-line-layout.md) : coupures UAX14, quatre alignements,
  retraits, interlignes, tailles/polices mixtes, clusters et géométrie bornée.
- [Fixture indépendante](../tests/docx-reference/README.md) : parties relocalisées,
  vraie police Carlito, vecteur ODTTF et paquet opaque; import vers layout testé.

La [validation finale](../_bmad-output/implementation-artifacts/docx-rich-layout-validation.json)
réussit **15 contrôles et 540 tests workspace** : builds Windows/WASM/musl,
Clippy hôte/WASM, format, licences, garde, métadonnées et reproduction de fixture.
Sources et lock sont inchangés pendant les contrôles; les 32 paquets et toutes
leurs dépendances restent locaux. Les tests musl sont compilés localement.
La [CI du code publié](https://github.com/simkamil/mon-bureau/actions/runs/38227439851)
n’a lancé aucun job : GitHub signale un échec de paiement ou un plafond de dépenses
Actions à relever. Aucun build ni test distant n’a été exécuté; voir le [reçu CI](../_bmad-output/implementation-artifacts/docx-rich-layout-ci.json).
Le moteur embarqué reste à 1 179 080 octets, avec le même plafond; sa provenance
inclut le nouveau lock.

La [revue](../_bmad-output/implementation-artifacts/docx-rich-layout-review.md)
conserve 19 constats séparés, leurs réfutations ou corrections, et une relecture
ciblée des correctifs. La [première passe](../_bmad-output/implementation-artifacts/docx-rich-layout-first-attempt.json)
conserve l’arrêt Clippy sur une assertion booléenne corrigée; les échecs de
développement restent dans `docx-rich-layout-exploration.json`.
Les resets de couleur/surlignage nécessitent encore une représentation dans
le modèle partagé; ils produisent une notice explicite en attendant.

Le [collecteur Word](../tests/text-layout-reference/README.md) est préparatoire,
sans référence collectée ni résultat ≥95 % revendiqué. La [sonde COM](../_bmad-output/implementation-artifacts/us-210-word-com-check.json)
confirme l’absence de Word; le démarrage PowerShell bloqué est tracé séparément.
Ces API ne sont pas encore raccordées à l’éditeur riche : le parcours utilisateur
actuel reste TXT. La [carte Graphify](../_bmad-output/implementation-artifacts/docx-rich-layout-graph.json)
contrôle les dépendances et les statuts du lot.

## Styles, OPC, JPEG et raster logiciel — 2026-10-12

Le lot développé en parallèle avec GPT-6 Luna, raisonnement moyen, termine
**US-201, US-300, US-042 et US-061**. La même passe valide l’intégration des
livraisons précédentes **US-013, US-041 et US-200** : le backlog atteint
**42 US terminées sur 158**. À la date de ce lot, US-052 restait partielle (charsets Expert, `seac`,
`random` et `FontMatrix` non pris en charge) ; elle est complétée par le lot ci-dessus.

- [Styles](us-201-style-engine.md) : cascade, styles liés explicites, bascules Word,
  thèmes et cache invalidé par opération ; 12 tests dédiés.
- [OPC](us-300-opc-package.md) : parties opaques, types MIME, relations, URI
  relatives, propriétés core/app et écriture déterministe ; 14 tests dédiés.
- [JPEG](us-042-jpeg.md) : baseline/progressif, redémarrages, gris/YCbCr/CMYK/YCCK,
  orientation EXIF ; 11 tests, 17 références positives indépendantes, erreur
  moyenne ≤ 1 par canal et fixture malformée de redémarrage progressif.
- [Raster logiciel](us-061-software-rasterizer.md) : remplissages, contours,
  clips, images transformées, alpha et sortie PNG ; 13 tests dédiés.

La [validation finale](../_bmad-output/implementation-artifacts/styles-formats-raster-validation.json)
réussit **41 contrôles, 507 tests workspace, 55 tests natifs release et 57
assertions de l’éditeur dans le navigateur**. Clippy Windows/WASM, format,
licences, build Windows/WASM/musl et compilation des tests musl passent.
Les 32 paquets Cargo sont locaux ; aucune dépendance externe ajoutée.
Les sources et le lock restent inchangés pendant cette passe. Le moteur embarqué
mesure 1 179 080 octets, sous le plafond existant de 1 180 429 octets.
La [copie isolée](../_bmad-output/implementation-artifacts/styles-formats-raster-portable.json)
charge ses 18 ressources et exécute Worker, saisie, pixels, undo/redo sans web/data.

La [revue](../_bmad-output/implementation-artifacts/styles-formats-raster-review.md)
trace 38 constats et leur décision. R6 conserve une comparaison avec Word à
réaliser pour les tableaux imbriqués ; aucune divergence n’est établie à ce stade.
Les deux passes interrompues par Clippy puis rustfmt sont conservées dans
`styles-formats-raster-exploration.json` et `styles-formats-raster-format-attempt.json`.
La dernière passe reprend tous les contrôles sur les sources corrigées.

À la date de ce lot, l’import WordprocessingML restait à développer; il est
maintenant livré par US-301 ci-dessus. Le backend raster de liste d’affichage
(US-082) est maintenant livré par le lot ci-dessus ; le raccordement à l’éditeur riche reste séparé. Les diagnostics WebView2 debug/release sont
fonctionnels, mais US-111 reste ouverte : le test strict des handles échoue
encore. Les essais AZERTY/IME physiques et les dialogues manuels restent ouverts.
Les tests musl sont seulement compilés localement ; leur exécution appartient
au job Linux distant du commit publié. Le graphe et les statuts des 158 US
sont contrôlés par le [reçu Graphify](../_bmad-output/implementation-artifacts/styles-formats-raster-graph.json).


## Validation du premier éditeur intégré — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle le moteur, l’interface
et le pont fichiers, puis corrigé le lot après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/editor-integration-validation.json)
réussit **40 contrôles, 368 tests workspace et 55 tests natifs release**,
avec sources et lock inchangés. Le navigateur utilise le vrai Worker/WASM/Canvas
et un Host factice : saisie/sélection, pagination, composition, annulations,
sauvegarde obsolète, raccourcis de la vraie page et miroir textuel accessible.
L’ABI réelle vérifie aussi le rollback d’un candidat lorsque le registre de
128 Mio ne peut réserver sa réponse.

Les diagnostics WebView2 debug/release chargent les **17 ressources** et vérifient
saisie accentuée, pixels de glyphes, undo/redo, HTTP et six catégories CSP.
La [copie isolée](../_bmad-output/implementation-artifacts/editor-integration-portable.json)
fonctionne sans dossier web/data. Les tests de routage natif injectent des résultats
de dialogue contrôlés et passent par le broker réel ; ils n’ouvrent aucun dialogue.
La [revue](../_bmad-output/implementation-artifacts/editor-integration-review.md)
conserve les 12 constats séparément : cinq corrections retenues et trois fonctions
V1 reportées (presse-papiers, navigation par mot, colonne préférée), avec les rejets
motivés. Les limites de wrapping et de Tab sont également décrites dans le contrat UI.

Le premier parcours TXT est testable avec `cargo run -p atelier-desktop-win --release --locked --offline`. La tranche reste limitée à
128 Kio, latin gauche à droite et mise en page complète à chaque modification.
Les dialogues manuels, AZERTY/IME physique et mesures de 300 pages/démarrage restent
ouverts pour US-120. US-113 conserve SharedBuffer, polices/URL ; US-111 conserve
l’échec strict de handles. Les **35/158 US complètes** sont inchangées. Les tests
musl sont compilés localement ; leur exécution appartient à la CI Linux publiée.

## Édition TXT au clavier, à la souris et presse-papiers — 2026-10-11

Trois agents GPT-6 Luna moyen ont développé en parallèle la navigation, la
sélection souris et le presse-papiers, puis trois revues indépendantes ont été
triées. Les [contrats clavier](us-221-keyboard-editing.md),
[souris](us-222-mouse-selection.md) et [texte brut](us-225-plain-clipboard.md)
décrivent les comportements livrés : Ctrl+flèches par mot/paragraphe,
Ctrl+effacement par mot, colonne visuelle conservée, Ctrl+A, F8/Échap,
glisser avec défilement aux bords, double/triple-clic et copier/couper/coller.

La [validation finale](../_bmad-output/implementation-artifacts/editing-ergonomics-validation.json)
réussit **41 contrôles, 377 tests workspace et 55 tests natifs release**.
Les 57 assertions du navigateur exercent le vrai Worker/WASM ;
l’ABI réelle refuse onze SelectPoint invalides sans mutation. Les sources et le
lock sont inchangés pendant les contrôles. Le shell embarque **18 ressources**,
et la [copie isolée](../_bmad-output/implementation-artifacts/editing-ergonomics-portable.json)
fonctionne sans dossier web/data. La [revue](../_bmad-output/implementation-artifacts/editing-ergonomics-review.md)
conserve 17 constats et leurs décisions, sans nouveau report. Les anciens reports
copy/cut, mot et colonne sont résolus pour cette tranche TXT.
Un timeout d’initialisation WebView2 est conservé dans
`editing-ergonomics-retry-validation.json` ; les contrôles restants ont été repris
sur les mêmes sources, avec des dossiers de diagnostic natifs isolés.
La sonde portable a également réussi à la seconde tentative, avec le même
exécutable ; `editing-ergonomics-portable-first-attempt.json` conserve son timeout
ExecuteScript initial. Ces reprises ne prouvent pas l’absence d’instabilité du runtime.

Le backlog reste à **35/158 US complètes**. US-214/220/221/222/223/225 restent
ouvertes : objets riches, phrases/marge/bloc rectangulaire, formats HTML/interne
et images, ainsi que les essais physiques, ne sont pas tous livrés. US-111
conserve son échec strict de handles. Les événements synthétiques ne valident pas
AZERTY/IME physique ; les tests musl sont compilés localement et exécutés en CI Linux.

Les preuves des lots antérieurs ci-dessous décrivent leurs périmètres à leur date.
La référence WASM de 52 323 octets concernait le transport écho ; l’éditeur ajoute
le modèle, Unicode, le façonnage et Carlito. La [nouvelle référence mesurée](../_bmad-output/implementation-artifacts/editor-integration-wasm-size.json)
conserve le seuil de régression de 10 %.

Les [21 contrôles du lot précédent](../_bmad-output/implementation-artifacts/prototype-foundations-validation.json)
passent : 233 tests workspace, 43 tests release, builds Windows/WASM/musl,
compilation des tests musl, Clippy hôte/WASM, format, SPDX, garde du lock,
reproductibilité, intégrité des ressources, tests Node et suivi de taille.
Les 32 paquets et toutes leurs dépendances sont locaux. Les [trois revues](../_bmad-output/implementation-artifacts/prototype-foundations-review.md)
ne laissent aucun constat différé. Les sources et le lock ne changent pas
pendant les contrôles ; la CI publiée exécute ensuite les tests sur Linux musl.

US-011, US-053 et US-091 ont été développées en parallèle avec GPT-6 Luna moyen,
puis relues et validées :

- [Segmentation](us-011-segmentation.md) : 766 cas GraphemeBreakTest et
  1 944 cas WordBreakTest officiels, navigation à chaque position UTF-8,
  itération inverse et longues suites de contexte en temps linéaire.
- [Crénage et ligatures](us-053-kerning-ligatures.md) : GSUB liga,
  GPOS kern et repli kern historique, clusters conservés et budgets cumulés.
  Les 54 716 [références HarfBuzz](../tests/font-layout-reference/README.md)
  concordent sur vingt faces ; 10 000 mutations ne déclenchent aucune panique.
- [Protocole binaire](us-091-binary-protocol.md) : dix messages versionnés,
  IDs u64/BigInt, codecs Rust/JS croisés, golden frames, erreurs et capacité
  maximale exacte. L'ABI transporte les trames par écho ; ses handlers
  métier et le Worker restent à développer.

Les [25 contrôles du lot US-011/053/091](../_bmad-output/implementation-artifacts/text-bridge-validation.json)
passent, avec **260 tests workspace et 70 tests release**,
builds Windows/WASM/musl, Clippy hôte/WASM, format, SPDX et ressources.
La taille WASM reste de 52 323 octets. Les sources et le lock restent identiques
pendant les contrôles, et les 32 paquets gardent des dépendances locales.
La [revue complète](../_bmad-output/implementation-artifacts/text-bridge-review.md)
ne laisse aucun constat différé. Les tests musl sont compilés localement ;
leur exécution appartient à la CI Linux du commit publié.

US-012, US-054 et US-055 sont développées, relues et validées :

- [Coupures de ligne](us-012-line-breaks.md) : 19 338 cas officiels UAX14,
  les mêmes cas enrichis de marques LB9 et les largeurs EAW de tous les scalaires.
- [Façonnage latin](us-054-latin-shaping.md) : glyphes/clusters et positions
  déterministes, interlettrage signé, chasse, 54 716 références HarfBuzz sur vingt faces.
- [Gestionnaire de polices](us-055-font-manager.md) : vingt faces embarquées,
  cinq alias métriques, proximité CSS, sources/TTC, couverture et limites atomiques.
  Quatre variantes Arial réellement installées sont sélectionnées par l'API System.

Les [26 contrôles finaux](../_bmad-output/implementation-artifacts/text-layout-validation.json)
passent : **291 tests workspace et 101 tests release**, builds Windows/WASM/musl,
Clippy hôte/WASM, format, SPDX, reproductibilité et intégrité des ressources.
La taille WASM reste de 52 323 octets. Sources et lock sont identiques pendant
les contrôles ; les 32 paquets n'utilisent que des dépendances locales.
Les [trois revues Luna moyen](../_bmad-output/implementation-artifacts/text-layout-review.md)
ne laissent aucun constat différé. Le coordinateur a finalisé les corrections
après la limite d'usage des deux agents de développement ; les agents frais
de revue ont pu travailler. Les tests musl sont compilés localement et exécutés
par la CI Linux du commit publié.

US-110 et US-116 sont développées, relues et validées ; le transport de US-092
est implémenté et vérifié en parallèle avec GPT-6 Luna moyen :

- [Worker](us-092-worker.md) : FIFO, IDs u64, buffers transférés et détachés,
  mémoire WASM réutilisable, annulation et progression réelles des copies.
- [Fenêtre Windows](us-110-win32-window.md) : boucle native, Per-Monitor V2,
  thème système et géométrie persistante, tests de vrais HWND cachés.
- [Stockage portable](us-116-portable-data.md) : une racine commune,
  choix local explicite en cas d'échec et arrêt avant tout cache inaccessible.

Les [27 contrôles](../_bmad-output/implementation-artifacts/worker-shell-validation.json)
passent, avec **312 tests workspace et 21 tests shell release**.
Les sources et le lock sont restés identiques pendant la validation.
La [revue indépendante](../_bmad-output/implementation-artifacts/worker-shell-review.md)
ne laisse aucun constat différé dans le transport livré et les deux US Windows. Le multi-écran synthétique ne remplace pas un
essai sur plusieurs écrans physiques ; les tests natifs n'affichent pas de fenêtre.

Le lot front-end a été développé en parallèle avec GPT-6 Luna moyen :
[Canvas](us-101-canvas-renderer.md), [saisie](us-103-text-input.md) et
[WebView2/runtime absent](us-111-webview2.md). Le navigateur vérifie aussi
l’apparition et l’effacement du soulignement de composition dans un vrai Canvas.
La [validation locale](../_bmad-output/implementation-artifacts/frontend-host-validation.json)
distingue les contrôles fonctionnels de l’acceptation stricte des handles.
Le [journal d’exploration](../_bmad-output/implementation-artifacts/frontend-host-exploration.json)
conserve les échecs ; le [signalement Microsoft similaire](https://github.com/MicrosoftEdge/WebView2Feedback/issues/5704)
reste une piste, sans preuve de cause interne. Le benchmark headless ne valide
pas les 60 images/s physiques et les événements synthétiques ne valident pas
le clavier AZERTY ou l’IME du système.

Validation finale du lot : [34 contrôles fonctionnels et qualité](../_bmad-output/implementation-artifacts/frontend-host-validation.json),
**318 tests workspace et 27 tests shell release**, sur sources et lock inchangés.
Les tests Chromium couvrent Canvas, saisie et leur composition intégrée.
Les deux profils WebView2 accomplissent 100 cycles fonctionnels avec callbacks équilibrés ;
le rapport conserve séparément l'échec strict des handles et US-111 reste ouverte.
La [revue indépendante](../_bmad-output/implementation-artifacts/frontend-host-review.md)
conserve un point IME natif de gravité moyenne non vérifiée à trancher sur matériel.
Les tests musl sont compilés localement ; leur exécution appartient à la CI Linux.

Le lot ressources, fichiers et texte brut a été développé en parallèle avec trois
agents GPT-6 Luna moyen, puis corrigé après trois revues indépendantes.
La [validation finale](../_bmad-output/implementation-artifacts/embedded-foundations-validation.json)
réussit **37 contrôles, 351 tests workspace et 48 tests natifs release**,
sur sources et lock inchangés. Les diagnostics debug/release chargent les onze
ressources, vérifient le vrai Worker/WASM par écho Ping, HEAD/404/405 et six
catégories de refus CSP avec traces natives. La [copie isolée de l’exécutable](../_bmad-output/implementation-artifacts/embedded-foundations-portable.json)
fonctionne sans dossier web/data. Les [18 constats de revue](../_bmad-output/implementation-artifacts/embedded-foundations-review.md)
sont triés ; les corrections retenues sont appliquées, aucun nouveau constat
n’est différé. Les [échecs exploratoires](../_bmad-output/implementation-artifacts/embedded-foundations-exploration.json)
restent conservés.

US-112 reste ouverte pour la journalisation des violations CSP internes aux
Workers et les scénarios service-worker. US-113 reste ouverte pour le pont
fichiers JS, SharedBuffer, les polices/URL et les essais de dialogues natifs.
US-120 reste ouverte pour la mise en page et la connexion saisie/modèle/rendu,
ainsi que ses mesures de latence et démarrage. US-111 conserve l’échec strict
de handles ; ses contrôles fonctionnels de 100 cycles et callbacks passent.
Le backlog reste à **35 US entièrement validées sur 158**. Les tests musl sont
compilés localement ; leur exécution appartient à la CI Linux du commit publié.
