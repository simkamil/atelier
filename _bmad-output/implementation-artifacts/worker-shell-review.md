# Revue du lot Worker et coque Windows — 2026-10-11

Trois revues indépendantes avec GPT-6 Luna moyen, sans historique partagé, sur le diff complet depuis c091f6a52aa58af94966592ff356750471200d44. Les 15 constats sont classés individuellement avant regroupement. US-092 reste ouverte pour son critère métier ; US-110 et US-116 sont candidates à validation.

## Review Triage Log

| ID | Verdict | Constat | Preuve et décision | Route |
|---|---|---|---|---|
| B1 | false | DPI système pour tous les écrans | run et self_check créent d’abord un HWND caché sur le rectangle borné, lisent GetDpiForWindow puis remplacent le DPI de cet écran et recalculent la restauration avant affichage ; les autres écrans provisoires ne déterminent pas la taille finale. | rejet |
| B2 | false | Les coordonnées écran seraient multipliées par le DPI | restore_state applique scale seulement à width/height ; left/top sont bornés sans multiplication. Le déplacement décrit ne se produit pas. | rejet |
| B3 | low | Taille initiale fixe en pixels physiques | default_for ignore le DPI pour 1024x720 : à 192 DPI la taille logique est divisée par deux. Correction directe des dimensions et bornage. | patch fenêtre |
| B4 | medium | Minimum fixe en pixels physiques | restore_state et WM_GETMINMAXINFO utilisent 640x420 fixes : à 192 DPI le minimum logique descend à 320x210. Même cause que B3, helper partagé et tests 192 DPI. | patch fenêtre |
| B5 | false | create_dir_all écrirait à travers le lien racine avant rejet | Une racine qui est déjà un lien vers un dossier existant ne nécessite aucune création par create_dir_all ; symlink_metadata rejette ensuite le lien avant la sonde. Un lien cassé fait échouer la création. Le cas cité ne crée pas de descendant. | rejet |
| B6 | medium | Le self-check crée un descendant avant de rejeter une jonction sortante | Le chemin target/jonction/manquant passe le contrôle lexical, create_dir_all crée manquant au-delà de target puis canonicalize refuse. Valider les ancêtres existants avant toute création. | patch fenêtre |
| B7 | medium | Signal incomplet laisse une promesse non réglée | request vérifie addEventListener seul ; removePending appelle removeEventListener après suppression de la Map et avant resolve/reject. Un objet admis sans cette méthode reproduit le blocage. | patch Worker |
| B8 | maybe-false | Échec de SetWindowLongPtrW ignoré pendant WM_NCCREATE | La valeur de retour n’est pas contrôlée, mais le HWND neuf appartient au même thread/processus et GWLP_USERDATA est valide : aucun scénario atteignable d’échec n’est démontré. La documentation Microsoft précise zéro+GetLastError, sans montrer un échec avec ces arguments. Il faudrait une injection d’échec natif ; impact non établi et correction ajoutant des branches, rejet du durcissement hypothétique. | rejet (faible si confirmé) |
| B9 | medium | Banc navigateur absent de CI | Le banc existe mais seul worker_threads est invoqué. Le bootstrap WorkerGlobalScope navigateur peut régresser sans échec Node ; même cause que V2. | patch navigateur |
| B10 | false | Le choix Local n’est pas mémorisé | Le contrat exige un choix explicite quand le portable est inaccessible, à chaque résolution ; aucune préférence persistante de basculement n’est demandée. Redemander conserve cette garantie et ne perd aucune donnée locale. | rejet |
| E1 | medium | Worker silencieux : initialisation sans limite | await ready ne comporte aucun délai. Un Worker vivant qui ignore init laisse createEngineClient pendante ; un délai privé doit utiliser failAll et retirer les listeners. | patch Worker |
| E2 | false | Énumération interrompue accepterait une liste incomplète | collect renvoie FALSE sur erreur et monitors rejette EnumDisplayMonitors==0. Vérification Win32 réelle via ctypes : callback appelé une fois puis FALSE, résultat EnumDisplayMonitors=0 ; le résultat incomplet n’est pas rendu. | rejet |
| E3 | medium | Jonction self-check et descendant absent | Vérifié séparément : create_dir_all précède la vérification canonique et peut écrire hors target. Même défaut que B6. | patch fenêtre |
| V1 | medium | Les actions du démarrage normal ne sont pas vérifiées | Constat pré-vérifié : tests resolver et self-check contournent le mapping main des trois outcomes. Vérifier le coordinateur privé avec le vrai resolver et les actions injectées pour chemin/titre/arrêt, sans nouvelle API publique. | patch intégration |
| V2 | medium | Bootstrap navigateur par défaut non automatisé | Constat pré-vérifié : le point d’entrée Node installe explicitement le runtime ; il ne couvre pas le guard WorkerGlobalScope du navigateur. Automatiser le banc dans un vrai Edge headless local et CI. | patch navigateur |

Regroupements après classification : B3/B4 (dimensions DPI), B6/E3 (jonction self-check), B9/V2 (exécution navigateur). B7, E1 et V1 restent distincts. Les corrections B3/B4, B6/E3, B7 et E1 ont été appliquées par leurs agents d’implémentation, puis testées. V1 utilise le vrai resolver et vérifie les actions, chemins, titres et erreurs de fenêtre. B9/V2 sont couverts par un vrai Edge headless, qui exécute le point d’entrée ES navigateur de production et fait échouer le contrôle/CI en cas d’erreur. La revue de vérification ciblée confirme V1/V2 couverts.

Référence native : [SetWindowLongPtrW](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowlongptrw) et [EnumDisplayMonitors](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-enumdisplaymonitors).

## Corrections et preuves

- Dimensionnement à 192 DPI et petite work area : tests natifs ciblés réussis. Les coordonnées physiques enregistrées ne sont pas multipliées.
- Jonction self-check : validation de chaque ancêtre avant création ; test Windows réel `mklink /J`, obligatoire, fixture unique possédée sans suppression récursive.
- Signal incomplet et initialisation silencieuse : régressions Worker passées ; délai privé de 30 secondes avec nettoyage.
- Démarrage normal : vrai resolver portable/local/Move/Quit ; chemin et titre transmis, erreur de fenêtre propagée.
- Navigateur : `node tests/run-worker-browser.mjs` passe sur Edge 155 headless sans crossOriginIsolated ; cinq scénarios terminés. La première tentative CUA a échoué avant toute exécution ; le runner direct apporte la preuve navigateur effective.

Corrections du coordinateur pendant l’intégration : nettoyage d’une collision de fichier temporaire limité au fichier créé ; capture de la longueur initiale pour libérer l’entrée WASM même après détachement ; contrôle du dernier bloc ; maintien de la fenêtre self-check cachée ; traitement des préfixes absolus Windows ; assertions de vraie sonde d’écriture ; vérification de l’écho déplacée dans le Worker et contrôle limité à l’en-tête côté appelant ; avertissements Clippy corrigés. Le compteur Graphify Cargo est 32 paquets, distinct des 34 US terminées attendues.

Aucun constat différé dans les deux US Windows et le transport livré. US-092 reste explicitement ouverte pour l’ouverture/export métier annulable ; ce critère n’est pas déclaré validé. Les tests multi-écran utilisent des géométries synthétiques. Validation finale complète : voir worker-shell-validation.json ; seuls des contrôles tous réussis sur sources inchangées autorisent la clôture documentaire.
