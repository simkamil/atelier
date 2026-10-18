// SPDX-License-Identifier: AGPL-3.0-or-later
// US-209 : miroir DOM accessible dans Edge/Chromium (structure, rôles, alt, sélection DOM, focus, contraste forcé).
import { createDocumentMirror, createReaderModeToggle } from "/document-mirror.mjs";
import { attachTextInput } from "/text-input.mjs";
import { createI18n } from "/i18n.mjs";

const checks = [];
const assert = (value, name) => { if (!value) throw new Error(name); checks.push(name); };
const forced = new URLSearchParams(location.search).get("forced") === "1";
const enc = new TextEncoder();
const item = (start, text, extra = {}) => ({ paraStart: BigInt(start), paraEnd: BigInt(start + enc.encode(text).length), role: 0, level: 0, label: "", text, inlines: [], ...extra });
// Paragraph starts follow the flat text: one newline byte between paragraphs.
const specs = [
  ["Titre un", { role: 1, level: 1 }], ["Sous-titre", { role: 1, level: 3 }], ["Très profond", { role: 1, level: 8 }],
  ["Premier", { role: 2, level: 0, label: "1." }], ["Second niveau", { role: 2, level: 1, label: "•" }],
  ["Voir ￼ et ￼.", { inlines: [{ offset: 5n, kind: 0, value: "Un chat noir" }, { offset: 12n, kind: 0, value: "" }] }], ["Café crème", {}],
];
const starts = [];
for (let at = 0, i = 0; i < specs.length; i += 1) { starts.push(at); at += enc.encode(specs[i][0]).length + 1; }
const baseDoc = (replace = {}) => specs.map(([text, extra], i) => item(starts[i], replace[i] ?? text, extra));
const [S_SECOND, S_FIRST, S_LAST] = [starts[4], starts[3], starts[6]];

async function main() {
  const catalog = await (await fetch("/i18n/fr-FR.txt")).text();
  const i18n = createI18n({ locale: "fr-FR", catalog });
  const t = (key, values) => i18n.t(key, values);
  const host = document.getElementById("host");
  let selection = { anchor: 0n, focus: 0n };
  const commands = [], phases = [];
  let mirror;
  const f1 = extra => new KeyboardEvent("keydown", { key: "F1", altKey: true, shiftKey: true, bubbles: true, cancelable: true, ...extra });
  const input = attachTextInput({
    container: host, getSelection: () => selection, getCaret: () => ({ x: 10, y: 10, width: 1, height: 12 }),
    onCommand: command => commands.push(command), onComposition: event => phases.push(event.phase),
    getClipboardSnapshot: () => ({ text: "Café crème", revision: 1n, selection: { anchor: 0, focus: 5 } }),
  });
  input.focus();
  assert(document.activeElement === input.element, "the input has the focus before the mirror exists");

  let revision = 1n;
  let current = baseDoc();
  const calls = [];
  mirror = createDocumentMirror({
    container: host, t, input, announceDelay: 0, activeDescendantTarget: input.element,
    getAccessibleWindow: async (start, end, { signal }) => {
      calls.push([start, end]);
      await new Promise(resolve => setTimeout(resolve, 5));
      if (signal.aborted) throw new DOMException("aborted", "AbortError");
      return { revision, items: current };
    },
  });
  const root = mirror.element;
  const push = () => mirror.update({ revision, docLen: 200n, selection, visible: { start: 0n, end: 120n } });
  assert((await push()).status === "applied", "first window applied");

  // Structure, roles, levels, alt.
  assert(root.getAttribute("role") === "document" && root.getAttribute("aria-label") === "Contenu du document", "root is a labelled document");
  assert(root.querySelector("h1")?.textContent === "Titre un", "level 1 heading is h1");
  assert(root.querySelector("h3")?.textContent === "Sous-titre", "level 3 heading is h3");
  const deep = root.querySelector("[role=heading]");
  assert(deep?.getAttribute("aria-level") === "8" && deep.tagName === "DIV", "level 8 heading uses role=heading and aria-level");
  const list = root.querySelector("[role=list]");
  assert(list && list.children.length === 2, "consecutive list items share one list");
  assert([...list.children].map(n => n.getAttribute("role") + n.getAttribute("aria-level") + n.textContent).join("|") === "listitem11. Premier|listitem2• Second niveau", "list items carry label and level");
  const images = [...root.querySelectorAll("[role=img]")];
  assert(images.length === 2 && images[0].getAttribute("aria-label") === "Un chat noir", "image alt text is exposed");
  assert(images[1].getAttribute("aria-label") === "Image sans texte alternatif", "image without alt is announced as such");
  assert(root.querySelectorAll("script, img").length === 0, "no script or img element");

  // Out of sight but readable by assistive technology.
  const rect = root.getBoundingClientRect();
  const style = getComputedStyle(root);
  assert(rect.width <= 1 && rect.height <= 1, "mirror occupies at most 1x1 px");
  assert(style.display !== "none" && style.visibility !== "hidden" && root.getAttribute("aria-hidden") === null && !root.hidden, "mirror is not hidden from the accessibility tree");
  assert(style.clipPath.startsWith("inset"), "mirror is clipped out of sight");
  assert(!root.querySelector("[tabindex]") && !root.hasAttribute("tabindex"), "mirror takes no tab stop");
  assert(forced === matchMedia("(forced-colors: active)").matches, forced ? "forced-colors is active in this run" : "forced-colors is inactive in the normal run");
  assert(style.borderTopWidth === "0px" && style.outlineWidth === "0px" || style.outlineStyle === "none", "no system border or outline on the mirror");
  assert(getComputedStyle(root.querySelector("p")).borderTopWidth === "0px", "no system border on mirror paragraphs");

  // Selection follows the model; focus stays in the input.
  selection = { anchor: BigInt(S_SECOND), focus: BigInt(S_SECOND + 7) };
  await push();
  const dom = getSelection();
  assert(dom.toString().trim() === "Second", "DOM selection selects the model range (UTF-8 offsets)");
  assert(document.activeElement === input.element, "setting the DOM selection does not steal the focus");
  selection = { anchor: BigInt(S_SECOND), focus: BigInt(S_FIRST) };
  await push();
  assert(dom.focusNode.parentNode.closest("[role=listitem]").textContent.startsWith("1."), "reversed selection: the focus end lands in the first list item");
  assert(dom.anchorNode.parentNode.closest("[role=listitem]").textContent.startsWith("•"), "reversed selection: the anchor end stays in the second item");
  assert(document.activeElement === input.element, "focus kept after a reversed selection");
  const accent = BigInt(S_LAST) + BigInt(enc.encode("Caf").length);
  selection = { anchor: accent, focus: accent + 2n };
  await push();
  assert(dom.toString() === "é", "multi-byte character selected by its UTF-8 range");
  selection = { anchor: BigInt(S_LAST), focus: BigInt(S_LAST) };
  await push();
  assert(dom.isCollapsed && dom.anchorOffset === 0, "caret is a collapsed DOM selection");
  const last = [...root.querySelectorAll("p")].at(-1);
  assert(input.element.getAttribute("aria-activedescendant") === last.id, "aria-activedescendant names the current paragraph");
  const live = root.querySelector("[role=status]");
  assert(live.getAttribute("aria-live") === "polite" && live.textContent === "Sélection annulée", "live region announces that the selection was cleared");
  selection = { anchor: 0n, focus: 0n };
  await push();
  assert(live.textContent === "Titre de niveau 1 : Titre un", "live region announces headings with their level");
  selection = { anchor: 0n, focus: 5n };
  await push();
  assert(live.textContent === "Sélection : Titre", "live region announces the selected text");

  // The user can still type: the input keeps the focus and its content.
  input.element.value = "x";
  assert(document.activeElement === input.element && input.element.value === "x", "the input still holds focus and text");
  input.element.value = "";

  // Incremental update: only the changed paragraph is replaced, unchanged nodes are kept.
  const before = [...root.querySelectorAll("p")];
  const callsBefore = calls.length;
  revision = 2n;
  current = baseDoc({ 6: "Café crème brûlée" });
  assert((await push()).status === "applied" && calls.length === callsBefore + 1, "a new revision triggers one request");
  const after = [...root.querySelectorAll("p")];
  assert(after.length === 2 && after[0] === before[0] && after[1] === before[1] && after.at(-1).textContent === "Café crème brûlée", "the edited paragraph is updated in place (same node and id), the others are untouched");
  assert(document.activeElement === input.element, "focus kept after a content update");

  // A superseded request is cancelled and never applied.
  let release;
  const gate = new Promise(resolve => { release = resolve; });
  const slow = createDocumentMirror({ container: host, t, getAccessibleWindow: async (start, end, { signal }) => { await gate; if (signal.aborted) throw new DOMException("aborted", "AbortError"); return { revision: 5n, items: [item(0, "périmé")] }; } });
  const old = slow.update({ revision: 5n, docLen: 10n, selection: { anchor: 0n, focus: 0n } });
  const newer = slow.update({ revision: 6n, docLen: 10n, selection: { anchor: 0n, focus: 0n } });
  release();
  assert((await old).status === "stale" && slow.element.querySelector("p") === null && (await newer).status === "stale", "stale revision never reaches the DOM");
  slow.dispose();

  // Screen reader mode (Alt+Shift+F1, or the toggle button): the focused input holds the paragraph and the selection.
  assert(input.element.value === "" && !mirror.screenReaderMode(), "reader mode is off by default and the input is empty");
  selection = { anchor: BigInt(S_LAST + 1), focus: BigInt(S_LAST + 3) };
  await push();
  const toggle = createReaderModeToggle({ container: host, input, t });
  assert(toggle.element.getAttribute("aria-pressed") === "false" && toggle.element.textContent === "Mode lecteur d’écran" && toggle.element.title.includes("Alt+Maj+F1"), "the toggle button is labelled and shows the shortcut");
  const altGr = f1({ ctrlKey: true, modifierAltGraph: true });
  input.element.dispatchEvent(altGr);
  input.element.dispatchEvent(new KeyboardEvent("keydown", { key: "z", ctrlKey: true, altKey: true, bubbles: true, cancelable: true }));
  assert(!mirror.screenReaderMode() && !altGr.defaultPrevented, "Ctrl+Alt+Z and AltGr combinations do not toggle the mode");
  input.element.dispatchEvent(f1());
  assert(mirror.screenReaderMode() && input.screenReaderMode() && toggle.element.getAttribute("aria-pressed") === "true", "Alt+Shift+F1 turns the mode on in the input, the mirror and the button");
  assert(live.textContent === "Mode lecteur d’écran activé", "the mode change is announced");
  await push();
  const ta = input.element;
  assert(ta.value === "Café crème brûlée" && ta.selectionStart === 1 && ta.selectionEnd === 3 && ta.selectionDirection === "forward", "the input holds the paragraph with the selection");
  assert(document.activeElement === ta, "focus stays in the input in reader mode");
  selection = { anchor: BigInt(S_LAST + 5), focus: BigInt(S_LAST) };
  await push();
  assert(ta.selectionStart === 0 && ta.selectionEnd === 4 && ta.selectionDirection === "backward", "a reversed selection is reflected with its direction");
  selection = { anchor: BigInt(S_LAST + 5), focus: BigInt(S_LAST + 5) };
  await push();
  assert(ta.selectionStart === 4 && ta.selectionEnd === 4, "the caret is a collapsed selection in UTF-16 units");
  // Typing is still routed to the editor, not applied to the input.
  const typed = new InputEvent("beforeinput", { inputType: "insertText", data: "x", bubbles: true, cancelable: true });
  ta.dispatchEvent(typed);
  assert(typed.defaultPrevented && commands.at(-1).type === "insertText" && commands.at(-1).text === "x" && ta.value === "Café crème brûlée", "typing goes to the editor and leaves the paragraph text in place");
  // Copy and cut use the editor snapshot, not the paragraph in the input.
  const copy = new DataTransfer();
  const copyEvent = new ClipboardEvent("copy", { clipboardData: copy, bubbles: true, cancelable: true });
  ta.dispatchEvent(copyEvent);
  assert(copyEvent.defaultPrevented && copy.getData("text/plain") === "Café", "copy puts the editor selection on the clipboard");
  const cut = new DataTransfer();
  ta.dispatchEvent(new ClipboardEvent("cut", { clipboardData: cut, bubbles: true, cancelable: true }));
  assert(cut.getData("text/plain") === "Café" && commands.some(c => c.type === "deleteSelection"), "cut copies the selection and asks the editor to delete it");
  // IME composition: transactions as usual, the paragraph is restored afterwards.
  phases.length = 0;
  ta.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  ta.dispatchEvent(new CompositionEvent("compositionupdate", { data: "e", bubbles: true }));
  await push();
  ta.dispatchEvent(new CompositionEvent("compositionend", { data: "é", bubbles: true }));
  assert(phases.join() === "start,update,commit" && commands.at(-1).text === "é" && commands.at(-1).inputType === "insertCompositionText", "composition start, update and commit reach the editor");
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(ta.value === "Café crème brûlée" && document.activeElement === ta, "the paragraph is back in the input after the composition");
  phases.length = 0;
  ta.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  ta.dispatchEvent(new CompositionEvent("compositionend", { data: "", bubbles: true }));
  assert(phases.join() === "start,cancel", "a cancelled composition inserts nothing");
  // Browser-managed text that nobody handles is replaced by the paragraph again.
  ta.value = "zz";
  ta.dispatchEvent(new InputEvent("input", { inputType: "formatBold", bubbles: true }));
  await new Promise(resolve => setTimeout(resolve, 30));
  assert(ta.value === "Café crème brûlée", "unhandled native edits are undone in reader mode");
  // The button toggles the same state, keeps the focus in the input, and the mirror announces it.
  toggle.element.click();
  assert(!mirror.screenReaderMode() && ta.value === "" && !ta.hasAttribute("aria-multiline") && toggle.element.getAttribute("aria-pressed") === "false", "the button turns the mode off and empties the input");
  assert(live.textContent === "Mode lecteur d’écran désactivé" && document.activeElement === ta, "leaving the mode is announced and the focus is back in the input");
  toggle.element.click();
  assert(mirror.screenReaderMode() && live.textContent === "Mode lecteur d’écran activé", "the button turns it on again");
  ta.dispatchEvent(f1());
  assert(!mirror.screenReaderMode() && toggle.element.getAttribute("aria-pressed") === "false", "the shortcut turns it off: button and mirror follow");
  toggle.dispose();

  mirror.dispose();
  assert(!root.isConnected && !input.element.hasAttribute("aria-activedescendant"), "dispose removes the mirror and the reference");
  input.dispose();
}

let report;
try { await main(); report = { status: "passed", forced, checks }; } catch (error) { report = { status: "failed", forced, checks, message: String(error?.message ?? error), stack: String(error?.stack ?? "") }; }
document.getElementById("result").textContent = JSON.stringify(report);
await fetch("/__mirror_result", { method: "POST", body: JSON.stringify(report) });
