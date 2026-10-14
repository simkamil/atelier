---
title: 'US-230 — Menu contextuel du TXT'
type: feature
created: '2026-10-13'
status: done
route: dispatch
baseline_commit: f419d339daef132a29fdbdeb68ca0093aaaa0a57
context: []
---

<frozen-after-approval reason="Poursuite autonome du backlog autorisée">

## Intent

Donner accès aux actions TXT pertinentes par clic droit, touche Menu/ContextMenu et Maj+F10. Annuler, Rétablir, Couper, Copier, Coller et Tout sélectionner utilisent les commandes réelles de l'éditeur et le presse-papiers texte. L'ouverture préserve la sélection courante. Les contextes lien/tableau/image/en-tête ne sont pas encore éditables ; leur couverture reste ouverte dans US-230 et aucun menu factice n'est créé.

## Boundaries & Constraints

Réutiliser createContextMenu, DOM textContent et libellés injectés. Le menu calcule les actions disponibles à chaque ouverture depuis un snapshot (revision/anchor/focus/text/canUndo/canRedo/busy). Les actions mutantes sont désactivées pendant busy/composition/aperçu. Le parent fournit le vrai adaptateur clipboard et valide encore le snapshot au moment de l'action, notamment après une opération clipboard asynchrone. Aucun événement clavier synthétique utilisé comme commande d'édition. Le focus revient à l'entrée éditeur sans changer la sélection. Les refus clipboard sont remontés au callback onError, sans supprimer le texte.

## I/O & Edge-Case Matrix

| État | Action | Résultat |
|---|---|---|
| Sélection non vide | Clic droit/Menu/Maj+F10 | Menu accessible ; snapshot inchangé |
| Aucun historique/aucune sélection | Ouvrir | Undo/Redo/Cut/Copy désactivés selon état |
| Clipboard indisponible | Ouvrir | Cut/Copy/Paste indisponibles selon capacités annoncées |
| Action asynchrone refusée | Activer | onError appelé ; aucune mutation par le composant |
| Aperçu/busy | Ouvrir | Aucun menu d'édition actif |
| Menu actif | Échap/flèches/Home/End/Tab | Comportement partagé, fermeture et focus corrects |

</frozen-after-approval>

## Code Map

- `web/ui-components.mjs::createContextMenu` : gestion focus, position, clavier ; possède items statiques, donc recréer le menu à chaque ouverture si nécessaire, disposer l'ancien.
- `web/editor-context-menu.mjs` : seul nouveau module agent.
- `tests/editor-context-menu-browser.mjs` : suite exportable au runner parent.
- Editor/app/catalogue/ressources : parent exclusivement. Specs US230 présentes dans US-V0-V1.md, recherche avec `py target/read-sections.py` si rg échoue sur Windows.

## Tasks & Acceptance

- [x] API `createEditorContextMenu({target,focusTarget=target,labels,getState,onCommand,onError,clipboardCapabilities})`, retour `{element,open,close,dispose}`. `getState` inclut enabled/busy ; onCommand(action,snapshot) asynchrone ; clipboardCapabilities `{read,write}`.
- [x] Écouter contextmenu sur target et clavier sur focusTarget ; empêcher les doubles ouvertures, ignorer événements hors cible et composition.
- [x] Tester l'état recalculé, sélection conservée, actions désactivées, erreurs et stale snapshot transmis, nettoyage des écouteurs.
- Given une sélection active, when le menu est ouvert au clavier ou à la souris, then revision/anchor/focus restent identiques et les actions utilisent le snapshot capturé.
- Given une action désactivée, when elle est activée artificiellement, then aucun callback métier ne s'exécute.

## Implementation Notes

Agent limité à son nouveau module et tests ; parent intègre les commandes et leur validation asynchrone. Le rafraîchissement Graphify est centralisé par le parent.

## Review Triage Log

| ID | Verdict | Vérification et décision |
|---|---|---|
| E3 | false | Le diff initial utilisait une échéance de 500 ms, sans le reset de 50 ms cité ; chaque ouverture remplaçait/disposait le popup précédent. Aucun double popup démontré. Le problème réel de clic droit après fermeture est traité séparément en P1. |
| P1 | medium | Le clic droit réel immédiatement après fermeture d’un menu clavier était ignoré pendant la fenêtre temporelle. Coalescence maintenant liée à la visibilité du popup, sans minuterie ; tests ouverture clavier/native et fermeture/clic droit. |
| P2 | low | Le test de coupe différée se fondait sur 50 ms d’attente. Il attend maintenant une nouvelle mutation du statut d’erreur avant de vérifier texte et révision. |

## Verification

Suite navigateur réelle et test intégré Worker/WASM par le parent ; aucun support clipboard riche revendiqué.

Validation finale du lot : Validation Windows du 2026-10-14 : 18 contrôles, 243 tests Rust des quatre paquets concernés, 1177 entrées immuables. Les suites Edge contrôlent les composants, le produit normal/contraste élevé et le vrai Worker/WASM ; Clippy hôte/WASM, fmt, licences, i18n sans nouvelle dérogation de référence, données générées, budget WASM inchangé et sonde WebView2 passent. La sonde vérifie 49 ressources et les textes de licence embarqués. Les événements clavier sont synthétiques ; aucun audit lecteur d’écran, essai matériel ou passage Linux distant n’est revendiqué.
