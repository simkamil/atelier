// SPDX-License-Identifier: AGPL-3.0-or-later

/** Exercise the real app shortcut and Worker/WASM replacement path. */
export async function run({ assert, app, openText }) {
  const waitFor = async (predicate, name, timeoutMs = 7000) => {
    const end = performance.now() + timeoutMs;
    while (performance.now() < end) {
      if (predicate()) return;
      await new Promise(resolve => setTimeout(resolve, 10));
    }
    throw new Error(`Timed out waiting for ${name}`);
  };
  const key = (element, name, modifiers = {}) => element.dispatchEvent(new KeyboardEvent("keydown", {
    key: name, bubbles: true, cancelable: true, ...modifiers,
  }));
  const inputText = (element, text) => element.dispatchEvent(new InputEvent("beforeinput", {
    inputType: "insertText", data: text, bubbles: true, cancelable: true,
  }));
  const original = app.getState().text;
  const queryNode = () => document.querySelector("[data-search-query]");
  const replacementNode = () => document.querySelector("[data-replace-text]");
  const query = value => {
    const node = queryNode();
    node.value = value;
    node.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const option = (name, value) => {
    const node = document.querySelector(`[data-search-option="${name}"]`);
    node.checked = value;
    node.dispatchEvent(new Event("change", { bubbles: true }));
  };
  const configureOptions = options => {
    for (const name of ["matchCase", "wholeWord", "ignoreAccents", "matchNbsp"]) {
      const node = document.querySelector(`[data-search-option="${name}"]`);
      if (node.checked !== Boolean(options[name])) option(name, Boolean(options[name]));
    }
  };
  const openReplace = async value => {
    key(app.editor.element, "h", { ctrlKey: true });
    await waitFor(() => !document.querySelector("#search-slot").hidden, "Ctrl+H replacement pane");
    query(value);
    await waitFor(() => app.navigation.searchResult?.revision === app.getState().revision,
      `search response for ${JSON.stringify(value)}`);
  };
  const choose = async index => {
    const item = app.navigation.searchResult.items[index];
    if (!item) throw new Error(`Missing search result ${index}`);
    document.querySelector(`[data-search-result="${index}"]`).click();
    await waitFor(() => Math.min(app.getState().anchor, app.getState().focus) === item.start
        && Math.max(app.getState().anchor, app.getState().focus) === item.end,
    `selection of search result ${index}`);
  };
  const action = async (selector, label) => {
    const button = document.querySelector(selector);
    if (!button || button.disabled) throw new Error(`${label} is not enabled`);
    button.click();
  };
  const closeReplace = () => key(queryNode() ?? app.editor.element, "Escape");
  const setReplacement = value => { replacementNode().value = value; };
  const countDigits = () => (document.querySelector("[data-replace-status]")?.textContent ?? "").replace(/\D/g, "");

  try {
    await openText("The quick brown fox.");
    const clean = app.getState();
    key(app.editor.element, "h", { ctrlKey: true });
    assert(!document.querySelector("#search-slot").hidden
      && !document.querySelector("[data-replace-text]").hidden
      && document.activeElement === queryNode(),
    "Ctrl+H opens and focuses the actual search and replacement pane");
    query("fox");
    await waitFor(() => app.navigation.searchResult?.items.length === 1, "current-match search result");
    await choose(0);
    setReplacement("wolf");
    await action("[data-replace-one]", "Replace");
    await waitFor(() => app.getState().text === "The quick brown wolf.", "current match replaced in Worker/WASM");
    await waitFor(() => countDigits() === "1", "single replacement count");
    assert(app.getState().dirty && app.getState().canUndo,
      "replacing the selected occurrence creates a dirty, undoable edit");

    await openText("same");
    await openReplace("same");
    await choose(0);
    setReplacement("same");
    const beforeNoop = app.getState();
    await action("[data-replace-one]", "identical Replace");
    await waitFor(() => countDigits() === "1", "identical replacement occurrence count");
    assert(app.getState().revision === beforeNoop.revision && app.getState().text === "same"
        && !app.getState().dirty && !app.getState().canUndo,
    "identical replacement reports its occurrence without changing revision, dirty state, or undo history");
    query("absent");
    await waitFor(() => app.navigation.searchResult?.items.length === 0, "zero-match search result");
    const beforeZero = app.getState();
    await action("[data-replace-all]", "zero-match Replace all");
    await waitFor(() => document.querySelector("[data-replace-status]")?.textContent.includes("Aucune"),
      "completed zero-match replacement status");
    assert(app.getState().revision === beforeZero.revision && app.getState().text === "same"
        && !app.getState().dirty && !app.getState().canUndo
        && document.querySelector("[data-replace-status]").textContent.toLowerCase().includes("aucun"),
    "zero matches report none and preserve revision, dirty state, text, and undo history");

    const many = "a ".repeat(1105);
    await openText(many);
    await openReplace("a");
    await waitFor(() => app.navigation.searchResult?.items.length === 1000, "capped visible search results");
    assert(app.navigation.searchResult.truncated,
      "the UI keeps its 1000-result search bound while replacement uses the full source");
    setReplacement("aa");
    await action("[data-replace-all]", "Replace all beyond the search-result cap");
    const expectedMany = "aa ".repeat(1105);
    await waitFor(() => app.getState().text === expectedMany, "all original matches replaced once");
    await waitFor(() => countDigits() === "1105", "uncapped exact replacement count");
    assert(app.getState().dirty && app.getState().canUndo,
      "Replace all makes one dirty undoable edit for all 1105 original occurrences");
    await app.editor.undo();
    await waitFor(() => app.getState().text === many && !app.getState().canUndo, "single undo restores the full source");
    assert(!app.getState().dirty,
      "one Undo restores the entire multi-match transaction and clean state");
    await app.editor.redo();
    await waitFor(() => app.getState().text === expectedMany && !app.getState().canRedo,
      "single redo reapplies the full replacement");
    assert(app.getState().dirty,
      "one Redo reapplies all matches without creating per-match transactions");

    await openText("École e\u0301cole écoleine\nStraße STRASSE\nA\u00a0B A B");
    await openReplace("ecole");
    option("ignoreAccents", true);
    option("wholeWord", true);
    await waitFor(() => app.navigation.searchResult?.items.length === 2, "accent-insensitive whole-word search");
    await choose(0);
    setReplacement("campus");
    await action("[data-replace-one]", "Unicode Replace");
    await waitFor(() => app.getState().text === "campus e\u0301cole écoleine\nStraße STRASSE\nA\u00a0B A B",
      "Unicode options replace the original accented grapheme range");
    assert(app.getState().text.startsWith("campus e\u0301cole")
        && app.getState().text.includes("écoleine"),
    "accent folding and whole-word boundaries agree with the real Rust searcher");

    const replaceAllWithOptions = async ({ source, search, replacement, options, expected, count, name }) => {
      await openText(source);
      await openReplace(search);
      configureOptions(options);
      await waitFor(() => app.navigation.searchResult?.revision === app.getState().revision,
        `${name} configured replacement search`);
      setReplacement(replacement);
      await action("[data-replace-all]", `${name} Replace all`);
      await waitFor(() => app.getState().text === expected, `${name} exact Replace all output`);
      await waitFor(() => countDigits() === String(count), `${name} Replace all count`);
      assert(app.getState().text === expected && countDigits() === String(count),
        `${name} Replace all applies the configured option and reports its exact occurrence count`);
    };
    await replaceAllWithOptions({ source: "cat CAT Cat", search: "cat", replacement: "dog",
      options: { matchCase: true }, expected: "dog CAT Cat", count: 1, name: "matchCase" });
    await replaceAllWithOptions({ source: "cat cater cat", search: "cat", replacement: "dog",
      options: { wholeWord: true }, expected: "dog cater dog", count: 2, name: "wholeWord" });
    await replaceAllWithOptions({ source: "cafe café caféine", search: "cafe", replacement: "coffee",
      options: { ignoreAccents: true }, expected: "coffee coffee coffeeine", count: 3, name: "ignoreAccents" });
    await replaceAllWithOptions({ source: "A\u00a0B A B", search: "A B", replacement: "X",
      options: { matchNbsp: true }, expected: "A\u00a0B X", count: 1, name: "matchNbsp" });

    await openText("alpha\n\nbeta\n");
    await openReplace("alpha");
    await choose(0);
    setReplacement("");
    await action("[data-replace-one]", "empty replacement");
    await waitFor(() => app.getState().text === "\n\nbeta\n", "empty replacement preserves line endings");
    await openReplace("beta");
    await choose(0);
    setReplacement("first\nsecond");
    await action("[data-replace-one]", "newline replacement");
    await waitFor(() => app.getState().text === "\n\nfirst\nsecond\n", "newline-containing replacement");
    assert(app.getState().text === "\n\nfirst\nsecond\n",
      "empty strings and newline-containing replacements preserve surrounding blank and trailing lines");

    await openText("stale match");
    const staleSearch = await app.editor.search("stale");
    await app.editor.selectSearchResult(staleSearch, 0);
    const selectedState = app.getState();
    let staleSelection = false;
    try {
      await app.editor.replaceCurrent({ revision: selectedState.revision, query: "stale", replacement: "fresh", start: 6, end: 11 });
    } catch (error) { staleSelection = error.code === "EDITOR_STALE_MATCH"; }
    assert(staleSelection && app.getState().text === "stale match"
        && app.getState().revision === selectedState.revision && !app.getState().canUndo,
    "a stale match selection is rejected by Worker/WASM without publishing a partial edit");

    const oldRevision = app.getState().revision;
    key(app.editor.element, "End");
    await waitFor(() => app.getState().focus === new TextEncoder().encode("stale match").length,
      "caret change before stale revision probe");
    inputText(app.editor.element, "!");
    await waitFor(() => app.getState().text === "stale match!", "revision change before stale replacement");
    let staleRevision = false;
    try {
      await app.editor.replaceAll({ revision: oldRevision, query: "stale", replacement: "fresh" });
    } catch (error) { staleRevision = error.code === "EDITOR_STALE_QUERY"; }
    assert(staleRevision && app.getState().text === "stale match!" && app.getState().dirty,
      "a replacement carrying an old revision is refused without changing the latest document");

    await openText("x".repeat(10 * 1024));
    const overflowBefore = app.getState();
    let overflow = false;
    try {
      await app.editor.replaceAll({ revision: overflowBefore.revision, query: "x", replacement: "x".repeat(20) });
    } catch (error) { overflow = error.code === "EDITOR_TEXT_LIMIT"; }
    assert(overflow && app.getState().text === overflowBefore.text
        && app.getState().revision === overflowBefore.revision && !app.getState().dirty
        && !app.getState().canUndo,
    "a result above 128 KiB fails atomically with the original revision, clean state, and history intact");
  } finally {
    if (!document.querySelector("#search-slot")?.hidden) closeReplace();
    await openText(original);
  }
}
