// SPDX-License-Identifier: AGPL-3.0-or-later
// R7b-2 : recherche, remplacement, statistiques et propriétés de l'éditeur v2, Worker et WASM réels, au niveau de l'API publique
// de createRichEditor (les mêmes méthodes que createEditor). Reprend les vérifications de replacement-browser.mjs et des suites
// de navigation de v1 : options, plafond de 1000 résultats, un seul pas d'annulation pour « Tout remplacer », remplacement
// identique ou sans occurrence sans effet, surlignage de toutes les occurrences visibles (pixels du Canvas), propriétés.
const encoder = new TextEncoder();

/** Pixels du Canvas teintés comme le surlignage (jaune) ou comme l'occurrence courante (orange). */
function tints(canvas) {
  const context = canvas.getContext("2d");
  const { data } = context.getImageData(0, 0, canvas.width, canvas.height);
  let yellow = 0; let orange = 0;
  for (let i = 0; i < data.length; i += 4) {
    const [r, g, b] = [data[i], data[i + 1], data[i + 2]];
    if (r >= 250 && g >= 225 && g <= 245 && b >= 165 && b <= 195) yellow += 1;
    else if (r >= 240 && r <= 255 && g >= 195 && g <= 212 && b >= 150 && b <= 175) orange += 1;
  }
  return { yellow, orange };
}

export async function run({ assert, waitFor, editor, openText, text, canvas }) {
  const rejects = async (promise, code, name) => {
    try { await promise; } catch (error) { assert(error?.code === code, `${name}: ${code} (got ${error?.code})`); return; }
    throw new Error(`${name}: expected ${code}`);
  };

  // ---- search options, results, excerpts ------------------------------------------------------------------------------------------
  await openText("Hello hello HELLO hellos\nrésumé resume\na b a b");
  const counted = async (query, options) => (await editor.search(query, options)).items.length;
  const first = await editor.search("hello");
  assert(first.kind === "search" && first.revision === editor.state.revision && first.items.length === 4 && !first.truncated && first.total === 4,
    "search answers the revision, four occurrences and their count");
  assert(first.items.every(i => Number.isInteger(i.start) && Number.isInteger(i.end) && i.end > i.start)
      && first.items[0].start === 0 && first.items[0].end === 5 && first.items[1].start === 6,
    "occurrences are flat byte positions (numbers) in reading order");
  const item = first.items[1];
  const bytes = encoder.encode(item.excerpt.text);
  assert(new TextDecoder().decode(bytes.subarray(item.excerpt.start, item.excerpt.end)) === "hello" && item.excerpt.truncatedBefore === false,
    "the excerpt carries the match as UTF-8 byte offsets");
  assert(await counted("hello", { matchCase: true }) === 2 && await counted("hello", { wholeWord: true }) === 3,
    "case and whole-word options");
  assert(await counted("resume") === 1 && await counted("resume", { ignoreAccents: true }) === 2, "accent option");
  assert(await counted("a b") === 2 && await counted("a b", { matchNbsp: true }) === 1, "no-break space option");
  assert(await counted("") === 0 && await counted("absent") === 0, "an empty query and a missing word give no occurrence");
  await rejects(editor.search("x", { fuzzy: true }), "EDITOR_UNSUPPORTED_INPUT", "unknown option");
  await rejects(editor.search("x".repeat(1025)), "MESSAGE_TOO_LARGE", "query over 1024 bytes");

  const many = "a ".repeat(1105);
  await openText(many);
  const capped = await editor.search("a");
  assert(capped.items.length === 1000 && capped.truncated && capped.total === 1105 && !capped.totalCapped,
    "the result list keeps its 1000-item bound and reports the exact total");

  // ---- selecting a result, stale results --------------------------------------------------------------------------------------------
  await openText("one two\nthree two\nfour");
  const result = await editor.search("two");
  assert(result.items.length === 2, "two occurrences across two paragraphs");
  const selected = await editor.selectSearchResult(result, 1, {});
  assert(selected.anchor === result.items[1].start && selected.focus === result.items[1].end, "selectSearchResult answers the new state");
  assert(editor.state.anchor === result.items[1].start && editor.state.focus === result.items[1].end, "the selection is the occurrence");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: "Z", bubbles: true, cancelable: true }));
  await waitFor(async () => (await text()).includes("Z"), "typing over the selected occurrence");
  await rejects(editor.selectSearchResult(result, 0, {}), "EDITOR_STALE_QUERY", "a result of an older revision");
  assert(editor.setSearchHighlights(result, 0) === false, "highlights of an older revision are refused");
  await rejects(editor.replaceAll({ revision: result.revision, query: "two", replacement: "2", options: {} }), "EDITOR_STALE_QUERY", "replace with an older revision");
  await editor.undo();
  await waitFor(async () => await text() === "one two\nthree two\nfour", "undo restores the text");

  // ---- highlights of the visible occurrences ----------------------------------------------------------------------------------------
  const lines = Array.from({ length: 6 }, (_, i) => `ligne ${i} cible`).join("\n");
  await openText(lines);
  const targets = await editor.search("cible");
  assert(targets.items.length === 6, "six occurrences to highlight");
  const before = tints(canvas);
  assert(before.yellow === 0 && before.orange === 0, "no highlight before a search is shown");
  assert(editor.setSearchHighlights(targets, 2) === true, "highlights of the current revision are accepted");
  await waitFor(() => { const t = tints(canvas); return t.yellow > 0 && t.orange > 0; }, "all occurrences are highlighted, the current one in another colour");
  const shown = tints(canvas);
  assert(shown.yellow > shown.orange, "five occurrences are painted yellow and one orange");
  assert(editor.setSearchHighlights(targets, -1) === true, "no current occurrence");
  await waitFor(() => tints(canvas).orange === 0, "the current colour disappears");
  assert(editor.setSearchHighlights(null) === true, "highlights are cleared");
  await waitFor(() => { const t = tints(canvas); return t.yellow === 0 && t.orange === 0; }, "no highlight after clearing");
  assert(editor.setSearchHighlights(targets, 0) === true, "highlights come back");
  await waitFor(() => tints(canvas).yellow > 0 || tints(canvas).orange > 0, "highlights painted again");
  editor.element.dispatchEvent(new InputEvent("beforeinput", { inputType: "insertText", data: "!", bubbles: true, cancelable: true }));
  await waitFor(() => { const t = tints(canvas); return t.yellow === 0 && t.orange === 0; }, "an edit drops the highlights of the old revision");

  // ---- replace the current occurrence ------------------------------------------------------------------------------------------------
  await openText("The quick brown fox.");
  const fox = await editor.search("fox");
  await editor.selectSearchResult(fox, 0, {});
  const replaced = await editor.replaceCurrent({ revision: editor.state.revision, query: "fox", replacement: "wolf", options: {},
    start: fox.items[0].start, end: fox.items[0].end });
  assert(replaced.kind === "replacement" && replaced.count === 1 && replaced.state.revision === editor.state.revision, "replaceCurrent answers its count and the new state");
  assert(await text() === "The quick brown wolf." && editor.state.dirty && editor.state.canUndo, "the occurrence is replaced by one dirty, undoable edit");
  assert(editor.state.anchor === editor.state.focus && editor.state.focus === "The quick brown wolf".length, "the caret follows the replacement");
  await rejects(editor.replaceCurrent({ revision: editor.state.revision, query: "fox", replacement: "x", options: {}, start: 0, end: 3 }),
    "EDITOR_STALE_MATCH", "a range that is not the selection");
  await openText("same");
  const same = await editor.search("same");
  await editor.selectSearchResult(same, 0, {});
  const revision = editor.state.revision;
  const identical = await editor.replaceCurrent({ revision, query: "same", replacement: "same", options: {}, start: 0, end: 4 });
  assert(identical.count === 1 && editor.state.revision === revision && !editor.state.dirty && !editor.state.canUndo,
    "an identical replacement reports its occurrence without changing the revision, dirty state or undo history");
  const none = await editor.replaceAll({ revision: editor.state.revision, query: "absent", replacement: "x", options: {} });
  assert(none.count === 0 && editor.state.revision === revision && !editor.state.dirty && !editor.state.canUndo, "zero matches change nothing");
  await rejects(editor.replaceAll({ revision: editor.state.revision, query: "", replacement: "x", options: {} }), "EDITOR_EMPTY_QUERY", "an empty query");

  // ---- replace all: one undoable step -------------------------------------------------------------------------------------------------
  await openText(many);
  const manyResult = await editor.search("a");
  assert(manyResult.truncated, "more occurrences than the list shows");
  const all = await editor.replaceAll({ revision: editor.state.revision, query: "a", replacement: "aa", options: {} });
  const expected = "aa ".repeat(1105);
  assert(all.count === 1105 && await text() === expected, "Replace all replaces the 1105 occurrences, beyond the result bound, and counts them exactly");
  assert(editor.state.dirty && editor.state.canUndo && editor.state.revision === 1n, "Replace all is one edit");
  await editor.undo();
  // A revision counts applied edits (undo included), so the document stays "modified" after an undo, as for every v2 edit.
  assert(await text() === many && !editor.state.canUndo && editor.state.canRedo, "one Undo restores the whole source and leaves nothing more to undo");
  await editor.redo();
  assert(await text() === expected && !editor.state.canRedo, "one Redo reapplies every replacement");
  await openText("a,b,c\nd,e");
  const split = await editor.replaceAll({ revision: editor.state.revision, query: ",", replacement: "\n", options: {} });
  assert(split.count === 3 && await text() === "a\nb\nc\nd\ne", "a replacement that holds a line break splits paragraphs");
  await editor.undo();
  assert(await text() === "a,b,c\nd,e", "undoing it joins them again in one step");
  await openText("Straße STRASSE");
  assert((await editor.replaceAll({ revision: editor.state.revision, query: "strasse", replacement: "x", options: { matchCase: true } })).count === 0,
    "options reach the replacement");

  // ---- statistics ----------------------------------------------------------------------------------------------------------------------
  await openText("one two three\nfour");
  await editor.selectionAction("selectAll", editor.state);
  const all2 = await editor.statistics();
  assert(all2.kind === "statistics" && all2.revision === editor.state.revision && all2.anchor === 0 && all2.focus === editor.state.docLen,
    "statistics carry the revision and the selection as numbers");
  assert(all2.document.words === 4 && all2.document.characters === 15 && all2.document.charactersWithSpaces === 17 && all2.document.paragraphs === 2
      && all2.document.lines === 2 && all2.document.pages === 1, "document counts: words, characters, with spaces, paragraphs, lines, pages");
  assert(all2.selection.words === 4 && all2.selection.paragraphs === 2 && all2.selection.lines === 2 && all2.selection.pages === 1,
    "a whole-document selection counts as the document");
  editor.element.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true, cancelable: true }));
  await waitFor(() => editor.state.anchor === editor.state.focus, "collapse the selection");
  const caret = await editor.statistics();
  assert(caret.selection.words === 0 && caret.selection.characters === 0 && caret.selection.lines === 0, "a caret selects nothing");

  // ---- properties ----------------------------------------------------------------------------------------------------------------------
  await openText("props");
  const snapshot = await editor.readProperties();
  assert(snapshot.kind === "snapshot" && snapshot.editable.title === "" && snapshot.limits.title === 255 && snapshot.statistics.words === 1,
    "properties: values, limits and live counts");
  assert(Object.keys(snapshot.editable).join() === "title,subject,author,manager,company,category,keywords,comments,hyperlinkBase,contentStatus,language",
    "properties are named like the properties dialog");
  const applied = await editor.applyProperties({ title: "Mon titre", keywords: "a, b" });
  assert(applied.kind === "applied" && applied.changed === 2 && editor.state.dirty && editor.state.revision === 0n,
    "properties apply, mark the document modified and leave the text revision alone");
  assert((await editor.readProperties()).editable.title === "Mon titre", "the new value is read back");
  const tooLong = await editor.applyProperties({ subject: "x".repeat(300) });
  assert(tooLong.kind === "tooLong" && tooLong.field === "subject" && tooLong.max === 255 && tooLong.length === 300, "a value over its limit is refused with its field and limits");
  assert((await editor.readProperties()).editable.subject === "", "nothing is applied when a value is too long");
  const exported = await editor.exportDocx();
  window.confirm = () => true;
  const reopened = await editor.openDocument(async () => ({ kind: "ok", bytes: exported.bytes, format: "docx" }));
  assert(reopened.kind === "ok", "the exported DOCX opens");
  const back = await editor.readProperties();
  assert(back.editable.title === "Mon titre" && back.editable.keywords === "a, b" && back.lastModifiedBy !== undefined,
    "the properties survive a save as DOCX and an open");
  const removed = await editor.applyProperties({ title: "" });
  assert(removed.kind === "applied" && removed.changed === 1 && (await editor.readProperties()).editable.title === "", "an empty text removes a property");
  let unknown = null;
  try { await editor.applyProperties({ nickname: "x" }); } catch (error) { unknown = error; }
  assert(unknown instanceof TypeError, "an unknown property name is a caller error");
}
