// SPDX-License-Identifier: AGPL-3.0-or-later
import { createSearchPane } from "./search-pane.mjs";
import { createStatisticsControls } from "./statistics-controls.mjs";
import { createReplaceControls } from "./replace-controls.mjs";

const encoder = new TextEncoder();
const decoder = new TextDecoder("utf-8", { fatal: true });
function querySeed(text) {
  let bytes = 0, out = "";
  for (const scalar of text) {
    const size = encoder.encode(scalar).length;
    if (bytes + size > 1024) break;
    out += scalar; bytes += size;
  }
  return out;
}

/** Coordinates revisioned search, replacement and statistics views. */
export function createNavigationTools({ document: doc = globalThis.document, editor, paneSlot, statusSlot, labels, i18n }) {
  let disposed = false, enabled = false, opened = false;
  let currentState = null, query = "", options = {};
  let searchResult = null, index = -1, searchGeneration = 0, searchTimer = 0;
  let statisticsGeneration = 0, statisticsTimer = 0;
  let selectionAbort = new AbortController();
  let navigationQueue = Promise.resolve();
  let replaceVisible = false, replacePending = false, replaceCount = null, replaceError = null;
  let replaceActionId = 0, documentEpoch = 0, searchPending = false;
  const enqueueNavigation = run => {
    const generation = searchGeneration;
    navigationQueue = navigationQueue.then(() => !disposed && generation === searchGeneration ? run() : undefined).catch(() => {});
  };
  const pane = createSearchPane({ document: doc, labels, i18n,
    onQuery: change => { query = change.query; options = { ...change.options }; resetReplacement(); scheduleSearch(); },
    onNavigate: direction => enqueueNavigation(() => navigate(direction)),
    onSelect: selected => enqueueNavigation(() => select(selected)),
    onClose: () => {
      opened = false; replaceVisible = false; paneSlot.hidden = true; resetReplacement(); invalidateSearch(); paintReplacement();
    },
  });
  paneSlot.append(pane.element); paneSlot.hidden = true;
  const replacement = createReplaceControls({ document: doc, labels, i18n,
    onReplace: ({ replacement: value }) => enqueueNavigation(() => replaceMatches(false, value)),
    onReplaceAll: ({ replacement: value }) => enqueueNavigation(() => replaceMatches(true, value)),
  });
  pane.replacementSlot.append(replacement.element);
  const statistics = createStatisticsControls({ document: doc, labels, i18n, onOpen: () => scheduleStatistics(0) });
  statusSlot.append(statistics.element);

  function invalidateSearch() {
    searchGeneration += 1; clearTimeout(searchTimer); searchTimer = 0;
    selectionAbort.abort(); selectionAbort = new AbortController();
    searchResult = null; index = -1;
    editor.setSearchHighlights(null);
  }
  function paintSearch(extra = {}) {
    searchPending = Boolean(extra.busy);
    pane.setState({ items: searchResult?.items ?? [], index, truncated: searchResult?.truncated ?? false,
      disabled: !enabled, error: null, wrapped: false, ...extra, busy: searchPending || replacePending });
    paintReplacement();
  }
  function resetReplacement() { replaceActionId += 1; replaceCount = null; replaceError = null; }
  function paintReplacement() {
    const state = editor.state, item = searchResult?.items[index];
    const selected = item && searchResult.revision === state?.revision
      && item.start === Math.min(state.anchor,state.focus) && item.end === Math.max(state.anchor,state.focus);
    replacement.setState({ visible: opened && replaceVisible, disabled: !enabled, busy: replacePending,
      canReplace: Boolean(selected && !searchPending), canReplaceAll: Boolean(query && searchResult && !searchPending),
      count: replaceCount, error: replaceError });
  }
  async function replaceMatches(all, value) {
    if (disposed || !enabled || !opened || !replaceVisible || replacePending || searchPending || !searchResult || !query) return;
    const item = searchResult.items[index];
    if (!all && !item) return;
    const action = ++replaceActionId, epoch = documentEpoch, sourceQuery = query;
    const values = { revision: editor.state.revision, query, replacement: value, options: { ...options },
      ...(all ? {} : { start: item.start, end: item.end }) };
    replacePending = true; replaceCount = null; replaceError = null; paintSearch();
    try {
      const result = await editor[all ? "replaceAll" : "replaceCurrent"](values, { signal: selectionAbort.signal });
      if (disposed || !opened || !enabled || action !== replaceActionId || epoch !== documentEpoch || query !== sourceQuery
          || editor.state?.revision !== result.state.revision) return;
      replaceCount = result.count;
    } catch (error) {
      if (!disposed && opened && enabled && action === replaceActionId && epoch === documentEpoch) replaceError = error.code ?? "REPLACE_FAILED";
    } finally {
      replacePending = false;
      if (!disposed) paintSearch({ busy: searchPending });
    }
  }
  function scheduleSearch(delay = 100) {
    invalidateSearch();
    if (disposed || !enabled || !opened) return;
    const generation = searchGeneration;
    paintSearch({ busy: Boolean(query) });
    if (!query) return;
    searchTimer = setTimeout(async () => {
      searchTimer = 0;
      try {
        const result = await editor.search(query, options);
        if (disposed || !opened || !enabled || generation !== searchGeneration || result.revision !== editor.state?.revision) return;
        searchResult = result;
        const state = editor.state;
        index = result.items.findIndex(item => item.start === Math.min(state.anchor,state.focus) && item.end === Math.max(state.anchor,state.focus));
        editor.setSearchHighlights(result,index);
        paintSearch();
      } catch (error) {
        if (disposed || generation !== searchGeneration || !enabled || !opened) return;
        paintSearch({ error: error.code ?? "SEARCH_FAILED" });
      }
    }, delay);
  }
  async function select(next, wrapped = false) {
    if (disposed || !enabled || !opened || !searchResult || !searchResult.items[next]) return;
    const generation = searchGeneration, result = searchResult;
    try {
      await editor.selectSearchResult(result,next,{ signal: selectionAbort.signal });
      if (disposed || generation !== searchGeneration || !opened || !enabled) return;
      index = next; editor.setSearchHighlights(result,index); paintSearch({ wrapped });
    } catch (error) {
      if (!disposed && generation === searchGeneration && enabled && opened) paintSearch({ error: error.code ?? "SEARCH_FAILED" });
    }
  }
  async function navigate(direction) {
    if (!searchResult?.items.length || !enabled || !opened) return;
    const items = searchResult.items;
    let next = index + direction, wrapped = false;
    if (index < 0) {
      const state = editor.state;
      if (direction > 0) next = items.findIndex(item => item.start >= Math.max(state.anchor,state.focus));
      else {
        next = -1;
        for (let i = items.length - 1; i >= 0; i--) if (items[i].start < Math.min(state.anchor,state.focus)) { next = i; break; }
      }
      if (next < 0) { wrapped = true; next = direction > 0 ? 0 : items.length - 1; }
    } else if (next < 0 || next >= items.length) { wrapped = true; next = (next + items.length) % items.length; }
    await select(next,wrapped);
  }
  function scheduleStatistics(delay = 120) {
    statisticsGeneration += 1; clearTimeout(statisticsTimer); statisticsTimer = 0;
    const generation = statisticsGeneration;
    statistics.setState({ document: null, selection: null, hasSelection: Boolean(editor.state && editor.state.anchor !== editor.state.focus),
      busy: enabled, disabled: !enabled, error: null });
    if (disposed || !enabled) return;
    statisticsTimer = setTimeout(async () => {
      statisticsTimer = 0;
      try {
        const result = await editor.statistics();
        const state = editor.state;
        if (disposed || !enabled || generation !== statisticsGeneration || result.revision !== state?.revision
            || result.anchor !== state.anchor || result.focus !== state.focus) return;
        statistics.setState({ document: result.document, selection: result.selection,
          hasSelection: result.anchor !== result.focus, busy: false, disabled: false, error: null });
      } catch (error) {
        if (!disposed && enabled && generation === statisticsGeneration) statistics.setState({ busy: false, error: error.code ?? "STATISTICS_FAILED" });
      }
    },delay);
  }
  function openSearch(showReplacement = false) {
    if (disposed || !enabled || doc.querySelector("dialog:modal")) return false;
    const state = editor.state;
    const selected = state && state.anchor !== state.focus
      ? decoder.decode(state.textBytes.subarray(Math.min(state.anchor,state.focus),Math.max(state.anchor,state.focus))) : "";
    if (selected) query = querySeed(selected);
    opened = true; replaceVisible = showReplacement; paneSlot.hidden = false;
    pane.open({ query, returnFocusTo: editor.element });
    return true;
  }
  function openStatistics() {
    if (disposed || !enabled || doc.querySelector("dialog:modal")) return false;
    return statistics.open({ returnFocusTo: doc.activeElement });
  }
  const onKeyDown = event => {
    if (event.isComposing || event.keyCode === 229 || event.altKey || event.getModifierState?.("AltGraph") || doc.querySelector("dialog:modal")) return;
    const action = event.key.toLowerCase();
    if ((event.ctrlKey || event.metaKey) && !event.shiftKey && ["f", "h"].includes(action)) {
      event.preventDefault(); openSearch(action === "h");
    }
  };
  doc.addEventListener("keydown",onKeyDown);
  return Object.freeze({
    openSearch: () => openSearch(false), openReplace: () => openSearch(true), openStatistics,
    get searchResult() { return searchResult; },
    update(state,{ documentReplaced = false } = {}) {
      if (disposed) return;
      const changed = documentReplaced || currentState?.revision !== state.revision;
      if (documentReplaced) { documentEpoch += 1; resetReplacement(); }
      else if (changed && !replacePending) resetReplacement();
      currentState = state;
      if (changed) scheduleSearch();
      else if (searchResult) {
        index = searchResult.items.findIndex(item => item.start === Math.min(state.anchor,state.focus) && item.end === Math.max(state.anchor,state.focus));
        editor.setSearchHighlights(searchResult,index); paintSearch();
      }
      scheduleStatistics();
    },
    setEnabled(value,{ preview = false } = {}) {
      if (disposed) return;
      const changed = enabled !== Boolean(value);
      enabled = Boolean(value);
      if (!enabled) { resetReplacement(); invalidateSearch(); if (preview && opened) pane.close(); paintSearch(); }
      if (changed) { if (enabled) { paintSearch(); if (opened) scheduleSearch(); } scheduleStatistics(); }
    },
    dispose() {
      if (disposed) return;
      disposed = true; invalidateSearch(); statisticsGeneration += 1;
      clearTimeout(statisticsTimer); doc.removeEventListener("keydown",onKeyDown);
      resetReplacement(); replacement.dispose(); pane.dispose(); statistics.dispose(); paneSlot.hidden = true;
    },
  });
}
