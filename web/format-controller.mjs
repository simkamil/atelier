// SPDX-License-Identifier: AGPL-3.0-or-later
// Contrôleur partagé de la mise en forme (préparation d'US-240 et US-243) : interroge FormatState UNE fois par
// changement utile (sélection, révision, ouverture ou réouverture, lecture seule, Resync, occupé), quel que soit le
// nombre de modules abonnés, jette les réponses périmées et diffuse l'état aux abonnés. Il porte aussi le routeur
// de raccourcis clavier des modules (`keyBindings()`), pour qu'aucun module ne touche app.mjs.
//
// Contrat (docs/us-240-format-protocol.md §7) :
//   const controller = createFormatController({ onError });   // créé avant l'éditeur : les modules le reçoivent
//   controller.attach(editor)       -> à l'ouverture de l'éditeur ; avant : state null, busy faux. Refuse seulement si un
//                                      éditeur est ENCORE attaché (US-327 : detach() d'abord, puis attach() du suivant)
//   controller.detach()             -> retire l'abonnement à l'éditeur et publie l'état neutre ; le contrôleur reste utilisable
//   controller.subscribe(listener)  -> unsubscribe ; listener({ state, readOnly, plainText, busy }) est appelé tout de
//                                      suite avec l'instantané courant, puis après chaque changement.
//   controller.snapshot()           -> le dernier instantané
//   controller.refresh()            -> Promise : relit FormatState maintenant (regroupé avec une relecture déjà prévue)
//   controller.run(fn)              -> Promise ; fn(formatClient) exécute la commande (applyCharFormat…) ; ensuite
//                                      FormatState est relu (réussie, refusée ou de résultat inconnu) et le focus revient
//                                      au document. La valeur de fn est rendue, son erreur est relancée.
//   controller.client               -> createFormatClient de l'éditeur (une instance par éditeur)
//   controller.dispose()
// Instantané : { state, readOnly, plainText, busy }
//   state     : réponse `formatState` décodée (voir rich-editor-format.mjs), ou null tant que le document n'est pas lu
//   readOnly  : document en lecture seule (révisions, commentaires)
//   plainText : document TXT (aucune mise en forme possible)
//   busy      : l'éditeur est suspendu (dialogue, ouverture, enregistrement) : les commandes doivent être désactivées
//   exemple   : { state: { type: "formatState", revision: 9n, collapsed: true, readOnly: false, partial: false,
//                 typing: false, plain: false, chars: { bold: { state: "value", value: true }, ... }, paragraph: {...} },
//               readOnly: false, plainText: false, busy: false }
// Une commande n'est possible que si `canFormat(snapshot)` (état lu, non occupé, modifiable, pas du texte brut).
//
// Raccourcis : chaque module expose `keyBindings()` -> [{ key | code, ctrl, shift, alt, run(event) }] ; voir
// `createFormatKeyRouter`. Raccourcis déjà pris (ne pas les redéfinir) : RESERVED_SHORTCUTS.

/** Vrai si une commande de mise en forme peut partir. */
export const canFormat = snapshot => Boolean(snapshot && snapshot.state && !snapshot.busy && !snapshot.readOnly && !snapshot.plainText);

const EMPTY = Object.freeze({ state: null, readOnly: false, plainText: false, busy: false });

export function createFormatController({ editor: initial = null, onError = () => {} } = {}) {
  let editor = null;
  let unsubscribeEditor = () => {};
  const listeners = new Set();
  let snap = EMPTY;
  let disposed = false;
  let generation = 0;   // incremented by each change: a reply of an older generation is stale
  let scheduled = false;
  let pendingRefresh = Promise.resolve();

  // readOnly comes from the editor's own state (always current); plainText from the FormatState, which is dropped when a
  // document is opened (reason "document") so that a previous document's value never shows.
  const build = state => Object.freeze({
    state, readOnly: Boolean(editor?.state?.readOnly ?? state?.readOnly ?? false),
    plainText: Boolean(state?.plain ?? false), busy: Boolean(editor?.busy),
  });
  const same = (a, b) => a.state === b.state && a.readOnly === b.readOnly && a.plainText === b.plainText && a.busy === b.busy;
  function publish(next) {
    if (same(snap, next)) return;
    snap = next;
    for (const listener of [...listeners]) { try { listener(snap); } catch (error) { onError(error); } }
  }

  async function query() {
    const mine = generation;
    if (!editor?.state) { publish(build(null)); return; }
    try {
      const state = await editor.format.formatState();
      if (disposed || mine !== generation) return; // stale reply: a newer query follows
      publish(build(state));
    } catch (error) {
      if (disposed || mine !== generation) return;
      if (error?.code !== "CLIENT_CLOSED") onError(error);
      publish(build(null));
    }
  }

  /** Coalesces every change of the same tick into one query. */
  function schedule() {
    if (disposed) return pendingRefresh;
    generation += 1;
    if (scheduled) return pendingRefresh;
    scheduled = true;
    pendingRefresh = Promise.resolve().then(async () => {
      scheduled = false;
      if (disposed) return;
      publish(build(snap.state)); // busy and read-only show at once, without waiting for the reply
      if (!editor || editor.busy) return; // no query while suspended; the end of the suspension is a change
      await query();
    });
    return pendingRefresh;
  }

  const api = {
    attach(target) {
      if (disposed) return pendingRefresh;
      if (editor) throw new TypeError("The controller already has an editor.");
      if (!target || typeof target.subscribe !== "function" || !target.format) throw new TypeError("An editor with subscribe() and format is required.");
      editor = target;
      unsubscribeEditor = editor.subscribe(({ reason } = {}) => {
        if (reason === "document" && snap.state) publish(build(null));
        void schedule();
      });
      return schedule();
    },
    /** US-327 : détache l'éditeur courant (changement d'onglet) ; les réponses en vol deviennent périmées, l'état redevient neutre. */
    detach() {
      if (!editor) return;
      unsubscribeEditor(); unsubscribeEditor = () => {};
      editor = null;
      generation += 1;
      publish(EMPTY);
    },
    get client() { return editor?.format ?? null; },
    snapshot: () => snap,
    subscribe(listener) {
      if (typeof listener !== "function") throw new TypeError("A listener function is required.");
      listeners.add(listener);
      try { listener(snap); } catch (error) { onError(error); }
      return () => { listeners.delete(listener); };
    },
    refresh: () => schedule(),
    async run(fn) {
      try {
        if (!editor) throw new TypeError("No editor is attached.");
        return await fn(editor.format);
      } finally {
        // Applied, refused or unknown outcome: the state is read back (docs §7) and the document gets the focus.
        try { await schedule(); } finally { if (!disposed) { try { editor?.focus(); } catch { /* disposed editor */ } } }
      }
    },
    dispose() {
      if (disposed) return;
      disposed = true; unsubscribeEditor(); listeners.clear();
    },
  };
  if (initial) api.attach(initial);
  return Object.freeze(api);
}

/**
 * Raccourcis déjà utilisés par l'application ou le champ de saisie : un module de mise en forme ne peut pas les
 * prendre (le routeur lève une erreur à l'enregistrement). Ctrl vaut aussi Cmd.
 */
export const RESERVED_SHORTCUTS = Object.freeze([
  "Ctrl+N", "Ctrl+O", "Ctrl+S",                  // app.mjs : nouveau, ouvrir, enregistrer
  "Ctrl+W", "Ctrl+F4", "Ctrl+Tab", "Ctrl+Shift+Tab", // document-tabs.mjs : fermer, onglet suivant, onglet précédent
  "Ctrl+F", "Ctrl+H",                            // navigation-tools.mjs : rechercher, remplacer
  "Ctrl+A", "Ctrl+Z", "Ctrl+Y", "Ctrl+Shift+Z",  // text-input.mjs : tout sélectionner, annuler, rétablir
  "Ctrl+Shift+Space",                            // app.mjs : espace insécable
  "Ctrl+F1",                                     // ribbon.mjs : réduire le ruban
  "Ctrl+C", "Ctrl+X", "Ctrl+V",                  // presse-papiers
  "Ctrl+Home", "Ctrl+End",                       // text-input.mjs ; Ctrl+flèches (par mot) aussi
  "Ctrl+Wheel",                                  // zoom.mjs (molette seulement : Ctrl+= et Ctrl+- sont libres)
  "Alt+Shift+F1", "F8", "Escape", "Alt",         // text-input.mjs (lecteur d'écran, mode extension), ribbon.mjs (astuces)
]);

const id = binding => {
  const name = String(binding.key ?? binding.code ?? "").toLowerCase();
  return `${binding.ctrl ? "ctrl+" : ""}${binding.shift ? "shift+" : ""}${binding.alt ? "alt+" : ""}${name}`;
};
const RESERVED_IDS = new Set(["ctrl+w", "ctrl+f4", "ctrl+tab", "ctrl+shift+tab", "ctrl+n", "ctrl+o", "ctrl+s", "ctrl+f", "ctrl+h", "ctrl+a", "ctrl+z", "ctrl+shift+z", "ctrl+y",
  "ctrl+c", "ctrl+x", "ctrl+v", "ctrl+f1", "ctrl+shift+ ", "ctrl+shift+space", "ctrl+home", "ctrl+end",
  "ctrl+arrowleft", "ctrl+arrowright", "ctrl+arrowup", "ctrl+arrowdown"]);

/**
 * Routeur de raccourcis. Les modules fournissent `keyBindings()` -> [{ key, code, ctrl, shift, alt, run(event) }] :
 *   key   : valeur de `event.key` (insensible à la casse), p. ex. "g", ">", "=" ; ou code : valeur de `event.code`
 *   ctrl  : Ctrl ou Cmd requis (défaut faux) ; alt : exact (défaut faux)
 *   shift : exact (défaut faux) ; `undefined` explicite = Maj ignoré (utile pour ">" ou "+", que Maj produit selon la disposition)
 *   ifNoCharacter : pour Ctrl+Alt (AltGr sous Windows) : reconnu par `code`, exécuté au RELÂCHEMENT de la touche, et seulement si l'appui
 *           n'a tapé aucun caractère (voir le routeur) ; sans cette option, Ctrl+Alt n'est jamais un raccourci. Déclarer `ctrl: true, alt: true` (l'identité du raccourci en dépend).
 *   run   : appelé avec l'événement quand le focus est dans l'éditeur, qu'aucun dialogue modal n'est ouvert et que
 *           `isEnabled()` est vrai ; le routeur fait preventDefault dans tous les cas (même désactivé). `run` doit tester `canFormat(controller.snapshot())`.
 * Deux modules ne peuvent pas prendre la même combinaison, ni une combinaison réservée (erreur à la création).
 */
export function createFormatKeyRouter({ document: doc, target, modules = [], isEnabled = () => true } = {}) {
  const table = [];
  const seen = new Set();
  for (const module of modules) {
    for (const binding of module?.keyBindings?.() ?? []) {
      if (typeof binding.run !== "function" || !(binding.key || binding.code)) throw new TypeError("Invalid key binding.");
      const bindingId = id(binding);
      if (RESERVED_IDS.has(bindingId) || seen.has(bindingId)) throw new TypeError(`Shortcut already taken: ${bindingId}.`);
      seen.add(bindingId); table.push(binding);
    }
  }
  // Ctrl+Alt (AltGr on Windows) is never a shortcut by default: AltGr+2 is « ~ » on an AZERTY keyboard. A binding declared
  // `ifNoCharacter` (Word's Ctrl+Alt+1 to 3) is recognised by `code` at the press, but runs at the RELEASE of the key, and only if the
  // keystroke typed nothing: no `beforeinput`, `keypress`, `input` or composition event came in between (they follow the press and
  // precede the release). The press is not prevented, so a keystroke that does type a character types it.
  let armed = null;
  const onKeyDown = event => {
    if (!table.length || event.isComposing || event.keyCode === 229) return;
    const mine = event.target === target() && !doc.querySelector("dialog:modal");
    if (armed && !(mine && event.repeat && event.code === armed.code)) armed = null; // another key: the chord is over
    if (!mine) return;
    const altGraph = event.getModifierState?.("AltGraph") === true;
    if (altGraph || (event.ctrlKey && event.altKey)) {
      if (event.repeat) return;
      const ctrl = Boolean(event.ctrlKey || event.metaKey);
      const binding = table.find(candidate => candidate.ifNoCharacter && Boolean(candidate.ctrl) === ctrl && Boolean(candidate.shift) === Boolean(event.shiftKey)
        && candidate.code === event.code);
      if (binding) armed = { binding, code: event.code };
      return;
    }
    const ctrl = Boolean(event.ctrlKey || event.metaKey);
    const key = typeof event.key === "string" ? event.key.toLowerCase() : "";
    for (const binding of table) {
      if (binding.ifNoCharacter) continue;
      if (Boolean(binding.ctrl) !== ctrl || Boolean(binding.alt) !== Boolean(event.altKey)) continue;
      if (!("shift" in binding && binding.shift === undefined) && Boolean(binding.shift) !== Boolean(event.shiftKey)) continue;
      if (binding.code ? event.code !== binding.code : key !== String(binding.key).toLowerCase()) continue;
      event.preventDefault(); // the shortcut is ours even when disabled: it must not reach the browser
      if (!isEnabled()) return;
      binding.run(event);
      return;
    }
  };
  const onTyped = () => { armed = null; };
  const onKeyUp = event => {
    if (!armed || event.code !== armed.code) return;
    const { binding } = armed;
    armed = null;
    if (event.target !== target() || doc.querySelector("dialog:modal") || !isEnabled()) return;
    event.preventDefault?.();
    binding.run(event);
  };
  const typedEvents = ["beforeinput", "keypress", "input", "compositionstart"];
  doc.addEventListener("keydown", onKeyDown);
  doc.addEventListener("keyup", onKeyUp);
  for (const type of typedEvents) doc.addEventListener(type, onTyped, true);
  return Object.freeze({
    dispose() {
      doc.removeEventListener("keydown", onKeyDown); doc.removeEventListener("keyup", onKeyUp);
      for (const type of typedEvents) doc.removeEventListener(type, onTyped, true);
    },
    get size() { return table.length; },
  });
}
