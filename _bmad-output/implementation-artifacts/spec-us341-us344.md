---
title: 'US-341 / US-344 — Remplacement TXT et barre d’état configurable'
type: feature
created: '2026-10-13'
status: done
route: dispatch
baseline_commit: 73eb4d0a851cee4d930c6ff639676ddee2e274ca
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="L’utilisateur demande de développer un lot complémentaire avec plusieurs agents GPT-6 Luna moyen, sans nouvelle confirmation">

## Intent

Permettre de corriger le TXT par Ctrl+H, remplacer l’occurrence choisie ou toutes les occurrences en une seule opération annulable, et afficher une barre d’état utile/configurable avec accès visuel à une page. Deux livrables indépendants autorisés dans un lot parallèle. Le lot précédent fournit recherche, Unicode, statistiques et zoom ; réutiliser ces modules.

## Boundaries & Constraints

Remplacement limité au TXT éditable ; aperçu DOCX en lecture seule, mise en forme riche non raccordée. L’US-341 reste ouverte pour l’héritage des attributs riches. La barre d’état indique pages, mots, langue, vue, zoom et enregistrement ; le TXT sans métadonnée de langue affiche « non définie ». Ne pas confondre langue de l’interface et langue du document. Les mots du DOCX restent indisponibles ; ne pas clôturer les critères riches. Les préférences de visibilité vivent pendant la session.

Ne modifier aucune branche externe 291/306/316. Leurs API de navigation riche, préservation DOCX et IO texte restent séparées. Accès visuel à une page basé sur le layout déjà affiché, sans ajouter de signets ni modifier le document. Parent seul propriétaire de app/editor/navigation-tools, renderer, shell, catalogue, routes, packaging et docs. Pas de dépendance externe, pas de nouveau repli français en JS, garder le budget WASM existant.

Remplacement exact UTF-8 via le Searcher Rust, avec les quatre options existantes. Requête max 1 024 octets, remplacement et document résultant max 128 Kio. Remplacer tout porte sur toutes les occurrences de la source d’origine, indépendamment du plafond de 1 000 résultats de l’UI ; aucun remplacement récursif du texte inséré. Une transaction isolée, un seul Undo restaure texte et sélection initiaux ; Redo réapplique. Pas de modification partielle en cas de dépassement, erreur de layout/historique ou révision périmée. Une absence de match ou un remplacement identique ne crée pas d’entrée d’historique. Le compte rapporte les occurrences traitées, même identiques.

## I/O & Edge-Case Matrix

| Situation | Résultat attendu |
|---|---|
| Unicode, casse/accent/NBSP/mot entier | Mêmes plages originales que la recherche Rust ; aucun grapheme coupé |
| Un match sélectionné, ou sélection différente/stale | Remplacement exact ; refus explicite si révision/sélection/plage ne correspondent plus |
| >1 000 résultats, texte réintroduisant la requête | Tous les matches originaux traités une seule fois, compte exact, transaction unique |
| Zéro match, remplacement identique | Texte, dirty, révision et historique conservés ; compte explicite |
| Document résultant trop grand, trame invalide | Rejet atomique avant publication du candidat ; négatifs codec et Rust |
| Édition/load/changement de requête/fermeture tardifs | Générations invalidées ; aucune réponse tardive ne rouvre le volet ni ne remplace un autre document |
| Modal/IME/AltGr/aperçu/busy | Ctrl+H scoped, commandes bloquées quand nécessaire ; aucune mutation en aperçu |
| Page invalide, resize/zoom/scroll | Dialogue validé ; déplacement visuel au bon layout sans changer sélection/historique |
| Clic droit / Shift+F10 | Menu nommé et clavier ; choix visibles reflétés par aria-checked ; visibilité de chaque champ configurable |
| Catalogue absent/partiel, disposal, 420 px/forced colors | Pas de clé brute ni outil sans libellé ; cleanup complet ; interface accessible sans débordement |

</frozen-after-approval>

## Code Map

- `crates/text-model/src/search/` : seul algorithme Unicode ; `PlainTextDocument::replace_range_isolated` fournit la transaction/historique. Aucune modification des modules riches/navigation/plain_io.
- `crates/core-wasm/src/editor.rs`, `editor_queries.rs` : session candidate clonée, contrôle révision, layout/cache et nouveaux opcodes 21/22. Engine agent propriétaire de Rust et codec uniquement.
- `web/editor-protocol.mjs` : commandes strictes et nouveau résultat ATQR contenant le compte et un ATES complet.
- `web/replace-controls.mjs/.css` : nouvelle vue indépendante, agent UI. Parent raccorde son slot au volet existant.
- `web/document-status.mjs/.css` : nouvelle barre indépendante et dialogue/menu accessibles, agent statut.
- Parent : `web/navigation-tools.mjs`, `editor.mjs`, `rich-preview.mjs`, `app.mjs`, `editor-shell.mjs`, `index.html`, `app.css`, catalogue/runners/routes ; méthodes de déplacement visuel et notifications de viewport.

## Tasks & Acceptance

- [x] Agent moteur : commandes Replace/ReplaceAll, codec ATQR, tests atomiques/Unicode/historique et contrat binaire.
- [x] Agent UI : contrôles Remplacer indépendants, états et tests DOM.
- [x] Agent statut : barre, menu de visibilité, dialogue Atteindre une page et tests DOM.
- [x] Parent : Ctrl+H, publication des états, invalidation/races, intégration des champs existants et vues TXT/DOCX.
- [x] Parent : revues indépendantes, vraie exécution Worker/WASM/navigateur/native, corpus, budget, docs/backlog, Graphify, audit Git et publication.

AC : Given un TXT et une requête, when Remplacer tout, then chaque occurrence originale est remplacée, le compte exact s’affiche et un Undo restaure l’ensemble. Given une sélection/révision devenue périmée, when une commande arrive, then elle échoue sans changer le document. Given une page affichée, when Atteindre ou un défilement agit, then la barre reflète la page visible sans déplacer la sélection. Given le menu de barre d’état, when un champ est décoché, then ce champ disparaît et son état accessible est cohérent.

## Implementation Notes

Contrats figés avant dispatch :

- Opcodes 21 Replace, 22 ReplaceAll. Corps : `revision:u64`, `nowMs:u64`, flags options Search `u8`, reserved `u8+u16=0`, `queryLen:u32`, `replacementLen:u32`; pour 21 seulement `start:u32,end:u32`; puis query UTF-8 et replacement UTF-8. JS values `{revision,nowMs,query,replacement,options,start,end}` (start/end seulement Replace). Révision erronée `EDITOR_STALE_QUERY`, match/sélection erronés `EDITOR_STALE_MATCH`, requête vide `EDITOR_EMPTY_QUERY`. UTF-8 strict, options inconnues/trailing bytes refusées.
- Réponse ATQR : magic4, version `u16=1`, reserved `u16=0`, count `u32`, stateLen `u32`, puis état ATES exact. Décodage `{kind:'replacement',count,state}` ; état imbriqué strict, compte borné à 131 072, réponse totale ≤32 Mio. Le parent publie uniquement `state`, retourne aussi le compte.
- Construire le remplacement global avant mutation, en utilisant un itérateur Searcher non plafonné ; au plus une opération `replace_range_isolated` sur la plage englobante, un seul layout final. Vérifier budgets/overflow avant allocation. Source d’origine immuable pendant l’énumération.
- `createReplaceControls({document,labels,onReplace,onReplaceAll})` → `{element,setState({visible,disabled,busy,canReplace,canReplaceAll,count,error}),focus(),dispose()}`. Callbacks reçoivent `{replacement:string}`. Deux lignes textarea, valeur conservée au rerender ; aucune interprétation de markup. Labels : `replaceTitle`, `replaceText`, `replaceOne`, `replaceAll`, `replaceBusy`, `replaceError`, `replaceNone`, `replaceCountOne`, `replaceCountMany` (dernier `{count}`). `count:null` n’annonce rien.
- `createDocumentStatus({document,labels,menuTarget,onGoToPage,onVisibilityChange})` → `{element,setState({page,totalPages,language,mode,dirty,busy}),getVisibility(),dispose()}`. Page 1-based ; `language:null` non définie ; `mode:'text'|'preview'`; dirty null = non disponible. `onGoToPage(page)` après validation entière 1..totalPages. `onVisibilityChange({field,visible})` avec `page,words,language,view,zoom,saved`; widget contrôle ses quatre champs, parent contrôle slots mots/zoom. Menu contextuel réutilise les composants accessibles existants ; aucun accès métier.
- Labels statut : `statusPageCount` (`{page}`,`{total}`), `statusPageUnavailable`, `statusLanguage` (`{language}`), `statusLanguageUnknown`, `statusViewText`, `statusViewPreview`, `statusSaved`, `statusUnsaved`, `statusReadOnly`, `statusConfigure`, `statusShowPage`, `statusShowWords`, `statusShowLanguage`, `statusShowView`, `statusShowZoom`, `statusShowSaved`, `statusGotoTitle`, `statusGotoPage`, `statusGotoApply`, `statusGotoCancel`, `statusGotoInvalid`, `statusGotoRange` (`{total}`), `statusGotoClose`.

## Spec Change Log

## Review Triage Log

Trois revues indépendantes GPT-6 Luna moyen, toutes lancées avant le triage, ont examiné le diff figé du baseline. Le choix de modèle suit la demande utilisateur. Aucun défaut produit retenu n’est différé.

| ID | Verdict | Route | Preuve et décision |
|---|---|---|---|
| B1 langue toujours null | false | rejet | TXT et protocole d’aperçu ne fournissent pas de langue documentaire. Aucun cas de langue connue exposée n’a été établi ; afficher la locale serait une fausse métadonnée. Limite riche explicitement conservée. |
| B2 absence de mots dans widget | false | rejet | createNavigationTools monte le compteur document/sélection existant dans statistics-slot. Le test réel observe ce compteur ; le champ est fourni par le parent. |
| B3 masquer statistics-slot | false | rejet | Ce slot contient précisément le compteur cliquable, donc le champ Mots est bien masqué. La commande Statistiques reste accessible dans Affichage. |
| B4 visibilité mots/zoom séparée | false | rejet | onVisibilityChange applique les deux slots parent ; l’intégration teste masquage/restauration et aria-checked. Aucun consommateur divergent démontré. |
| B5 persistance après reload | false | rejet | La configuration de cette livraison vit pendant la session ; la recréation/recharge termine cette instance. Aucune persistance après fermeture promise. |
| B6 Tab stop sans action | false | rejet | Maj+F10 ouvre le menu depuis le groupe ; avec menuTarget footer, l’événement remonte vers cette cible. Le groupe constitue donc un accès clavier utile. |
| B7 dialogue sans bouton en catalogue partiel | low | rejet | Possible uniquement pour un appel direct avec tous les labels goto mais sans labels page ; app.mjs exige le catalogue statut complet. Aucun impact utilisateur ordinaire, aucun nouveau garde ajouté. |
| B8 callback refusé sans retour | false | rejet | Le parent garde busy/initialisation et les erreurs goToPage sont affichées via onError. Les entrées invalides gardent le dialogue ouvert. Le repro de perte silencieuse n’est pas établi. |
| B9 géométrie malformée | false | rejet | L’accès optionnel produit undefined et la validation explicite !Number.isFinite(height) lève le TypeError prévu. Aucun accès brut non protégé. |
| B10 mémoire plages dense | false | rejet | Source bornée à 128 Kio, requête non vide et matches non chevauchants : au plus 131 072 plages, avec try_reserve. Aucun document arbitrairement grand ne rejoint cet algorithme. |
| E1 overflow flottant | false | rejet | Les deux appelants reçoivent uniquement des pages ATDL décodées (coordonnées ≤1e9), zoom 0,1..5, écart16 et nombre de pages borné par le protocole. Les tailles Number.MAX_VALUE du repro ne franchissent pas ces entrées ; pas de position infinie utilisateur. |
| V1 goto déjà visible | medium | patch | Test réel démarrait page2 puis demandait page2 ; corrigé pour revenir page1, demander2 et constater le déplacement et l’état inchangé. |
| V2 options ReplaceAll non exercées | medium | patch | Les options étaient seulement cherchées ou appliquées à ReplaceCurrent ; ajout de résultats/compteurs exacts de ReplaceAll pour les quatre options. |
| P1 Redo testé depuis état édité | medium | patch | Le test Rust utilisait changed_session au lieu de la session Undo ; corriger la chaîne réelle et les assertions sélection/historique. |

## Verification

Rust + codec négatifs et tests réels Worker/WASM, >1 000 remplacements, no-op, stale, budgets, Unicode, Undo/Redo, races contrôlées. UI normale/contraste élevé, catalogue manquant/partiel, 420 px, menu clavier, scroll/goto TXT/DOCX, non-mutation. Compilation/Clippy hôte et WASM, tests workspace, corpus/i18n/licences/lock, repackage moteur et seuil inchangé, sonde WebView2, Graphify conservant statuts/concepts, audit des octets Git. Pas de certification Word ni clavier/lecteur d’écran physique.

Validation finale : 21 contrôles réussis, 1 085 tests Rust exécutés, 219 assertions éditeur/Worker/WASM, 263 UI normale et 266 couleurs forcées ; 1 152 entrées sources immuables. Régressions Canvas/saisie et catalogue également réussies. Sonde native : 41 ressources, CSP et transport Worker, saisie/Undo/Redo, 64 entrées conservées avec 34 rotations, objets callback 0→0. WASM 4 213 853 octets (plafond 4 271 903 inchangé), SHA-256 `5dff84d02ac5896fc2e032764172f2cf71ebb985bacaa58a3bdf385b60a7e228`. Graphe : 9 320 nœuds / 23 878 arêtes, 70 concepts documentaires historiques et 56 relations conservés, 34 paquets locaux sans cycle.

Le lot validé est terminé ; les US complètes restent ouvertes pour les critères riches indiqués. Les correctifs de revue ont été vérifiés dans une passe globale fraîche, sans réutiliser le premier reçu. Aucun défaut produit retenu différé.
