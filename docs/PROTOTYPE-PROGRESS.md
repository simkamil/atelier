# Préparation du démonstrateur — 2026-10-11

US-010/011/012, US-051/053/054/055 et US-090/091 ont été développées en parallèle avec GPT-6 Luna,
raisonnement moyen, puis relues et validées. Le backlog compte désormais
**35 US terminées sur 158**.

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
