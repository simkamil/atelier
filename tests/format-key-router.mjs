// SPDX-License-Identifier: AGPL-3.0-or-later
// Routeur de raccourcis de la mise en forme (web/format-controller.mjs) : `node tests/format-key-router.mjs`.
import assert from "node:assert/strict";
import { createFormatKeyRouter } from "../web/format-controller.mjs";

const editorElement = {};
let handler = null; let modal = null;
const doc = { addEventListener: (_, fn) => { handler = fn; }, removeEventListener() { handler = null; }, querySelector: () => modal };
let ran = []; let prevented = 0; let enabled = true;
const module = { keyBindings: () => [
  { key: "g", ctrl: true, run: () => ran.push("ctrl+g") },
  { key: ">", ctrl: true, shift: undefined, run: () => ran.push("ctrl+>") },
  { key: "u", ctrl: true, run: () => ran.push("ctrl+u") },
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
router.dispose();
assert.equal(handler, null, "dispose removes the listener");
console.log("ok - format key router");
