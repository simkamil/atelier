// SPDX-License-Identifier: AGPL-3.0-or-later
import { attachTextInput } from "../web/text-input.mjs";

function check(value, message) {
  if (!value) throw new Error(message);
}

function clipboardEvent(type, { text = "", fail = false } = {}) {
  const written = [];
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: {
    setData(format, value) {
      if (fail) throw new Error("clipboard denied");
      written.push([format, value]);
    },
    getData: format => format === "text/plain" ? text : "",
    types: ["text/plain", "text/html"],
  } });
  return { event, written };
}

function keyEvent(key, modifiers = {}) {
  return new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...modifiers });
}

/** Run focused clipboard and semantic shortcut checks in a real browser document. */
export async function runTextInputEditingChecks() {
  const passed = [];
  const run = (name, scenario) => { scenario(); passed.push(name); };

  run("copy and reversed UTF-8 selection write only the selected plain text", () => {
    const commands = [];
    const text = "avant é🙂 après";
    const bytes = new TextEncoder().encode(text);
    const start = new TextEncoder().encode("avant ").length;
    const end = bytes.length - new TextEncoder().encode(" après").length;
    const adapter = attachTextInput({
      document,
      getSelection: () => ({ anchor: end, focus: start }),
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      getClipboardSnapshot: () => ({ text, selection: { anchor: end, focus: start }, revision: 8n }),
      onCommand: command => commands.push(command),
    });
    const { event, written } = clipboardEvent("copy");
    adapter.element.dispatchEvent(event);
    check(event.defaultPrevented && JSON.stringify(written) === JSON.stringify([["text/plain", "é🙂"]]), "copy writes the exact UTF-8 selection as text/plain");
    check(commands.length === 0, "copy does not mutate the model");
    adapter.dispose();
    const afterDispose = clipboardEvent("copy");
    adapter.element.dispatchEvent(afterDispose.event);
    check(afterDispose.written.length === 0 && !afterDispose.event.defaultPrevented, "dispose removes the clipboard listener");
  });

  run("copy preserves a leading U+FEFF content character", () => {
    const adapter = attachTextInput({ document,
      getSelection: () => ({ anchor: 0, focus: 4 }),
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      getClipboardSnapshot: () => ({ text: "\uFEFFx", selection: { anchor: 0, focus: 4 }, revision: 1n }),
      onCommand: () => {},
    });
    const { event, written } = clipboardEvent("copy");
    adapter.element.dispatchEvent(event);
    check(written.length === 1 && written[0][1] === "\uFEFFx", "document content BOM must not be stripped by clipboard decoding");
    adapter.dispose();
  });

  run("cut writes before emitting one selection-and-revision delete command", () => {
    const sequence = [];
    const selection = { anchor: 1, focus: 6 };
    const adapter = attachTextInput({
      document,
      getSelection: () => selection,
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      getClipboardSnapshot: () => ({ text: "café!", selection, revision: 21n }),
      onCommand: command => sequence.push(["command", command]),
    });
    const { event, written } = clipboardEvent("cut");
    const setData = event.clipboardData.setData;
    event.clipboardData.setData = (format, value) => { setData(format, value); sequence.push(["write", format, value]); };
    adapter.element.dispatchEvent(event);
    check(event.defaultPrevented && JSON.stringify(written) === JSON.stringify([["text/plain", "afé!"]]), "cut writes selected text synchronously");
    check(sequence.map(entry => entry[0]).join() === "write,command", "delete command is emitted only after clipboard write");
    const command = sequence[1][1];
    check(command.type === "deleteSelection" && command.revision === 21n
      && command.selection.anchor === 1 && command.selection.focus === 6, "cut carries the original range and revision");
    adapter.dispose();
  });

  run("failed, empty, stale-boundary, and unavailable snapshots preserve native clipboard behavior", () => {
    const commands = [];
    let snapshot = { text: "é", selection: { anchor: 0, focus: 2 }, revision: 1n };
    const adapter = attachTextInput({
      document,
      getSelection: () => ({ anchor: 0, focus: 2 }),
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      getClipboardSnapshot: () => snapshot,
      onCommand: command => commands.push(command),
    });
    adapter.element.value = "dernier texte natif";
    const failed = clipboardEvent("cut", { fail: true });
    adapter.element.dispatchEvent(failed.event);
    check(failed.event.defaultPrevented && commands.length === 0, "setData failure blocks stale textarea contents and does not delete document content");
    for (const value of [
      { text: "", selection: { anchor: 0, focus: 0 }, revision: 2n },
      { text: "é", selection: { anchor: 1, focus: 2 }, revision: 3n },
      null,
    ]) {
      snapshot = value;
      const empty = clipboardEvent("copy");
      adapter.element.dispatchEvent(empty.event);
      check(empty.event.defaultPrevented && empty.written.length === 0, "empty, invalid-boundary, or unavailable snapshot blocks transient textarea contents");
    }
    adapter.dispose();

    const unavailable = attachTextInput({
      document,
      getSelection: () => ({ anchor: 0, focus: 0 }),
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      onCommand: () => {},
    });
    const copy = clipboardEvent("copy");
    unavailable.element.dispatchEvent(copy.event);
    check(!copy.event.defaultPrevented && copy.written.length === 0, "missing snapshot callback is a no-op");
    unavailable.dispose();

    const noSnapshot = attachTextInput({
      document,
      getSelection: () => ({ anchor: 0, focus: 0 }),
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      getClipboardSnapshot: () => null,
      onCommand: () => {},
    });
    noSnapshot.element.value = "dernier texte natif";
    const protectedCopy = clipboardEvent("copy");
    noSnapshot.element.dispatchEvent(protectedCopy.event);
    check(protectedCopy.event.defaultPrevented && protectedCopy.written.length === 0,
      "configured but unavailable snapshots prevent copying stale textarea text");
    noSnapshot.dispose();
  });

  run("clipboard commands pause during composition and shortcuts use semantic commands", () => {
    const commands = [];
    const adapter = attachTextInput({
      document,
      getSelection: () => ({ anchor: 0, focus: 1 }),
      getCaret: () => ({ x: 0, y: 0, width: 1, height: 1 }),
      getClipboardSnapshot: () => ({ text: "x", selection: { anchor: 0, focus: 1 }, revision: 1n }),
      onCommand: command => commands.push(command),
    });
    adapter.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
    const duringComposition = clipboardEvent("cut");
    adapter.element.dispatchEvent(duringComposition.event);
    check(duringComposition.event.defaultPrevented && duringComposition.written.length === 0
      && commands.length === 0, "clipboard is protected and no cut occurs during composition");
    adapter.element.dispatchEvent(new CompositionEvent("compositionend", { data: "", bubbles: true }));

    for (const [key, modifiers, expected] of [
      ["a", { ctrlKey: true }, { type: "selectAll" }],
      ["F8", {}, { type: "extensionMode", enabled: true }],
      ["Escape", {}, { type: "extensionMode", enabled: false }],
    ]) {
      const event = keyEvent(key, modifiers);
      adapter.element.dispatchEvent(event);
      check(event.defaultPrevented, `${key} is routed to the editor`);
      const actual = commands.at(-1);
      check(actual.type === expected.type && actual.enabled === expected.enabled, `${key} emits ${expected.type}`);
    }
    const copyShortcut = keyEvent("c", { ctrlKey: true });
    const cutShortcut = keyEvent("x", { ctrlKey: true });
    const plainPasteShortcut = keyEvent("v", { ctrlKey: true, shiftKey: true });
    for (const event of [copyShortcut, cutShortcut, plainPasteShortcut]) adapter.element.dispatchEvent(event);
    check([copyShortcut, cutShortcut, plainPasteShortcut].every(event => !event.defaultPrevented), "copy, cut, and plain-text paste retain native clipboard events");

    adapter.element.disabled = true;
    const count = commands.length;
    const disabledKey = keyEvent("a", { ctrlKey: true });
    adapter.element.dispatchEvent(disabledKey);
    const disabledPaste = clipboardEvent("paste", { text: "ignored" });
    adapter.element.dispatchEvent(disabledPaste.event);
    check(!disabledKey.defaultPrevented && !disabledPaste.event.defaultPrevented
      && commands.length === count, "disabled input ignores keyboard and paste events");
    adapter.dispose();
  });

  return passed;
}
