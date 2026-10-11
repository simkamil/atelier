# US-112 — Ressources embarquées et origine locale

La page d’accueil du shell est chargée depuis `https://app.atelier/`. Le binaire reçoit les octets depuis la table explicite `crates/desktop-win/src/resources.rs`; il n’y a aucun serveur de fichiers ni recours au dossier de travail. La table intègre l’HTML, le module d’accueil, le CSS, le catalogue `fr-FR`, les quatre modules Worker/client/chargeur/protocole requis, le module WASM, Carlito Regular et sa licence OFL. Les polices Caladea et Liberation restent disponibles via `atelier-font` pour le rendu natif ; elles ne sont pas exposées par l’origine Web dans ce lot.

## Contrat HTTP local

L’origine admise est exactement `https://app.atelier/`. Les routes explicites acceptent GET et HEAD, et toutes les autres méthodes renvoient 405. Une URL d’origine locale non répertoriée renvoie 404. La classification porte sur l’URL reçue de WebView2, après sa normalisation par le navigateur ; elle ne reconstitue pas la chaîne saisie avant cette normalisation. Une origine externe, un hôte ressemblant, un port, des identifiants, une traversée de chemin, un séparateur ambigu ou un encodage percent sont refusés sans servir d’octets. Query et fragment sont ignorés pour la résolution et exclus du journal. Le journal en mémoire conserve au plus 64 entrées bornées et ne contient ni query ni fragment.

Chaque réponse ajoute une CSP restrictive et `X-Content-Type-Options: nosniff`. Le document ne contient aucun script ou style inline. Les modules locaux, le Worker de module et `WebAssembly.instantiate` sont autorisés. `connect-src` nomme explicitement `https://app.atelier`, sans wildcard `wss:`. Le contrôle charge le binaire intégré dans un vrai Worker et vérifie que le moteur renvoie l’écho exact de la commande Ping; ce protocole ne produit pas de Pong métier.

## Reproduction et couverture

Les tests ciblés de `resources.rs` vérifient les routes, méthodes, origines, cas ambigus et bornes du journal; les tests fake-vtable vérifient le refus par défaut lorsque les getters de navigation, popup ou requête échouent. Sur Windows avec un runtime Evergreen exposant `ICoreWebView2_22`, le self-check caché vérifie chaque ressource et son MIME, HEAD avec corps vide et Content-Length égal au GET, 404 pour une route inconnue, 405 pour POST, le chargement du Worker et l’écho Ping, le refus d’un chemin encodé, ainsi que les refus par `connect-src` d’un fetch et d’un WebSocket, par `img-src` d’une image, `frame-src` d’une frame, `script-src-elem` d’un script et `style-src-elem` d’une feuille de style. Il exige à la fois les six événements CSP côté page et leurs entrées correspondantes dans le journal natif, ainsi que les refus `window.open` et navigation top-level externes. Le relais utilise `ICoreWebView2::add_WebMessageReceived`/`remove_WebMessageReceived` (slots 34/35, IID `57213f19-00e6-49fa-8e07-898ea01ecbd2`) et ne fait que journaliser : il accepte des chaînes d’au plus 768 caractères depuis la source exacte `https://app.atelier/`, une directive CSP reconnue (dont `script-src-elem`, `script-src-attr`, `style-src-elem` et `style-src-attr`) et une URI de blocage bornée; les messages CSP restent distincts des commandes du [pont fichiers](us-113-file-bridge.md). Le journal conserve au plus 64 entrées, retire credentials/query/fragment et borne chaque entrée. Les messages de la page sont non fiables; la branche CSP ne les interprète jamais comme commandes fichier.

Le contrôle ne revendique pas une preuve générale de tous les refus CSP possibles : il exécute ces six catégories et journalise les directives définies par la policy, y compris les variantes `*-elem` et `*-attr`. Selon le runtime, `blockedURI` peut être réduit à l’origine pour confidentialité; le test de frame a ainsi produit `https://us112-frame.invalid` sans chemin. La preuve associe alors la directive à l’origine unique du scénario. Les imports externes depuis un Worker, les autres schémas de transport, les extensions CSP et les chemins de service-worker ne font pas partie des scénarios exécutés ici. Les violations CSP produites à l’intérieur du Worker, l’installation et le cycle de vie de service-worker restent hors critères. Aucun isolement réseau au niveau transport n’est revendiqué. L’artefact requis est `data/engine/atelier_core_wasm.wasm`; `data/engine/manifest.json`, `tools/package-engine.py` et `node tests/check-embedded-engine.mjs --compare-artifact` documentent et vérifient sa reconstruction reproductible.

Le filtre `*` utilise `AddWebResourceRequestedFilterWithRequestSourceKinds` avec contexte All et sources All, après QueryInterface de `ICoreWebView2_22`. Cela couvre les requêtes de documents, shared workers et service workers signalées par cette API. Si le runtime n’expose pas cette interface, l’initialisation échoue explicitement plutôt que d’annoncer une couverture worker incomplète. Toutes les navigations top-level hors origine sont annulées.

## Limites fonctionnelles

La page fournit désormais l’éditeur TXT intégré, décrit dans [son contrat UI](us-120-editor-ui.md). Le contrôle natif vérifie le socle local, le moteur réel, l’origine et la politique de ressources. Les événements `securitypolicyviolation` suivent le relais CSP distinct du pont fichiers ; les messages inconnus sont ignorés. Les libellés affichés viennent de `fr-FR.txt`; le HTML et le fallback JavaScript gardent un texte français minimal si le catalogue embarqué échoue. Le self-check WebView2 ne peut être exécuté que sur Windows lorsque WebView2 Evergreen est présent. Le compte de callbacks suit les objets COM conservés par ce crate ; la livraison interne d’événements après `Close` dépend du runtime WebView2.

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
