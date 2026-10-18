# US-103 — Adaptateur de saisie DOM

`web/text-input.mjs` fournit une petite surface d’entrée au navigateur. Elle conserve le clavier, les touches mortes, AltGr et l’IME dans un vrai `textarea`, puis transmet les caractères résolus et les opérations à l’éditeur. Le module ne lit jamais `KeyboardEvent.key` pour fabriquer du texte et ne modifie pas le modèle documentaire.

```js
import { attachTextInput } from "../web/text-input.mjs";

const input = attachTextInput({
  getSelection: () => editor.selection(),
  getCaret: () => ({ x: caret.x, y: caret.y, width: caret.width, height: caret.height }),
  onCommand: command => editor.applyInput(command),
  onComposition: preview => {
    if (preview.phase === "start" || preview.phase === "update") {
      // Cette fonction d’application met en page l’aperçu et renvoie ses rectangles CSS.
      canvasRenderer.setOverlay(layoutCompositionPreview(preview));
    } else {
      canvasRenderer.clearOverlay();
    }
  },
  onUnhandled: event => editor.delegateBrowserInput(event),
  onError: error => console.error("Text input adapter:", error),
});

input.focus();                 // seulement quand le propriétaire le demande
input.updateCaret();           // relit getCaret()
input.updateCaret({ x: 40, y: 90, width: 1, height: 18 });
input.dispose();
```

L’objet retourné expose aussi `element`, le `textarea` créé ou fourni. Un textarea créé reçoit le nom accessible français `Saisie du texte du document`; le propriétaire peut le remplacer via `input.element.setAttribute("aria-label", "…")`. Le caret est un rectangle en pixels CSS du viewport (`x`/`y`, ou `left`/`top`, `width`, `height`). Toutes ses valeurs doivent être finies et non négatives. L’adaptateur positionne le `textarea` en `position: fixed`, avec une opacité nulle ; il reste focusable et n’utilise ni `display: none`, ni `visibility: hidden`, ni `contenteditable`. L’attachement ne prend pas le focus. `focus()` le demande explicitement et ne passe pas devant un dialogue HTML ouvert. Aucun retour de `window` au premier plan ne déclenche un focus automatique.

`getSelection()` fournit la sélection courante, dans le format choisi par le propriétaire. Chaque commande la transporte dans `selection`. La commande texte est `{ type: "insertText", text, inputType, selection }`. Les retours à la ligne et suppressions sont routés comme `insertText` et `{ type: "delete", unit, direction, inputType, selection }`. `Ctrl+Z`, `Ctrl+Y`, `Ctrl+Maj+Z` et les touches de navigation reconnues produisent respectivement `undo`, `redo` et `move`. Les suppressions et caractères proviennent de `beforeinput`/`input`, jamais d’une traduction des touches.

Une composition forme une transaction séparée. `onComposition` reçoit `{ phase, text, selection, caret }`, avec `phase` égal à `start`, `update`, `commit` ou `cancel`. La sélection et le caret sont capturés au début de la composition. Les mises à jour transmettent un aperçu sans commande d’insertion ; le propriétaire met ce texte en page puis passe les rectangles CSS calculés à `CanvasRenderer.setOverlay(rectangles)`. Il appelle `clearOverlay()` au commit ou à l’annulation. L’adaptateur ne calcule ni la mise en page ni la géométrie de soulignement. Un `compositionend` non vide produit un seul aperçu `commit` et une commande texte. Un `compositionend` vide annule sans insérer. Le `insertText` final parfois envoyé après `compositionend` par Chromium est reconnu et supprimé.

Le collage `text/plain` est inséré comme texte. Pour les types non pris en charge ou les données presse-papiers inaccessibles, `onUnhandled`, lorsqu’il est fourni, reçoit le type, les données disponibles et la sélection ; l’adaptateur laisse l’événement navigateur continuer. Le propriétaire peut alors déléguer explicitement le traitement. Sans délégué, un `inputType` inconnu reste au navigateur et l’adaptateur ne vide pas le `textarea` : le texte natif éventuel y reste disponible. Les `inputType` connus continuent d’être routés puis nettoyés. `onError` reçoit les exceptions de callbacks. Une erreur pendant un aperçu annule la transaction et vide la surface après la fin de la composition native, sans changer le focus. Le nettoyage différé évite de perturber une composition encore active.

`dispose()` retire les listeners. Il annule une composition active, puis arrête tout callback ultérieur. Il supprime le `textarea` qu’il a créé. Pour un élément fourni, il restaure les attributs, le style, la valeur et la sélection initiaux.

## Limites et essais réels

Les vérifications synthétiques de `tests/input-browser.mjs` exercent le routage DOM dans Chromium et retournent `runInputChecks()`, un tableau des scénarios réussis. Elles ne valident pas les pilotes clavier ni les services IME natifs. Avant de clôturer US-103, vérifier manuellement sous Windows/WebView2 avec un clavier AZERTY : touches mortes `^`, `¨`, `` ` `` ; AltGr `€`, `@`, `#`, `{`, `[` ; Alt+code numérique ; composition IME, validation et annulation ; focus d’un dialogue et d’un autre champ. Les événements synthétiques ne constituent pas une preuve de ces essais matériels.

Cet adaptateur reçoit maintenant la sélection et la géométrie du [contrôleur US-120](us-120-editor-ui.md), qui envoie les commandes au moteur et dessine texte, caret, sélection et précomposition soulignée. L’adaptateur reste indépendant du document. Les preuves synthétiques ne remplacent pas les essais physiques AZERTY/IME.

Validation finale du lot : [34 contrôles fonctionnels et qualité](../_bmad-output/implementation-artifacts/frontend-host-validation.json),
**318 tests workspace et 27 tests shell release**, sur sources et lock inchangés.
Les tests Chromium couvrent Canvas, saisie et leur composition intégrée.
Les deux profils WebView2 accomplissent 100 cycles fonctionnels avec callbacks équilibrés ;
le rapport conserve séparément l'échec strict des handles et US-111 reste ouverte.
La [revue indépendante](../_bmad-output/implementation-artifacts/frontend-host-review.md)
conserve un point IME natif de gravité moyenne non vérifiée à trancher sur matériel.
Les tests musl sont compilés localement ; leur exécution appartient à la CI Linux.

## Mode lecteur d’écran (US-209)

`attachTextInput` accepte `screenReaderMode` et `onScreenReaderMode`, et expose `setScreenReaderMode(on)`, `setScreenReaderContent({ text, start, end, backward })` et `screenReaderMode()`. Ctrl+Alt+Z (touche `z` réelle, pas AltGr) bascule le mode. Dans le mode, la zone contient le paragraphe courant et sa sélection au lieu de rester vide ; la saisie, la composition et le presse-papiers passent par les mêmes commandes. Hors mode, rien ne change. Voir [le miroir DOM](us-209-document-mirror.md).
