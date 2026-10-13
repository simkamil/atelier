// SPDX-License-Identifier: AGPL-3.0-or-later

const within = (promise, description, timeoutMs = 1200) => new Promise((resolve, reject) => {
  const timer = setTimeout(() => reject(new Error(`Timed out waiting for ${description} after ${timeoutMs}ms`)), timeoutMs);
  Promise.resolve(promise).then(value => { clearTimeout(timer); resolve(value); }, error => { clearTimeout(timer); reject(error); });
});

const waitFor = async (predicate, description, timeoutMs = 1200) => within((async () => {
  while (!predicate()) await new Promise(resolve => setTimeout(resolve, 10));
})(), description, timeoutMs);

const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};

function searchResult(revision, start, query) {
  return { kind: "search", revision, truncated: false, items: [{ start, end: start + query.length,
    excerpt: { text: query, start: 0, end: query.length, truncatedBefore: false, truncatedAfter: false }, rects: [] }] };
}

export async function run({ assert, fixture }) {
  const paneSlot = document.createElement("div");
  const statusSlot = document.createElement("div");
  const editorElement = document.createElement("button");
  editorElement.type = "button";
  editorElement.textContent = "Editor";
  fixture.append(editorElement, paneSlot, statusSlot);

  const labels = {
    searchTitle: "Search", searchQuery: "Find text", searchOptions: "Options", searchResults: "Results",
    searchMatchCase: "Match case", searchWholeWord: "Whole word", searchIgnoreAccents: "Ignore accents", searchMatchNbsp: "Match NBSP",
    searchPrevious: "Previous", searchNext: "Next", searchClose: "Close", searchCount: "{count} / {total}",
    searchEmptyQuery: "Enter a search", searchNoResults: "No results", searchTruncated: "Limited", searchBusy: "Searching",
    searchError: "Search failed", searchDisabled: "Unavailable", searchWrapped: "Wrapped", searchResult: "Result {index}: {excerpt}",
    replaceTitle: "Replace", replaceText: "Replacement text", replaceOne: "Replace", replaceAll: "Replace all",
    replaceBusy: "Replacing…", replaceError: "Replacement failed", replaceNone: "No matches replaced",
    replaceCountOne: "1 occurrence replaced", replaceCountMany: "{count} occurrences replaced",
  };

  let state = { revision: 5, anchor: 0, focus: 4, textBytes: new TextEncoder().encode("word other word") };
  const searches = [], currentReplacements = [], allReplacements = [], highlights = [];
  const editor = {
    element: editorElement,
    get state() { return state; },
    search(query, options) {
      const pending = deferred();
      searches.push({ query, options, ...pending });
      return pending.promise;
    },
    replaceCurrent(values, options) {
      const pending = deferred();
      currentReplacements.push({ values, options, ...pending });
      return pending.promise;
    },
    replaceAll(values, options) {
      const pending = deferred();
      allReplacements.push({ values, options, ...pending });
      return pending.promise;
    },
    statistics() {
      const snapshot = state;
      return Promise.resolve({ kind: "statistics", revision: snapshot.revision, anchor: snapshot.anchor, focus: snapshot.focus,
        document: { words: 3, characters: 15, charactersWithSpaces: 17, paragraphs: 1, lines: 1, pages: 1 },
        selection: { words: 1, characters: 4, charactersWithSpaces: 4, paragraphs: 1, lines: 1, pages: 1 } });
    },
    setSearchHighlights(result, index) { highlights.push({ result, index }); },
    selectSearchResult() { return Promise.resolve(); },
  };

  const { createNavigationTools } = await import("/navigation-tools.mjs");
  const tools = createNavigationTools({ document, editor, paneSlot, statusSlot, labels });
  tools.update(state);
  tools.setEnabled(true);
  const queryInput = () => paneSlot.querySelector("[data-search-query]");
  const replacementInput = () => paneSlot.querySelector("[data-replace-text]");
  const replacementOne = () => paneSlot.querySelector("[data-replace-one]");
  const replacementAll = () => paneSlot.querySelector("[data-replace-all]");
  const replacementStatus = () => paneSlot.querySelector("[data-replace-status]");
  try {
    const ctrlH = new KeyboardEvent("keydown", { key: "h", ctrlKey: true, bubbles: true, cancelable: true });
    document.dispatchEvent(ctrlH);
    assert(ctrlH.defaultPrevented && !paneSlot.hidden && !paneSlot.querySelector("[data-replace-text]")?.closest("[hidden]"),
      "Ctrl+H opens the search pane with replacement controls visible");
    await waitFor(() => searches.length === 1, "Ctrl+H seeded search");
    assert(searches[0].query === "word", "Ctrl+H seeds search from the selected text");
    const firstSearch = searchResult(state.revision, 0, "word");
    searches[0].resolve(firstSearch);
    await waitFor(() => tools.searchResult === firstSearch, "initial replacement search results");
    assert(!replacementOne().disabled && !replacementAll().disabled,
      "the selected match enables Replace and the query enables Replace all");

    const composing = new KeyboardEvent("keydown", { key: "h", ctrlKey: true, isComposing: true, bubbles: true, cancelable: true });
    document.dispatchEvent(composing);
    const altGraph = new KeyboardEvent("keydown", { key: "h", ctrlKey: true, bubbles: true, cancelable: true });
    Object.defineProperty(altGraph, "getModifierState", { value: name => name === "AltGraph" });
    document.dispatchEvent(altGraph);
    const altKey = new KeyboardEvent("keydown", { key: "h", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
    document.dispatchEvent(altKey);
    assert(!composing.defaultPrevented && !altGraph.defaultPrevented && !altKey.defaultPrevented,
      "Ctrl+H does not intercept IME, AltGraph, or Alt-modified input");

    const modal = document.createElement("dialog");
    fixture.append(modal);
    modal.showModal();
    const modalShortcut = new KeyboardEvent("keydown", { key: "h", ctrlKey: true, bubbles: true, cancelable: true });
    document.dispatchEvent(modalShortcut);
    assert(!modalShortcut.defaultPrevented, "Ctrl+H remains untouched while a modal dialog is open");
    modal.close();
    modal.remove();

    const replacement = replacementInput();
    replacement.value = "ligne 1\nligne 2";
    replacementOne().click();
    await waitFor(() => currentReplacements.length === 1, "single replacement dispatch");
    assert(currentReplacements[0].values.replacement === "ligne 1\nligne 2"
      && currentReplacements[0].values.start === 0 && currentReplacements[0].values.end === 4,
    "Replace passes exact multiline text and the selected UTF-8 match range");
    const failure = Object.assign(new Error("stale match"), { code: "EDITOR_STALE_MATCH" });
    currentReplacements[0].reject(failure);
    await waitFor(() => !replacementOne().disabled && replacementStatus().textContent === labels.replaceError,
      "replacement error clears busy state");
    assert(!replacementAll().disabled, "a failed replacement does not leave Replace all busy-disabled");

    replacement.value = "";
    replacementAll().click();
    await waitFor(() => allReplacements.length === 1, "replace all with empty text");
    assert(allReplacements[0].values.replacement === "", "Replace all passes an empty replacement without trimming or substitution");
    allReplacements[0].resolve({ kind: "replacement", count: 0, state });
    await waitFor(() => !replacementAll().disabled && replacementStatus().textContent === labels.replaceNone,
      "no-op replacement clears busy state and announces zero matches");

    replacement.value = "stale query result";
    replacementAll().click();
    await waitFor(() => allReplacements.length === 2, "replacement before query change");
    const query = queryInput();
    query.value = "other";
    query.dispatchEvent(new Event("input", { bubbles: true }));
    await waitFor(() => searches.length === 2, "replacement query update search");
    const changedQueryResult = searchResult(state.revision, 6, "other");
    searches[1].resolve(changedQueryResult);
    await waitFor(() => tools.searchResult === changedQueryResult, "new query result after stale replacement");
    allReplacements[1].resolve({ kind: "replacement", count: 19, state });
    await waitFor(() => !replacementAll().disabled, "stale replacement after query change finishes");
    assert(replacementStatus().textContent === "" && replacement.value === "stale query result",
      "a late result from the previous query is ignored and does not overwrite the replacement draft");

    replacementAll().click();
    await waitFor(() => allReplacements.length === 3, "replacement before pane close");
    paneSlot.querySelector("[data-search-close]").click();
    assert(paneSlot.hidden, "closing the pane hides its replacement controls immediately");
    allReplacements[2].resolve({ kind: "replacement", count: 23, state });
    await new Promise(resolve => setTimeout(resolve, 30));
    assert(paneSlot.hidden, "a late replacement result after close cannot reopen the pane");

    const reopen = new KeyboardEvent("keydown", { key: "h", ctrlKey: true, bubbles: true, cancelable: true });
    document.dispatchEvent(reopen);
    await waitFor(() => searches.length === 3, "search after reopening replacement pane");
    const reopenedResult = searchResult(state.revision, 0, "word");
    searches[2].resolve(reopenedResult);
    await waitFor(() => tools.searchResult === reopenedResult, "replacement results after reopening");
    assert(replacementStatus().textContent === "", "late result after close is not presented when the pane reopens");

    replacementAll().click();
    await waitFor(() => allReplacements.length === 4, "replacement before document replacement");
    state = { revision: 6, anchor: 0, focus: 4, textBytes: new TextEncoder().encode("word in a new document") };
    tools.update(state, { documentReplaced: true });
    await waitFor(() => searches.length === 4, "new document search");
    allReplacements[3].resolve({ kind: "replacement", count: 29, state });
    const newDocumentResult = searchResult(state.revision, 0, "word");
    searches[3].resolve(newDocumentResult);
    await waitFor(() => tools.searchResult === newDocumentResult && !replacementAll().disabled,
      "replacement and search after document replacement settle");
    assert(replacementStatus().textContent === "", "document replacement invalidates the prior document's late result");
  } finally {
    tools.dispose();
  }
}
