# Spécification — Suite bureautique Rust (nom de code provisoire : *Atelier*)

| | |
|---|---|
| Version | 0.2 — brouillon corrigé pour validation |
| Date | 2026-10-09 |
| Statut | Référence du backlog V0/V1 ; incohérences identifiées corrigées, validation produit à confirmer |
| Licence du produit | AGPL-3.0 |

---

## 1. Vision

Une suite bureautique complète (traitement de texte, tableur, présentations, PDF) équivalente à MS Office / OnlyOffice, écrite en Rust **sans aucune bibliothèque tierce**, destinée aux particuliers.

- **Desktop** : application locale, **totalement déconnectée** (aucun appel réseau, aucune télémétrie).
- **Web** : application complète auto-hébergée (gestion de fichiers + éditeurs), installable sur un hébergement mutualisé (cible de test : o2switch).
- Les deux produits partagent le même moteur mais sont **indépendants** (pas de synchronisation entre eux).

### 1.1 Non-objectifs (explicites)
- Pas de synchronisation desktop ↔ web.
- Pas de co-édition temps réel tant que l'hébergement cible est mutualisé (cf. §7.4).
- Pas de compatibilité VBA (les macros sont en Lua).
- Pas de service de conversion headless ni d'API de génération de documents avant une version ultérieure.

---

## 2. Décisions structurantes validées

| # | Décision |
|---|---|
| D1 | Modules cible : traitement de texte, tableur, présentations, éditeur PDF/formulaires. MVP = traitement de texte. |
| D2 | Format natif OOXML (docx/xlsx/pptx). Lecture des formats binaires anciens (doc/xls/ppt) et ODF (odt/ods/odp). |
| D3 | Macros et plugins en **Lua** (interpréteur écrit maison). |
| D4 | IA plus tard : modèle local **embarqué dans le paquet** (pas d'installation séparée) + possibilité de configurer un modèle externe. |
| D5 | Windows en MVP ; macOS, Linux ensuite. |
| D6 | Distribution : exécutable **portable** d'abord ; installeur et mise à jour automatique plus tard. |
| D7 | Web : auto-hébergé, stockage sur système de fichiers local, comptes locaux multi-utilisateurs. |
| D8 | Moteur unique Rust compilé en **WASM**, exécuté dans une WebView (desktop) ou un navigateur (web). |
| D9 | Shell desktop : **couche maison Win32 + WebView2** (pas de Tauri). |
| D10 | Serveur web : binaire Rust statique exécuté en **CGI** par Apache, sans état. |
| D11 | **Zéro dépendance** : ni crate Rust, ni bibliothèque/framework JS, ni outil de dev tiers hors chaîne Rust. Autorisés : chaîne Rust + `std`/`core`, API système (Win32, COM, WebView2, appels système Linux), API navigateur (DOM, Canvas, Clipboard…), fichiers de données (polices, tables Unicode, poids de modèle). |
| D12 | Interface en français en MVP, toutes les chaînes externalisées dès le départ. |
| D13 | Accessibilité requise (cible RGAA 4.1 / WCAG 2.1 AA). |
| D14 | Développeur unique, pas d'échéance imposée : la qualité et la solidité priment sur la vitesse. |

### 2.1 Conséquences de D11 (à garder en tête)
Tout est à écrire : décompression zip/deflate, parseur XML, lecture des polices OpenType, mise en forme du texte, décodeurs PNG/JPEG/GIF/BMP, rasterisation, générateur PDF, conteneur CFB, interpréteur Lua, cryptographie (SHA-2, HMAC, Argon2id, AES pour les documents protégés), liaison Rust↔JS, liaisons Win32/COM, et plus tard le moteur d'inférence IA.
Le front-end est en **JavaScript vanilla (modules ES) annoté en JSDoc**, sans étape de build (pas de TypeScript : le compilateur `tsc` serait un outil tiers).

---

## 3. Jalons

| Version | Contenu | Plateforme |
|---|---|---|
| **V0 — Fondations** (interne) | zip, XML, polices, mise en forme du texte, liste d'affichage, rendu canvas, shell Win32/WebView2, pont WASM | Windows |
| **V1 — MVP** | Traitement de texte : docx lecture/écriture, odt et doc en lecture, mise en forme, styles, listes, tableaux, images, en-têtes/pieds, annuler/rétablir, impression, export PDF | Windows portable |
| **V2 — Web** | Serveur CGI, comptes, gestionnaire de fichiers, traitement de texte V1 dans le navigateur | Navigateurs récents + o2switch |
| **V3 — Traitement de texte complet** | Commentaires, suivi des modifications, comparaison, publipostage, signature numérique, protection, historique des versions, notes, table des matières, formes, colonnes, EMF/WMF | Desktop + web |
| **V4 — Lua** | Interpréteur Lua, macros, plugins, gestionnaire de plugins | Desktop + web |
| **V5a — Tableur** | ~120 fonctions courantes (Annexe A), formats, graphiques de base, tri/filtre ; xlsx R/W, xls/ods lecture | Desktop + web |
| **V5b — Tableur complet** | Toutes les fonctions Excel, TCD, mise en forme conditionnelle avancée, solveur, valeur cible | Desktop + web |
| **V6 — Présentations** | pptx R/W, ppt/odp lecture, mode présentation | Desktop + web |
| **V7 — PDF** | Lecture, annotation, formulaires, édition, réorganisation des pages | Desktop + web |
| **Ultérieur** | Édition Markdown, installeur + MAJ auto, signature de code, macOS/Linux, correcteur orthographique, césure, autres langues, RTL/CJK, IA, chiffrement, co-édition (hébergement dédié), conversion headless/API, mobile | — |

---

## 4. Spécifications fonctionnelles

Convention : chaque exigence a un identifiant (`F-xxx-nnn`) et une version cible. Ces identifiants serviront de base aux user stories.

### 4.1 Commun à tous les modules (F-COM)

| ID | Exigence | Version |
|---|---|---|
| F-COM-001 | Créer un document vierge ou depuis un modèle | V1 |
| F-COM-002 | Ouvrir un fichier (boîte de dialogue native, glisser-déposer sur la fenêtre, argument en ligne de commande / association de fichier) | V1 |
| F-COM-003 | Enregistrer, Enregistrer sous (choix du format) | V1 |
| F-COM-004 | Liste des documents récents | V1 |
| F-COM-005 | Enregistrement atomique : jamais de fichier corrompu en cas de crash pendant l'écriture (§6.13) | V1 |
| F-COM-006 | Sauvegarde de récupération automatique autonome (instantané de base + journal, intervalle paramétrable, défaut 2 min) et proposition de restauration au redémarrage après crash ; récupération jusqu'au dernier point durable (§6.13) | V1 |
| F-COM-007 | Fichier verrou à la Word (`~$nom.docx`) et avertissement si le document est déjà ouvert ailleurs | V1 |
| F-COM-008 | Détection d'une modification externe du fichier ouvert | V1 |
| F-COM-009 | Annuler / rétablir illimités (bornés par la mémoire), y compris après enregistrement | V1 |
| F-COM-010 | Rechercher / remplacer (texte, casse, mot entier) | V1 |
| F-COM-011 | Rechercher / remplacer avec expressions régulières (moteur maison) | V3 |
| F-COM-012 | Zoom 10 %–500 %, ajustement page/largeur | V1 |
| F-COM-013 | Impression (choix imprimante, pages, copies, recto-verso selon pilote) | V1 |
| F-COM-014 | Export PDF (texte sélectionnable, polices incorporées en sous-ensemble, liens, signets depuis les titres) | V1 |
| F-COM-015 | Export PDF/A et PDF balisé (PDF/UA) | V3 |
| F-COM-016 | Presse-papiers : texte brut, HTML, format interne riche (copie entre documents sans perte) | V1 |
| F-COM-017 | Propriétés du document (titre, auteur, sujet, mots-clés, dates) | V1 |
| F-COM-018 | Interface à ruban (onglets par module), barre d'état, panneaux latéraux repliables | V1 |
| F-COM-019 | Thème clair / sombre / suivre le système ; respect du mode contraste élevé Windows | V1 |
| F-COM-020 | Raccourcis clavier calqués sur ceux de MS Office en français ; affectations de référence figées dans une table unique sans conflit (US-231), dont Ctrl+Maj+V pour coller le texte seul et Ctrl+Alt+C / Ctrl+Alt+V pour copier/appliquer la mise en forme | V1 |
| F-COM-021 | Navigation complète au clavier, compatibilité lecteurs d'écran (NVDA, Narrateur) | V1 |
| F-COM-022 | Préférences utilisateur stockées à côté de l'exe en mode portable ; repli en mode local uniquement sur choix explicite si le dossier n'est pas inscriptible (§7.2) | V1 |
| F-COM-023 | Onglets ou fenêtres multiples (plusieurs documents ouverts) | V1 |
| F-COM-024 | Correcteur orthographique (dictionnaires embarqués, moteur maison) | Ultérieur |
| F-COM-025 | Aide intégrée hors ligne | V3 |

### 4.2 Traitement de texte (F-TXT)

#### Édition
| ID | Exigence | Version |
|---|---|---|
| F-TXT-001 | Saisie de texte avec gestion complète du clavier français (touches mortes, AltGr, IME) | V1 |
| F-TXT-002 | Sélection souris (clic, double-clic mot, triple-clic paragraphe, glisser, Maj+clic) et clavier (Maj+flèches, Ctrl+Maj+flèches…) | V1 |
| F-TXT-003 | Glisser-déposer de texte dans le document | V1 |
| F-TXT-004 | Insertion de caractères spéciaux, espaces insécables (insertion automatique avant `: ; ! ?` en français, paramétrable) | V1 |
| F-TXT-005 | Correction automatique de base (guillemets français « », majuscule en début de phrase) | V1 |
| F-TXT-006 | Statistiques (mots, caractères, paragraphes, pages) | V1 |

#### Mise en forme
| ID | Exigence | Version |
|---|---|---|
| F-TXT-010 | Caractère : police, taille, gras, italique, souligné (simple, double, pointillé…), barré, exposant, indice, couleur, surlignage, petites majuscules, casse, espacement des caractères | V1 |
| F-TXT-011 | Paragraphe : alignement (gauche, centré, droite, justifié), retraits, première ligne, interligne, espacement avant/après, tabulations (gauche, centre, droite, décimale, points de suite) | V1 |
| F-TXT-012 | Paragraphe : bordures, trame de fond, veuves et orphelines, paragraphes solidaires, lignes solidaires, saut de page avant | V1 |
| F-TXT-013 | Reproduire la mise en forme | V1 |
| F-TXT-014 | Effacer la mise en forme | V1 |

#### Styles et listes
| ID | Exigence | Version |
|---|---|---|
| F-TXT-020 | Styles de paragraphe, de caractère, de tableau, de liste ; héritage ; galerie de styles | V1 |
| F-TXT-021 | Jeu de styles par défaut équivalent à Word (Normal, Titre 1–9, Titre, Citation…) | V1 |
| F-TXT-022 | Créer, modifier, renommer et supprimer un style | V1 |
| F-TXT-023 | Listes à puces, numérotées, multiniveaux ; redémarrer/continuer la numérotation | V1 |
| F-TXT-024 | Volet de navigation (plan par titres) | V1 |

#### Objets
| ID | Exigence | Version |
|---|---|---|
| F-TXT-030 | Tableaux : insertion, ajout/suppression de lignes et colonnes, fusion/fractionnement, largeurs, bordures, trames, ligne d'en-tête répétée, autoriser ou non le fractionnement des lignes ; lecture, rendu et préservation des tableaux imbriqués importés (contenu imbriqué en lecture seule) | V1 |
| F-TXT-031 | Tableaux : tri, formules simples, création et édition de tableaux imbriqués | V3 |
| F-TXT-032 | Images PNG, JPEG, GIF, BMP : insertion, redimensionnement, habillage (aligné sur le texte, carré, haut et bas, derrière, devant) | V1 |
| F-TXT-033 | Images : rognage, rotation, bordures ; SVG ; EMF/WMF (fréquents dans les docx issus de Word) | V3 |
| F-TXT-034 | Liens hypertexte et signets | V1 |
| F-TXT-035 | Champs : PAGE, NUMPAGES, DATE, TIME | V1 |
| F-TXT-036 | Champs : TOC (table des matières), REF, SEQ (légendes), index | V3 |
| F-TXT-037 | Notes de bas de page et de fin | V3 |
| F-TXT-038 | Formes, zones de texte, WordArt | V3 |
| F-TXT-039 | Équations (OMML) | Ultérieur |
| F-TXT-040 | Objets non gérés (SmartArt, graphiques, OLE) : affichage de l'image de repli contenue dans le fichier et préservation à l'enregistrement | V1 |

#### Mise en page
| ID | Exigence | Version |
|---|---|---|
| F-TXT-050 | Format de papier, orientation, marges | V1 |
| F-TXT-051 | Sauts de page et de section, sections avec mise en page différente | V1 |
| F-TXT-052 | En-têtes et pieds de page (première page différente, pages paires/impaires) | V1 |
| F-TXT-053 | Numérotation des pages (format, point de départ) | V1 |
| F-TXT-054 | Colonnes | V3 |
| F-TXT-055 | Filigrane, couleur de page, bordures de page | V3 |
| F-TXT-056 | Modes d'affichage : page, plan ; règles ; affichage des marques de mise en forme | V1 |
| F-TXT-057 | Césure automatique | Ultérieur |

#### Travail collaboratif hors ligne (V3)
| ID | Exigence | Version |
|---|---|---|
| F-TXT-060 | Commentaires : ajout, réponse, résolution, suppression ; volet des commentaires | V3 |
| F-TXT-061 | Suivi des modifications : activer, afficher, accepter/refuser une modification ou toutes ; auteur et date | V3 |
| F-TXT-062 | Comparaison de deux documents (résultat sous forme de modifications suivies) | V3 |
| F-TXT-063 | Publipostage depuis un fichier CSV ou xlsx (vers un document fusionné ou des PDF) | V3 |
| F-TXT-064 | Signature numérique du document (XML-DSig OOXML) et vérification | V3 |
| F-TXT-065 | Protection : ouverture par mot de passe (chiffrement ECMA-376 Agile), restriction de modification | V3 |
| F-TXT-066 | Historique des versions locales (instantanés à chaque enregistrement, consultation, restauration) | V3 |

#### Markdown (Ultérieur)
| ID | Exigence | Version |
|---|---|---|
| F-TXT-070 | Ouvrir et enregistrer des fichiers `.md` (CommonMark + extensions GFM : tableaux, listes de tâches, barré, liens automatiques) | Ultérieur |
| F-TXT-071 | Édition WYSIWYG limitée aux constructions représentables en Markdown (titres, gras, italique, code, listes, citations, tableaux, liens, images) ; avertissement si une mise en forme non représentable est appliquée | Ultérieur |
| F-TXT-072 | Vue source avec coloration syntaxique et bascule source ↔ rendu (ou vue côte à côte) | Ultérieur |
| F-TXT-073 | Aller-retour fidèle : un fichier ouvert puis enregistré sans modification reste identique octet pour octet ; seules les parties modifiées sont réécrites | Ultérieur |
| F-TXT-074 | Conversion Markdown ↔ docx et export PDF/HTML | Ultérieur |

### 4.3 Tableur (F-CLC)

| ID | Exigence | Version |
|---|---|---|
| F-CLC-001 | Classeur multi-feuilles : ajouter, renommer, déplacer, masquer, colorer les onglets | V5a |
| F-CLC-002 | Grille de 1 048 576 lignes × 16 384 colonnes (stockage creux) | V5a |
| F-CLC-003 | Types de valeur : nombre, texte, booléen, date/heure, erreur | V5a |
| F-CLC-004 | Formules : références relatives, absolues et mixtes, entre feuilles, plages nommées ; affichage des noms de fonctions **en français** (SOMME, SI, RECHERCHEV), stockage en anglais dans le fichier comme Excel ; séparateur `;` | V5a |
| F-CLC-005 | ~120 fonctions courantes (Annexe A) | V5a |
| F-CLC-006 | Toutes les fonctions Excel (~480), formules matricielles dynamiques | V5b |
| F-CLC-007 | Recalcul incrémental automatique ; mode manuel ; détection des références circulaires ; calcul itératif | V5a |
| F-CLC-008 | Formats de nombre (codes Excel, locale fr-FR), monétaire, pourcentage, date, personnalisés | V5a |
| F-CLC-009 | Mise en forme des cellules : police, bordures, remplissage, alignement, retour à la ligne, fusion | V5a |
| F-CLC-010 | Recopie incrémentée (séries, dates, listes personnalisées), collage spécial | V5a |
| F-CLC-011 | Tri multi-critères, filtre automatique | V5a |
| F-CLC-012 | Figer les volets, fractionner la fenêtre, masquer lignes/colonnes, groupement/plan | V5a |
| F-CLC-013 | Graphiques : histogramme, barres, courbes, secteurs, aires, nuages de points | V5a |
| F-CLC-014 | Graphiques avancés (combinés, axes secondaires, boursier, radar, cascade…) | V5b |
| F-CLC-015 | Mise en forme conditionnelle (règles simples) | V5a |
| F-CLC-016 | Mise en forme conditionnelle avancée (barres de données, nuances, jeux d'icônes) | V5b |
| F-CLC-017 | Validation des données (listes déroulantes, bornes) | V5a |
| F-CLC-018 | Tableaux croisés dynamiques | V5b |
| F-CLC-019 | Valeur cible, solveur, gestionnaire de scénarios | V5b |
| F-CLC-020 | Import/export CSV (choix du séparateur et de l'encodage) | V5a |
| F-CLC-021 | Commentaires et notes de cellules | V5a |
| F-CLC-022 | Protection de feuille et de classeur | V5b |
| F-CLC-023 | Mise en page d'impression : zone d'impression, titres répétés, ajustement à N pages, en-têtes/pieds | V5a |
| F-CLC-024 | Tableaux structurés (objets tableau Excel) | V5b |

### 4.4 Présentations (F-PRS) — V6

| ID | Exigence |
|---|---|
| F-PRS-001 | Diapositives : ajouter, dupliquer, supprimer, réordonner (trieuse), masquer |
| F-PRS-002 | Dispositions, masques des diapositives, thèmes (couleurs, polices) |
| F-PRS-003 | Zones de texte, formes, images, tableaux, graphiques (moteur partagé avec le tableur) |
| F-PRS-004 | Alignement, distribution, groupement, ordre de superposition, repères magnétiques |
| F-PRS-005 | Transitions entre diapositives |
| F-PRS-006 | Animations d'objets (entrée, emphase, sortie, trajectoires) |
| F-PRS-007 | Mode présentation plein écran ; mode présentateur sur double écran (notes, minuteur, diapositive suivante) |
| F-PRS-008 | Notes du présentateur |
| F-PRS-009 | Export PDF (diapositives, notes, documents d'accompagnement) |
| F-PRS-010 | Audio/vidéo : préservation, puis lecture (le décodage maison des formats vidéo est un chantier majeur, cf. §10) |

### 4.5 PDF (F-PDF) — V7

| ID | Exigence |
|---|---|
| F-PDF-001 | Visualisation (PDF jusqu'à 2.0), recherche, copie du texte, miniatures, signets |
| F-PDF-002 | Annotations : surlignage, soulignement, notes, dessin à main levée, formes, tampons |
| F-PDF-003 | Remplissage de formulaires AcroForm, enregistrement |
| F-PDF-004 | Création et modification de formulaires (champs texte, cases, listes, boutons radio) |
| F-PDF-005 | Édition du texte existant (dans la limite des polices incorporées) |
| F-PDF-006 | Réorganiser, pivoter, insérer, supprimer, extraire des pages ; fusionner et fractionner des PDF |
| F-PDF-007 | Signature numérique PDF et vérification |
| F-PDF-008 | Conversion PDF → docx (meilleur effort) — Ultérieur |

### 4.6 Macros et plugins Lua (F-LUA) — V4

| ID | Exigence |
|---|---|
| F-LUA-001 | Éditeur de macros intégré (coloration syntaxique, exécution, console de sortie, erreurs avec numéro de ligne) |
| F-LUA-002 | Macros stockées dans le document (partie personnalisée du paquet OOXML, ignorée par MS Office) ou dans la bibliothèque personnelle de l'utilisateur |
| F-LUA-003 | Macros désactivées par défaut à l'ouverture d'un document ; bandeau d'activation ; emplacements approuvés |
| F-LUA-004 | API objet par module (document, paragraphes, plages, styles, tableaux ; classeurs, feuilles, cellules ; diapositives…) |
| F-LUA-005 | Fonctions personnalisées utilisables dans les formules du tableur |
| F-LUA-006 | Plugins : paquet (manifeste + scripts + ressources) installable, désinstallable, activable ; permissions déclarées (lire le document, modifier le document, lire/écrire des fichiers, réseau) accordées par l'utilisateur |
| F-LUA-007 | Plugins : extension de l'interface (bouton dans le ruban, panneau latéral, entrée de menu contextuel) via une API déclarative |
| F-LUA-008 | Bac à sable : pas d'accès direct aux fichiers, au système ou au réseau hors API autorisée ; limites d'instructions et de mémoire ; interruption par l'utilisateur |
| F-LUA-009 | Enregistreur de macros (actions de l'utilisateur → script Lua) — Ultérieur |

### 4.7 IA (F-IA) — Ultérieur

| ID | Exigence |
|---|---|
| F-IA-001 | Modèle de langage local **inclus dans le paquet**, exécuté par un moteur d'inférence maison (CPU, format GGUF, quantification 4/8 bits) |
| F-IA-002 | Configuration optionnelle d'un fournisseur externe (URL, clé API, modèle) — seule exception à la règle « aucun appel réseau », activée explicitement par l'utilisateur |
| F-IA-003 | Fonctions : reformuler, résumer, corriger, traduire, continuer le texte ; aide aux formules dans le tableur ; génération d'un plan de présentation |
| F-IA-004 | Les suggestions sont toujours présentées pour validation, jamais appliquées sans action de l'utilisateur |

Conséquence acceptée : la version portable avec modèle pèsera 1 à 4 Go de plus. Une variante « sans modèle » pourra être distribuée.

### 4.8 Version web (F-WEB) — V2

| ID | Exigence |
|---|---|
| F-WEB-001 | Connexion par identifiant et mot de passe ; déconnexion ; changement de mot de passe |
| F-WEB-002 | Administration : création, désactivation et suppression de comptes, quota par utilisateur (page d'administration + commande CLI pour créer le premier administrateur) |
| F-WEB-003 | Gestionnaire de fichiers : dossiers, liste/grille, tri, recherche par nom, fil d'Ariane |
| F-WEB-004 | Téléverser (glisser-déposer, plusieurs fichiers) et télécharger |
| F-WEB-005 | Créer, renommer, déplacer, copier un document ou un dossier |
| F-WEB-006 | Suppression vers une **corbeille** (restauration possible ; vidage par l'utilisateur ou purge automatique après N jours, paramétrable) |
| F-WEB-007 | Ouvrir un document dans l'éditeur du navigateur (mêmes fonctions que le desktop pour le module concerné) |
| F-WEB-008 | Enregistrement manuel + automatique (défaut 30 s) avec contrôle de concurrence : si le fichier a changé côté serveur depuis l'ouverture, aucun écrasement silencieux (proposition : enregistrer une copie ou recharger) |
| F-WEB-009 | Verrou d'édition : un seul éditeur à la fois par document (lecture seule pour les autres, avec le nom du détenteur du verrou) ; expiration du verrou en cas d'abandon |
| F-WEB-010 | Historique des versions côté serveur (N dernières versions conservées) |
| F-WEB-011 | Lien « Code source » visible dans l'interface (obligation AGPL §13) |
| F-WEB-012 | Interface responsive pour tablette (éditeur), consultation sur téléphone |
| F-WEB-013 | Co-édition temps réel (curseurs, présence) — Ultérieur, hébergement non mutualisé requis |
| F-WEB-014 | Partage par lien (lecture seule / édition) entre utilisateurs de l'instance — Ultérieur |

---

## 5. Formats de fichiers

### 5.1 Matrice

| Format | Lecture | Écriture | Version |
|---|---|---|---|
| docx / dotx / docm | ✔ | ✔ | V1 |
| odt | ✔ | — | V1 |
| doc (Word 97–2003) | ✔ | — | V1 |
| txt (UTF-8, UTF-16, Windows-1252) | ✔ | ✔ | V1 |
| pdf | — | ✔ (export) | V1 |
| rtf | ✔ | ✔ | V3 (à confirmer, cf. §11) |
| html | — | ✔ (export) | V3 |
| md (Markdown CommonMark + extensions GFM) | ✔ | ✔ | Ultérieur |
| xlsx / xltx / xlsm | ✔ | ✔ | V5a |
| xls (BIFF8), ods | ✔ | — | V5a |
| csv | ✔ | ✔ | V5a |
| pptx / potx / pptm | ✔ | ✔ | V6 |
| ppt, odp | ✔ | — | V6 |
| pdf | ✔ | ✔ (édition) | V7 |

### 5.2 Principes de fidélité
1. **Préservation de l'inconnu** : tout élément ou attribut OOXML non géré est conservé tel quel (sous-arbre XML opaque rattaché au nœud du modèle) et réémis à l'enregistrement. Les parties du paquet non gérées sont recopiées à l'identique. Objectif : *ouvrir puis enregistrer ne détruit jamais d'information*.
2. **Image de repli** : pour tout objet non rendu (SmartArt, graphique, OLE, équation en V1), afficher l'image de repli présente dans le fichier.
3. **Compatibilité des métriques** : polices libres de métriques identiques embarquées (Carlito ↔ Calibri, Caladea ↔ Cambria, Liberation Sans/Serif/Mono ↔ Arial/Times New Roman/Courier New). Sur le desktop, la police système réelle est utilisée si elle est présente ; la mise en page reste identique dans les deux cas.
4. **Mise en page à la Word** : coupure de ligne gloutonne (comme Word, pas Knuth-Plass), règles de pagination et de largeur de tableaux alignées sur le comportement observé de Word, documenté dans un corpus de tests de référence.
5. **Formats anciens et ODF** : import vers le modèle interne ; à l'enregistrement, l'utilisateur est invité à choisir docx (aucune écriture dans ces formats).
6. Spécifications de référence : ECMA-376 5ᵉ éd. (OOXML), ISO/IEC 26300 / OASIS ODF 1.3, Microsoft Open Specifications ([MS-DOC], [MS-XLS], [MS-PPT], [MS-CFB], [MS-OFFCRYPTO], [MS-OI29500]), ISO 32000-2 (PDF).

---

## 6. Architecture technique

### 6.1 Vue d'ensemble

```
┌──────────────────────── Desktop (Windows) ────────────────────────┐
│ atelier.exe  (Rust natif, Win32 + COM)                            │
│  ├─ Fenêtres, menus système, boîtes de dialogue fichiers, DPI     │
│  ├─ Accès fichiers, verrous, sauvegardes de récupération          │
│  ├─ Polices système (C:\Windows\Fonts)                            │
│  └─ WebView2 ───────────────────────────────────────────────┐     │
│      ressources servies depuis l'exe (https://app.atelier/)  │     │
│      ┌────────────── Front-end (identique au web) ───────────┴┐   │
│      │ JS vanilla : ruban, panneaux, dialogues, saisie (IME)  │   │
│      │ Canvas 2D : zone du document                           │   │
│      │ ARIA : miroir accessible du document                   │   │
│      │      ▲ liste d'affichage   │ événements / commandes    │   │
│      │      │                     ▼                           │   │
│      │ atelier_core.wasm (modèle, mise en page, formules,     │   │
│      │                    formats, rendu, Lua, PDF)           │   │
│      └────────────────────────────────────────────────────────┘   │
└───────────────────────────────────────────────────────────────────┘

┌──────────────────────── Web (o2switch) ───────────────────────────┐
│ Apache (HTTPS fourni par l'hébergeur)                             │
│  ├─ /app/*  fichiers statiques : HTML, JS, CSS, WASM, polices     │
│  └─ /api/*  → atelier-server.cgi (Rust statique, musl, sans état) │
│               auth, fichiers, verrous, versions, corbeille        │
│               données : ~/atelier-data/ (hors public_html)        │
└───────────────────────────────────────────────────────────────────┘
```

Principe : **le client fait tout le travail**. Le moteur tourne en WASM dans les deux cas ; seul l'« hôte » change (bridge natif pour le desktop, API HTTP pour le web) derrière une interface commune côté JS (`Host` : `open`, `save`, `list`, `fonts`, `print`…).

### 6.2 Organisation du dépôt (workspace Cargo, aucune dépendance externe)

```
atelier/
├─ crates/
│  ├─ base/          utilitaires : erreurs, arènes, interning, collections, hachage
│  ├─ unicode/       tables générées depuis l'UCD (catégories, coupure de ligne UAX#14,
│  │                 graphèmes UAX#29, casse, normalisation NFC/NFD, bidi UAX#9 plus tard)
│  ├─ compress/      inflate/deflate (RFC 1951), zlib, CRC32, Adler32
│  ├─ zip/           lecture/écriture ZIP (+ Zip64), protection contre les bombes zip
│  ├─ xml/           parseur XML en flux avec espaces de noms, écrivain XML
│  ├─ cfb/           conteneur Compound File Binary (formats anciens, documents chiffrés)
│  ├─ crypto/        SHA-1/256/512, HMAC, PBKDF2, Argon2id, AES-CBC/ECB, RSA (vérif. signature),
│  │                 ASN.1/X.509 minimal — testés contre les vecteurs officiels (NIST, RFC)
│  ├─ image/         décodeurs PNG, JPEG (baseline + progressif), GIF, BMP ; encodeur PNG ;
│  │                 EMF/WMF/SVG plus tard
│  ├─ font/          lecture OpenType (cmap, head, hhea, hmtx, maxp, name, OS/2, post, glyf,
│  │                 loca, CFF/CFF2, kern, GSUB, GPOS), sous-ensemble pour PDF
│  ├─ shape/         mise en forme du texte (latin en V1 : kerning, ligatures ; complexe plus tard)
│  ├─ geom/          géométrie 2D, chemins, courbes de Bézier, transformations
│  ├─ raster/        rasterisation logicielle anti-crénelée (miniatures, export PNG, tests)
│  ├─ model/         modèle commun : opérations, transactions, annulation, sélection, styles
│  ├─ text-model/    modèle traitement de texte
│  ├─ text-layout/   mise en page traitement de texte
│  ├─ sheet-model/   modèle tableur ; calc/ moteur de formules
│  ├─ slide-model/   modèle présentations
│  ├─ render/        liste d'affichage + backends : SVG, PDF (le Canvas est piloté côté JS)
│  ├─ fmt-ooxml/     docx/xlsx/pptx (lecture/écriture, préservation de l'inconnu)
│  ├─ fmt-odf/       odt/ods/odp (lecture)
│  ├─ fmt-legacy/    doc/xls/ppt (lecture)
│  ├─ fmt-pdf/       écriture PDF (V1), lecture/édition (V7)
│  ├─ fmt-markdown/  md CommonMark + GFM (lecture/écriture, plus tard)
│  ├─ lua/           compilateur + VM Lua 5.4, bibliothèques standard restreintes
│  ├─ core-wasm/     point d'entrée WASM, ABI exportée, protocole de messages
│  ├─ desktop-win/   exe Windows : liaisons Win32/COM/WebView2 écrites à la main
│  └─ server/        binaire CGI
├─ web/              front-end JS vanilla, CSS, HTML, catalogues de chaînes (fr-FR)
├─ data/             polices embarquées (OFL), fichiers UCD, dictionnaires (plus tard)
├─ tools/            générateurs (tables Unicode…) et fuzzer, écrits en Rust dans le workspace
└─ tests/            corpus de documents, images de référence, tests d'aller-retour
```

Règles : `#![forbid(unsafe_code)]` dans tous les crates sauf `core-wasm`, `desktop-win`, `server` (FFI), où chaque bloc `unsafe` est commenté et isolé dans un module dédié.

### 6.3 Modèle de document et opérations

- **Identifiants stables** pour chaque nœud (bloc, paragraphe, cellule, diapositive, objet) — indispensables pour les commentaires, le suivi des modifications et la future co-édition.
- **Traitement de texte** : `Document → Section* → Bloc* (Paragraphe | Tableau | Bloc opaque)` ; un paragraphe contient son texte (UTF-8) + des **plages d'attributs** (runs) + des objets ancrés. Les blocs sont rangés dans une structure arborescente équilibrée indexée par position, pour des insertions et recherches en O(log n) sur les gros documents.
- **Toute modification passe par une opération** (`InsertText`, `DeleteRange`, `SetAttrs`, `SplitBlock`, `MergeBlocks`, `InsertNode`, `RemoveNode`, `SetStyle`…). Chaque opération est **inversible**. Une commande utilisateur = une **transaction** (liste d'opérations atomique).
- Le **journal de transactions** alimente : annuler/rétablir, sauvegarde de récupération incrémentale, suivi des modifications, historique, enregistreur de macros, et plus tard la co-édition (positions exprimées par identifiants de nœud + décalage, transformables ; choix d'un algorithme CRDT de séquence le moment venu).
- **Styles** : cascade `docDefaults → chaîne de styles → mise en forme directe`, propriétés calculées mises en cache et invalidées par opération.
- **Sélection** : ancre + tête (identifiant de nœud + décalage), plusieurs plages possibles (sélection de colonnes de tableau, Ctrl+clic).

### 6.4 Texte et polices
- Polices lues depuis les fichiers (embarquées, système sur le desktop, incorporées dans les documents). Cache des glyphes par (police, glyphe) sous forme de chemins vectoriels.
- **V1 : mise en forme du texte latin** (cmap, kerning GPOS/kern, ligatures standard GSUB, chasse). Le texte complexe (arabe, indien, CJK vertical) et le bidi viendront ensuite ; l'API de `shape` est conçue dès le départ pour des séquences de glyphes indépendantes des caractères.
- Coupure de ligne selon UAX#14 pour la version UCD figée dans US-010, y compris ses exceptions autour des espaces insécables. Les règles typographiques françaises déterminent l'insertion d'espaces insécables et fines insécables ; elles ne remplacent pas l'algorithme Unicode de coupure. Le corpus officiel est vérifié sur cet algorithme sans adaptation locale.
- Polices de repli : table de correspondance (police demandée → police métriquement compatible → police générique).
- Tables Unicode **générées à la compilation** depuis les fichiers UCD officiels par un outil maison (`tools/`).

### 6.5 Mise en page (traitement de texte)
- Entrée : modèle + styles calculés ; sortie : arbre de **pages → lignes → fragments de glyphes**, plus la géométrie nécessaire au placement du curseur et aux tests de position.
- **Incrémentale** : seuls les paragraphes touchés sont remis en page ; la pagination est reprise à partir de la première page impactée jusqu'à ce que les sauts de page se stabilisent.
- Fonctions : coupure gloutonne, justification, tabulations, retraits, interligne (exact, au moins, multiple), veuves/orphelines, solidarité, en-têtes/pieds, tableaux (algorithmes de largeur fixe et automatique façon Word, fractionnement des lignes entre pages, en-têtes répétés), objets flottants (zones d'exclusion par ligne), champs (recalcul après pagination).
- La mise en page est **déterministe** et indépendante de la plateforme (mêmes polices → mêmes coupures au desktop et sur le web).

### 6.6 Rendu
- Le moteur produit une **liste d'affichage** par page (chemins, glyphes positionnés, images, clips, transformations, liens).
- **Écran** : le JS rejoue la liste sur un Canvas 2D (glyphes dessinés comme `Path2D` mis en cache), seules les pages visibles sont dessinées, rectangles sales pour l'édition, gestion du HiDPI (`devicePixelRatio`), découpage en tuiles pour les zooms élevés.
- **Impression** : backend **SVG** (vectoriel) → vue d'impression dédiée → API d'impression de WebView2 (desktop) ou `window.print()` (web). Un seul chemin d'impression pour les deux plateformes.
- **Export PDF** : backend PDF à partir de la même liste d'affichage.
- **Miniatures et tests** : backend raster logiciel → PNG.

### 6.7 Pont WASM ↔ JS (écrit à la main)
- Cible `wasm32-unknown-unknown`, exports `extern "C"`, pas de `wasm-bindgen`.
- Mémoire linéaire WASM privée au **Web Worker** : le JS du worker écrit les entrées (fichiers, commandes) dans des tampons alloués par le WASM (`alloc`/`free` exportés) et lit les sorties (listes d'affichage, état) directement dans cette mémoire. Le fil principal n'y accède pas directement.
- **Protocole de messages binaire** versionné (commandes → événements), sérialisation maison, une seule fonction d'entrée `dispatch(ptr, len) -> ptr`.
- Le moteur tourne dans un **Web Worker** pour ne jamais bloquer l'interface. Une copie explicite des octets de sortie vers un `ArrayBuffer` JS indépendant précède son transfert de propriété au fil principal, sans copie supplémentaire lors du transfert. Le tampon `WebAssembly.Memory.buffer` n'est jamais transféré ; le moteur conserve une mémoire utilisable. Les entrées transférées au worker sont ensuite copiées dans les allocations WASM. V0/V1 ne requièrent ni `SharedArrayBuffer` ni isolation inter-origines.
- **Saisie** : une zone de texte invisible positionnée au curseur capte le clavier, les touches mortes, l'IME et les événements `beforeinput`/`composition*` ; le canvas n'affiche que le résultat.

### 6.8 Shell desktop Windows
- Exe Rust sans CRT tierce, liaisons `extern "system"` vers `user32`, `kernel32`, `gdi32`, `ole32`, `shell32`, `shcore`, `comdlg32`, et interfaces COM de WebView2 déclarées à la main (tables virtuelles).
- **WebView2** : runtime « Evergreen » préinstallé sur Windows 10/11. Le chargeur `WebView2Loader` est considéré comme une API système Microsoft (cohérent avec D11). Si le runtime est absent : message explicatif.
- Ressources (HTML, JS, CSS, WASM, polices) **incorporées dans l'exe** et servies via `WebResourceRequested` sur un hôte virtuel (`https://app.atelier/`). Politique CSP stricte : **aucune requête réseau sortante**.
- Pont natif ↔ JS : `postMessage` pour les commandes, `SharedBuffer` WebView2 pour les fichiers volumineux.
- Fonctions natives : boîtes de dialogue fichiers (`IFileOpenDialog`/`IFileSaveDialog`), écriture atomique (`ReplaceFileW`), verrous, surveillance des modifications externes (`ReadDirectoryChangesW`), récupération après crash, prise en charge du DPI par écran (Per-Monitor V2), barre de titre et thème système, mode portable (préférences et récupération à côté de l'exe).
- Instance unique : un double-clic sur un document l'ouvre dans l'instance existante.

### 6.9 Serveur web (CGI)
- Binaire Rust compilé en statique pour `x86_64-unknown-linux-musl`, exécuté par Apache à chaque requête sous `/api/` ; temps de démarrage visé < 10 ms.
- **Aucun processus permanent, aucune base de données** : tout est dans des fichiers sous un répertoire de données hors de la racine web.
  ```
  atelier-data/
  ├─ config.ini
  ├─ users/<id>.ini            (identifiant, hash Argon2id, rôle, quota)
  ├─ sessions/<hash>           (jeton haché, utilisateur, expiration)
  ├─ files/<user>/…            (arborescence de l'utilisateur)
  ├─ versions/<user>/<fileid>/ (versions antérieures)
  ├─ trash/<user>/…
  └─ locks/<fileid>            (verrou d'édition, détenteur, expiration)
  ```
- **Écritures atomiques** (fichier temporaire + `rename`) ; verrous de fichiers (`flock`) pour les mises à jour concurrentes ; **ETag** pour le contrôle de concurrence à l'enregistrement (F-WEB-008).
- **Sécurité** : HTTPS fourni par l'hébergeur ; mots de passe hachés en Argon2id ; jetons de session de 256 bits (aléa système `getrandom`), stockés hachés ; cookie `HttpOnly; Secure; SameSite=Strict` ; jeton anti-CSRF ; limitation des tentatives de connexion ; en-têtes CSP/HSTS/X-Content-Type-Options ; normalisation stricte des chemins (pas de traversée de répertoire) ; limites de taille de téléversement ; quotas.
- API REST JSON (sérialisation maison) : `/api/auth/*`, `/api/files/*`, `/api/locks/*`, `/api/versions/*`, `/api/trash/*`, `/api/admin/*`.
- Les mêmes fichiers statiques que le desktop sont servis par Apache ; seul l'objet `Host` JS diffère.

### 6.10 Moteur de calcul (tableur)
- Stockage creux par colonnes en blocs ; valeurs : `Vide | Nombre(f64) | Texte(interné) | Booléen | Erreur | Formule{AST, valeur en cache}` ; formules partagées.
- Analyseur de formules (grammaire Excel, références A1 et R1C1, noms, références structurées) → AST → évaluation.
- **Graphe de dépendances** (cellules + nœuds de plage), recalcul incrémental par ordre topologique, fonctions volatiles, détection des cycles, calcul itératif optionnel.
- Compatibilité Excel : nombres IEEE 754, arrondi d'affichage à 15 chiffres significatifs, système de dates 1900 (y compris le faux 29/02/1900) et 1904, comparaisons texte insensibles à la casse, codes d'erreur.
- Localisation : noms de fonctions et séparateurs selon la langue d'interface (catalogue fr-FR).

### 6.11 Interpréteur Lua
- Compatible Lua 5.4 : analyseur → bytecode → VM à registres, ramasse-miettes incrémental, coroutines, métatables.
- Bibliothèques : `string`, `table`, `math`, `utf8`, `coroutine` ; **pas** de `io`, `os`, `package`, `debug` (remplacés par l'API contrôlée d'Atelier).
- Bac à sable : quota d'instructions et de mémoire par exécution, interruption, permissions par plugin.

### 6.12 PDF
- **Écriture (V1)** : PDF 1.7, sous-ensembles de polices TrueType/CFF incorporés avec table `ToUnicode` (copie et recherche du texte), JPEG en passthrough, images compressées en Flate, liens, signets. PDF/A-2b et PDF balisé en V3.
- **Lecture/édition (V7)** : analyseur (tables xref et flux xref, objets compressés), filtres (Flate, LZW, ASCII85, ASCIIHex, RunLength, DCT, CCITT ; JBIG2 et JPX en dernier), polices Type1/TrueType/CFF/Type3, espaces colorimétriques, transparence, enregistrement incrémental.

### 6.13 Robustesse et protection des données
- Enregistrement : écriture dans un fichier temporaire du même répertoire, vidage sur disque (`FlushFileBuffers` sous Windows, `fsync` sur POSIX), puis publication atomique. Une destination existante est remplacée ; pour un premier enregistrement, la création refuse d'écraser un fichier apparu entre-temps. Une interruption laisse l'ancienne version complète avant publication, ou la nouvelle version complète après publication ; pour une nouvelle destination, le fichier est absent ou complet. Aucune version partielle n'est publiée.
- Récupération : chaque génération associe un identifiant de document, un instantané autonome de l'état de base (ressources comprises, y compris pour un document sans fichier), et un journal versionné de transactions validées. Le journal indique la génération de base et des numéros de séquence ; seules les transactions complètes dont les sommes de contrôle sont valides sont rejouées, jusqu'à la première entrée incomplète ou corrompue.
- L'instantané et son journal sont enregistrés dans le dossier de données résolu au §7.2. Le premier point de récupération est créé à la première modification, puis les transactions sont persistées toutes les 2 minutes par défaut (intervalle paramétrable) et à la mise en veille. La restauration retrouve le dernier point durable ; les modifications postérieures peuvent être perdues en cas d'arrêt brutal, et une erreur de persistance est signalée.
- Après un enregistrement réussi ou un rechargement du fichier source, la nouvelle génération de récupération est publiée atomiquement avant de retirer l'ancienne. Le rejeu part toujours de l'instantané sauvegardé, jamais du fichier source courant ; une modification externe ne change donc pas sa base. La récupération restaure un document distinct et ne remplace pas automatiquement ce fichier.
- En cas d'erreur fatale interceptable, une dernière sauvegarde est tentée si l'état reste accessible. Un arrêt forcé, une coupure de courant ou la perte de l'état du moteur ne permettent pas de garantir cette écriture ; le dernier point durable reste la base de récupération. Les données sont supprimées après enregistrement réussi et fermeture normale, ou après abandon explicite des modifications.
- Tout fichier ouvert est considéré comme **non fiable** : limites de taille, de profondeur XML, de taux de compression zip, de nombre d'objets ; aucune panique ne doit survenir sur une entrée malformée (erreur propre et document partiellement chargé quand c'est possible).
- **Fuzzing** de tous les analyseurs (zip, XML, OOXML, CFB, doc/xls/ppt, polices, images, PDF, Lua) avec un fuzzer mutationnel maison (`tools/fuzz`).

### 6.14 Accessibilité
- Interface (ruban, dialogues, panneaux) en HTML sémantique + ARIA, navigation au clavier, conseils de touches Alt façon Office, focus visible, contrastes AA, prise en charge de `forced-colors` (contraste élevé Windows).
- **Zone du document** : miroir DOM accessible caché de la portion visible (paragraphes, titres, tableaux, listes, images avec texte alternatif) synchronisé avec le curseur et la sélection, exposé aux lecteurs d'écran via UI Automation (WebView2) ; annonces des changements de mise en forme.
- Texte alternatif des images éditable, conservé dans le docx et exposé au lecteur d'écran dans l'éditeur dès V1. Sa restitution accessible dans un PDF balisé relève de V3 (F-COM-015) ; l'export PDF V1 ne garantit pas cette restitution.
- Tests manuels avec NVDA et le Narrateur à chaque version ; grille RGAA 4.1.

### 6.15 Internationalisation
- Chaînes de l'interface dans des catalogues `web/i18n/fr-FR.txt` (clé = valeur), aucune chaîne en dur.
- Formats de nombres, dates, monnaies et listes selon la locale (fr-FR en V1, tables locales maison).
- Le moteur est prêt pour Unicode complet dès V1 (stockage, coupure, curseur par graphème), même si la mise en forme des écritures complexes arrive plus tard.

### 6.16 Objectifs de performance (machine de référence : PC grand public de 2020, 8 Go)

| Mesure | Cible |
|---|---|
| Démarrage à froid (portable) | < 1,5 s jusqu'au document vierge |
| Ouverture d'un docx de 100 pages | < 1 s jusqu'à l'affichage de la 1ʳᵉ page |
| Latence de frappe sur un document de 300 pages | < 16 ms (une image) |
| Défilement | 60 images/s |
| Mémoire, document de 300 pages avec images | < 400 Mo |
| Recalcul d'une chaîne de 1 M de cellules | < 1 s |
| Taille de l'exe portable (V1, polices incluses) | < 30 Mo |

---

## 7. Plateformes et déploiement

### 7.1 Compilation
- Chaîne Rust stable, cibles : `x86_64-pc-windows-msvc` (desktop), `wasm32-unknown-unknown` (moteur), `x86_64-unknown-linux-musl` (serveur).
- Front-end sans étape de build (modules ES servis tels quels, éventuellement concaténés par un outil maison en Rust).
- L'éditeur de liens MSVC et le SDK Windows font partie de la chaîne de compilation (pas des bibliothèques du produit).

### 7.2 Desktop
- **V1, mode portable** : un fichier `atelier.exe` + un dossier `atelier-data/` créé à côté (préférences, récupération, documents récents, rapports de crash et cache WebView2). Atelier place toutes les données qu'il gère dans ce dossier et n'écrit pas dans le registre ; cette règle ne porte pas sur les traces produites indépendamment par Windows.
- Si le dossier n'est pas inscriptible, un dialogue natif est affiché avant toute création de cache WebView2 : déplacer l'exe dans un dossier inscriptible, démarrer en **mode local** avec les données sous `%LOCALAPPDATA%\Atelier` sur choix explicite, ou quitter. Aucun repli automatique ; le mode local est identifié dans l'interface et n'est pas présenté comme portable. Si le dossier choisi reste inaccessible, le démarrage s'arrête avec un message explicatif.
- Les chemins `atelier-data/…` de cette spec et des US désignent le dossier portable par défaut. En mode local, préférences, récupération, récents, cache et rapports utilisent la même racine de données résolue sous `%LOCALAPPDATA%\Atelier`.
- Exe non signé en V1 → avertissement SmartScreen à prévoir (signature de code avec l'installeur).
- **Ultérieur** : installeur maison (associations de fichiers, menu Démarrer), mise à jour automatique **opt-in** (seul autre appel réseau autorisé, désactivable), shells macOS (WKWebView) et Linux (WebKitGTK).

### 7.3 Web
- Paquet de déploiement : `atelier-server.cgi`, dossier `app/` statique, `.htaccess` (règles de réécriture, en-têtes, protection), script d'initialisation (création du répertoire de données et du premier administrateur).
- Procédure documentée pour o2switch (cPanel) : emplacement, droits, activation du CGI, limites de PHP non pertinentes, taille maximale de téléversement.

### 7.4 Limites connues de l'hébergement mutualisé (à vérifier sur o2switch au début de V2)
- Exécution de binaires CGI personnalisés autorisée ? (prérequis bloquant ; plan B : FastCGI ou Passenger si disponible).
- Limites de temps d'exécution, de mémoire et de nombre de processus par requête.
- Pas de WebSocket ni de processus permanent → **co-édition impossible en mutualisé** ; elle nécessitera un VPS ou un serveur dédié (mode « serveur » du même binaire).

---

## 8. Qualité et tests
- **Tests unitaires** : `cargo test` intégré, dans chaque crate.
- **Vecteurs officiels** : cryptographie (NIST, RFC 9106 pour Argon2), deflate, Unicode (fichiers de test UCD : LineBreakTest, GraphemeBreakTest, NormalizationTest).
- **Propriétés** : pour toute opération, `appliquer puis inverser = identité` ; générateur aléatoire maison.
- **Aller-retour** : corpus de documents (créés avec Word/LibreOffice + documents publics de licence compatible) ; ouvrir → enregistrer → rouvrir doit produire un modèle équivalent et préserver les parties inconnues.
- **Rendu de référence** : pages rendues en PNG comparées à des images de référence (tolérance par pixel) ; comparaison visuelle manuelle avec Word documentée pour les cas sensibles.
- **Fuzzing** continu des analyseurs (§6.13).
- **Performance** : bancs de mesure reproductibles sur les cibles du §6.16, suivis à chaque version.
- **Accessibilité** : grille RGAA + tests NVDA/Narrateur avant chaque version.
- Intégration continue (GitHub Actions ou équivalent) : build des trois cibles, tests, clippy, rustfmt.

## 9. Licence et propriété
- Code sous **AGPL-3.0-or-later** ; en-tête de licence dans chaque fichier.
- Polices embarquées : SIL OFL 1.1 (compatibles avec la redistribution).
- Fichiers de données Unicode : licence Unicode (compatible).
- Pas d'utilisation des marques « Office », « Word », « Excel », « OnlyOffice » dans le nom ou l'interface.
- Version web : lien vers le code source correspondant (AGPL §13).
- À décider avant d'accepter des contributions externes : CLA ou non (cf. §11).

## 10. Risques

| Risque | Impact | Mitigation |
|---|---|---|
| Ampleur du projet pour un développeur seul (tout réécrire) | Très élevé | Jalons stricts, V0 limité au strict nécessaire au MVP, chaque brique testée et réutilisée par tous les modules |
| Fidélité de mise en page vs Word | Élevé | Corpus de référence, polices de métriques compatibles, préservation de l'inconnu, imitation du comportement de Word plutôt que de l'« idéal » |
| Cryptographie maison (Argon2, AES, signatures) | Élevé (sécurité) | Implémentations strictement conformes aux spécifications, vecteurs de test officiels, temps constant pour les comparaisons, aucun algorithme inventé |
| Analyse de fichiers malveillants | Élevé | Rust sûr, limites, fuzzing, pas de panique |
| Formats binaires anciens (doc/xls/ppt) très vastes | Moyen | Lecture seule, couverture progressive, priorité au contenu courant |
| Dépendance au runtime WebView2 | Moyen | Runtime présent sur Windows 10/11 ; message clair s'il manque ; runtime « Fixed Version » possible en option |
| CGI non autorisé ou trop limité chez o2switch | Moyen | Vérification dès le début de V2 ; plan B FastCGI/Passenger |
| Décodage audio/vidéo pour les présentations | Moyen | Préservation et lecture différée ; codecs ouverts d'abord |
| Taille du paquet avec modèle IA | Faible | Variante avec ou sans modèle |

## 11. Points ouverts (non bloquants pour la validation)
1. **Nom définitif** du produit (vérification de disponibilité des marques et domaines).
2. **RTF** : lecture/écriture en V3 ? (format courant pour le presse-papiers et les échanges).
3. **Écriture ODF** (odt/ods/odp) : à prévoir à terme ?
4. **CLA** pour les contributions externes (permet une double licence future).
5. **Modèle IA** embarqué : choix du modèle (licence compatible avec la redistribution, taille).
6. **Mobile** (iOS/Android) : dans la feuille de route ou non.
7. **Télémétrie de crash** : aucune (choix actuel, cohérent avec « déconnecté ») — rapport de crash enregistré localement que l'utilisateur peut envoyer manuellement ?

---

## Annexe A — Fonctions du tableur en V5a (~120)

| Catégorie | Fonctions (noms anglais stockés ; noms français affichés) |
|---|---|
| Math | SUM, SUMIF, SUMIFS, SUMPRODUCT, PRODUCT, ABS, ROUND, ROUNDUP, ROUNDDOWN, INT, TRUNC, MOD, POWER, SQRT, EXP, LN, LOG, LOG10, PI, RAND, RANDBETWEEN, CEILING, FLOOR, SIGN, QUOTIENT |
| Statistiques | AVERAGE, AVERAGEIF, AVERAGEIFS, COUNT, COUNTA, COUNTBLANK, COUNTIF, COUNTIFS, MIN, MAX, MINIFS, MAXIFS, MEDIAN, MODE, STDEV, STDEVP, VAR, VARP, LARGE, SMALL, RANK, PERCENTILE, QUARTILE |
| Logique | IF, IFS, AND, OR, NOT, XOR, IFERROR, IFNA, SWITCH, TRUE, FALSE |
| Texte | CONCATENATE, CONCAT, TEXTJOIN, LEFT, RIGHT, MID, LEN, LOWER, UPPER, PROPER, TRIM, SUBSTITUTE, REPLACE, FIND, SEARCH, TEXT, VALUE, REPT, EXACT, CHAR, CODE |
| Date et heure | TODAY, NOW, DATE, TIME, YEAR, MONTH, DAY, HOUR, MINUTE, SECOND, WEEKDAY, WEEKNUM, EDATE, EOMONTH, DATEDIF, NETWORKDAYS, WORKDAY, DAYS |
| Recherche | VLOOKUP, HLOOKUP, XLOOKUP, INDEX, MATCH, CHOOSE, ROW, COLUMN, ROWS, COLUMNS, INDIRECT, OFFSET |
| Information | ISBLANK, ISNUMBER, ISTEXT, ISERROR, ISNA |
| Finances | PMT, FV, PV, NPV, RATE |
