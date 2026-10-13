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

function statisticsResult(state, documentWords, selectionWords) {
  const counts = words => ({ words, characters: words * 3, charactersWithSpaces: words * 4, paragraphs: 2, lines: 3, pages: 1 });
  return { kind: "statistics", revision: state.revision, anchor: state.anchor, focus: state.focus,
    document: counts(documentWords), selection: counts(selectionWords) };
}

function searchResult(revision, start, text) {
  return { kind: "search", revision, truncated: false, items: [{ start, end: start + text.length,
    excerpt: { text, start: 0, end: text.length, truncatedBefore: false, truncatedAfter: false }, rects: [] }] };
}

export async function run({ assert, fixture }) {
  const response = await within(fetch("/i18n/fr-FR.txt"), "French labels response");
  assert(response.ok, "French labels are loaded from the app catalog");
  const source = await within(response.text(), "French labels text");
  const labels = Object.fromEntries(source.split(/\r?\n/).filter(line => line && !line.startsWith("#") && line.includes("="))
    .map(line => { const at = line.indexOf("="); return [line.slice(0, at), line.slice(at + 1)]; }));

  const paneSlot = document.createElement("div");
  const statusSlot = document.createElement("div");
  const editorElement = document.createElement("button");
  editorElement.type = "button";
  editorElement.textContent = "Editor";
  fixture.append(editorElement, paneSlot, statusSlot);

  const state = { revision: 7, anchor: 0, focus: 0, textBytes: new TextEncoder().encode("first second third fourth") };
  const searches = [];
  const highlights = [];
  const statisticsRequests = [];
  let deferStatistics = false;
  const editor = {
    element: editorElement,
    state,
    getState() { return this.state; },
    search(query, options) {
      const pending = deferred();
      searches.push({ query, options, ...pending });
      return pending.promise;
    },
    statistics() {
      if (!deferStatistics) return Promise.resolve(statisticsResult(this.state, 10, 0));
      const pending = deferred();
      statisticsRequests.push(pending);
      return pending.promise;
    },
    setSearchHighlights(result, index) { highlights.push({ result, index }); },
    selectSearchResult() { return Promise.resolve(); },
  };
  const { createNavigationTools } = await import("/navigation-tools.mjs");
  const tools = createNavigationTools({ document, editor, paneSlot, statusSlot, labels });
  tools.update(editor.state);
  tools.setEnabled(true);
  const queryInput = () => paneSlot.querySelector("[data-search-query]");
  const setQuery = text => {
    const input = queryInput();
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const paneResults = () => [...paneSlot.querySelectorAll("[data-search-result]")];
  const activeHighlight = () => [...highlights].reverse().find(entry => entry.result)?.result;

  // A slow first query must not overwrite the more recent query's results.
  tools.openSearch();
  setQuery("first");
  await waitFor(() => searches.length === 1, "first search request", 1200);
  setQuery("second");
  await waitFor(() => searches.length === 2, "second search request", 1200);
  const second = searchResult(editor.state.revision, 6, "second");
  searches[1].resolve(second);
  await waitFor(() => tools.searchResult === second && activeHighlight() === second && paneResults()[0]?.textContent === "second", "second query rendering and highlight", 1200);
  const highlightsBeforeLateFirst = highlights.length;
  searches[0].resolve(searchResult(editor.state.revision, 0, "first"));
  await within(new Promise(resolve => setTimeout(resolve, 35)), "late first reply settling", 500);
  assert(tools.searchResult === second && paneResults()[0]?.textContent === "second" && highlights.length === highlightsBeforeLateFirst,
    "a late first search reply cannot replace or highlight over the second result");

  // Closing the pane invalidates a query already in flight.
  setQuery("closed-late");
  await waitFor(() => searches.length === 3, "search before closing pane", 1200);
  paneSlot.querySelector("[data-search-close]").click();
  const highlightsAfterClose = highlights.length;
  searches[2].resolve(searchResult(editor.state.revision, 12, "closed-late"));
  await within(new Promise(resolve => setTimeout(resolve, 35)), "reply after pane close", 500);
  assert(paneSlot.hidden && tools.searchResult === null && highlights.length === highlightsAfterClose,
    "a reply after close cannot reopen the pane or add highlights");

  // Preview transition invalidates an in-flight query and hides the search UI.
  tools.setEnabled(true);
  tools.openSearch();
  setQuery("preview-late");
  await waitFor(() => searches.length === 4, "search before preview transition", 1200);
  tools.setEnabled(false, { preview: true });
  const highlightsAfterPreview = highlights.length;
  searches[3].resolve(searchResult(editor.state.revision, 18, "preview-late"));
  await within(new Promise(resolve => setTimeout(resolve, 35)), "reply after preview transition", 500);
  assert(paneSlot.hidden && tools.searchResult === null && highlights.length === highlightsAfterPreview,
    "preview mode invalidates pending search results and highlights");
  tools.setEnabled(true);

  // A statistics response tied to an older selection must be discarded.
  deferStatistics = true;
  editor.state = { ...editor.state, anchor: 1, focus: 3 };
  tools.update(editor.state);
  tools.openStatistics();
  await waitFor(() => statisticsRequests.length === 1, "statistics for old selection", 1200);
  const oldStatistics = statisticsResult(editor.state, 10, 2);
  const changedSelection = { ...editor.state, anchor: 4, focus: 8 };
  editor.state = changedSelection;
  tools.update(changedSelection);
  await waitFor(() => statisticsRequests.length === 2, "statistics for updated selection", 1200);
  const currentStatistics = statisticsResult(changedSelection, 20, 4);
  statisticsRequests[1].resolve(currentStatistics);
  const statisticsDialog = statusSlot.querySelector("[data-statistics-dialog]");
  const currentWordCell = statusSlot.querySelector('[data-statistics-count="selection:words"]');
  await waitFor(() => currentWordCell?.textContent === "4", "current selection statistics rendering", 1200);
  statisticsRequests[0].resolve(oldStatistics);
  await within(new Promise(resolve => setTimeout(resolve, 35)), "late statistics reply settling", 500);
  assert(statisticsDialog.open && currentWordCell.textContent === "4"
    && statusSlot.querySelector('[data-statistics-count="document:words"]').textContent === "20",
  "late statistics for the previous selection are ignored");
  statisticsDialog.querySelector(".ui-dialog-close").click();

  // Disposal cancels a debounce timer and ignores already-dispatched replies.
  tools.openSearch();
  setQuery("dispose-late");
  await waitFor(() => searches.length === 5, "search before disposal", 1200);
  setQuery("timer-cancel");
  const searchCountBeforeDispose = searches.length;
  const highlightsBeforeDispose = highlights.length;
  tools.dispose();
  searches[4].resolve(searchResult(editor.state.revision, 0, "dispose-late"));
  await within(new Promise(resolve => setTimeout(resolve, 160)), "disposed debounce and late search reply", 500);
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "f", ctrlKey: true, bubbles: true, cancelable: true }));
  assert(searches.length === searchCountBeforeDispose && highlights.length === highlightsBeforeDispose + 1
    && highlights.at(-1).result === null && paneSlot.hidden && paneSlot.childElementCount === 0 && statusSlot.childElementCount === 0,
  "dispose cancels pending timers, clears highlights, removes views, and ignores late replies");
}
