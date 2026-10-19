// SPDX-License-Identifier: AGPL-3.0-or-later
// Routeur de raccourcis de la mise en forme (web/format-controller.mjs) : `node tests/format-key-router.mjs`.
import assert from "node:assert/strict";
import { createFormatKeyRouter } from "../web/format-controller.mjs";

const editorElement = {};
let modal = null;
const handlers = new Map();
const doc = {
  addEventListener: (type, fn) => { const list = handlers.get(type) ?? []; list.push(fn); handlers.set(type, list); },
  removeEventListener: (type, fn) => { handlers.set(type, (handlers.get(type) ?? []).filter(other => other !== fn)); },
  querySelector: () => modal,
};
const handler = event => { for (const fn of handlers.get("keydown") ?? []) fn(event); };
const fire = (type, event = {}) => { for (const fn of handlers.get(type) ?? []) fn(event); };
let ran = []; let prevented = 0; let enabled = true;
const module = { keyBindings: () => [
  { key: "g", ctrl: true, run: () => ran.push("ctrl+g") },
  { key: ">", ctrl: true, shift: undefined, run: () => ran.push("ctrl+>") },
  { key: "u", ctrl: true, run: () => ran.push("ctrl+u") },
  { code: "Digit1", ctrl: true, alt: true, ifNoCharacter: true, run: () => ran.push("ctrl+alt+1") },
  { code: "Digit2", ctrl: true, alt: true, ifNoCharacter: true, run: () => ran.push("ctrl+alt+2") },
] };
const router = createFormatKeyRouter({ document: doc, target: () => editorElement, modules: [module], isEnabled: () => enabled });
const press = (init, target = editorElement) => {
  ran = []; prevented = 0;
  handler({ target, key: "", preventDefault() { prevented += 1; }, getModifierState: name => name === "AltGraph" && init.altGraph === true, ...init });
  return { ran: [...ran], prevented };
};

assert.deepEqual(press({ key: "G", ctrlKey: true }), { ran: ["ctrl+g"], prevented: 1 }, "Ctrl+G (case-insensitive)");
assert.deepEqual(press({ key: "g", metaKey: true }), { ran: ["ctrl+g"], prevented: 1 }, "Cmd counts as Ctrl");
assert.deepEqual(press({ key: ">", ctrlKey: true, shiftKey: true }), { ran: ["ctrl+>"], prevented: 1 }, "shift ignored when undefined");
assert.deepEqual(press({ key: ">", ctrlKey: true }), { ran: ["ctrl+>"], prevented: 1 }, "shift ignored (without Shift too)");
assert.deepEqual(press({ key: "g", ctrlKey: true, shiftKey: true }), { ran: [], prevented: 0 }, "Shift is exact by default");
assert.deepEqual(press({ key: "g" }), { ran: [], prevented: 0 }, "no Ctrl: not ours");
assert.deepEqual(press({ key: "g", ctrlKey: true, altKey: true }), { ran: [], prevented: 0 }, "Ctrl+Alt (AltGr on Windows) is never a shortcut");
assert.deepEqual(press({ key: "1", ctrlKey: true, altKey: true, code: "Digit1" }), { ran: [], prevented: 0 }, "Ctrl+Alt+1: nothing at the press");
assert.deepEqual(press({ key: "g", ctrlKey: true, altGraph: true }), { ran: [], prevented: 0 }, "AltGraph modifier state");
assert.deepEqual(press({ key: "g", ctrlKey: true, isComposing: true }), { ran: [], prevented: 0 }, "composition (IME)");
assert.deepEqual(press({ key: "Process", keyCode: 229, ctrlKey: true }), { ran: [], prevented: 0 }, "keyCode 229 (IME)");
assert.deepEqual(press({ key: "g", ctrlKey: true }, {}), { ran: [], prevented: 0 }, "target outside the editor");
modal = {};
assert.deepEqual(press({ key: "g", ctrlKey: true }), { ran: [], prevented: 0 }, "modal dialog open");
modal = null;
enabled = false;
assert.deepEqual(press({ key: "u", ctrlKey: true }), { ran: [], prevented: 1 }, "disabled: preventDefault, no effect");
enabled = true;

const make = bindings => createFormatKeyRouter({ document: doc, target: () => editorElement, modules: [{ keyBindings: () => bindings }] });
assert.throws(() => make([{ key: "g", ctrl: true, run() {} }, { key: "G", ctrl: true, run() {} }]), /already taken/, "duplicate in one module");
assert.throws(() => createFormatKeyRouter({ document: doc, target: () => editorElement, modules: [module, module] }), /already taken/, "duplicate across modules");
for (const key of ["n", "o", "s", "f", "h", "a", "z", "y"]) assert.throws(() => make([{ key, ctrl: true, run() {} }]), /already taken/, `reserved Ctrl+${key}`);
assert.throws(() => make([{ key: "z", ctrl: true, shift: true, run() {} }]), /already taken/, "reserved Ctrl+Shift+Z");
assert.throws(() => make([{ key: "g", ctrl: true }]), /Invalid/, "binding without run");

// ---- Ctrl+Alt+digit (Word's Titre 1 to 3): never steals a typed character, AZERTY included ----------------------------------------
const down = (init, target = editorElement) => { prevented = 0; handler({ target, key: "", preventDefault() { prevented += 1; }, getModifierState: name => name === "AltGraph" && init.altGraph === true, ...init }); return prevented; };
const up = (init, target = editorElement) => { ran = []; fire("keyup", { target, key: "", preventDefault() {}, ...init }); return [...ran]; };
const chord = { ctrlKey: true, altKey: true };
// QWERTY: Ctrl+Alt+2 types nothing: the heading is applied when the key is released, the press is not prevented.
assert.equal(down({ ...chord, code: "Digit2", key: "2" }), 0, "QWERTY press: not prevented");
assert.deepEqual(up({ code: "Digit2", key: "2" }), ["ctrl+alt+2"], "QWERTY: runs at the release");
assert.deepEqual(up({ code: "Digit2", key: "2" }), [], "a release without press does nothing");
// AZERTY: AltGr+é is « ~ » (Windows reports Ctrl+Alt and AltGraph): the character arrives as `beforeinput`, nothing runs.
down({ ...chord, altGraph: true, code: "Digit2", key: "~" });
fire("keypress", { target: editorElement }); fire("beforeinput", { target: editorElement, data: "~" });
assert.deepEqual(up({ code: "Digit2", key: "~" }), [], "AZERTY AltGr+2 types « ~ »: no heading");
// Some layouts only give `keypress` or a composition: same.
for (const type of ["keypress", "compositionstart", "input"]) {
  down({ ...chord, code: "Digit1", key: "&" });
  fire(type, { target: editorElement });
  assert.deepEqual(up({ code: "Digit1", key: "&" }), [], `a ${type} event cancels the chord`);
}
// AZERTY Ctrl+Alt+& : AltGr+& types nothing: the heading is applied, with `key` the base character.
down({ ...chord, altGraph: true, code: "Digit1", key: "&" });
assert.deepEqual(up({ code: "Digit1", key: "&" }), ["ctrl+alt+1"], "AZERTY Ctrl+Alt+& types nothing: Titre 1");
// Key repeat: one run.
down({ ...chord, code: "Digit1", key: "1" }); down({ ...chord, code: "Digit1", key: "1", repeat: true }); down({ ...chord, code: "Digit1", key: "1", repeat: true });
assert.deepEqual(up({ code: "Digit1" }), ["ctrl+alt+1"], "repeat: a single run");
// Another key between press and release, a modifier change, another target, a modal, a disabled editor, extra Shift: nothing.
down({ ...chord, code: "Digit1", key: "1" }); down({ ctrlKey: true, key: "g" }); assert.deepEqual(up({ code: "Digit1" }), [], "another key ends the chord");
down({ ...chord, code: "Digit1", key: "1" }); assert.deepEqual(up({ code: "Digit1" }, {}), [], "release outside the editor");
down({ ...chord, code: "Digit1", key: "1" }, {}); assert.deepEqual(up({ code: "Digit1" }), [], "press outside the editor");
modal = {}; down({ ...chord, code: "Digit1", key: "1" }); assert.deepEqual(up({ code: "Digit1" }), [], "modal dialog"); modal = null;
enabled = false; down({ ...chord, code: "Digit1", key: "1" }); assert.deepEqual(up({ code: "Digit1" }), [], "disabled"); enabled = true;
down({ ...chord, shiftKey: true, code: "Digit1", key: "!" }); assert.deepEqual(up({ code: "Digit1" }), [], "Shift is exact");
down({ ...chord, code: "Digit4", key: "4" }); assert.deepEqual(up({ code: "Digit4" }), [], "unbound digit");
down({ ctrlKey: true, code: "Digit1", key: "1" }); assert.deepEqual(up({ code: "Digit1" }), [], "plain Ctrl+1 is not the chord");
down({ ...chord, key: "g", code: "KeyG" }); assert.deepEqual(up({ code: "KeyG" }), [], "Ctrl+Alt+G: not bound, never a shortcut");
router.dispose();
for (const type of ["keydown", "keyup", "beforeinput", "keypress", "input", "compositionstart"]) assert.equal((handlers.get(type) ?? []).length, 0, `dispose removes ${type}`);
console.log("ok - format key router");
