// SPDX-License-Identifier: AGPL-3.0-or-later

const OPTION_LABELS = Object.freeze([
  ["matchCase", "searchMatchCase"],
  ["wholeWord", "searchWholeWord"],
  ["ignoreAccents", "searchIgnoreAccents"],
  ["matchNbsp", "searchMatchNbsp"],
]);
const MAX_ITEMS = 1000;
let nextPaneId = 0;
const label = (labels, key) => String(labels?.[key] ?? key);
const interpolate = (value, values) => value.replace(/\{(count|total|index|excerpt)\}/g, (_, key) => String(values[key] ?? ""));

function safeExcerpt(doc, item) {
  const excerpt = item?.excerpt;
  const text = typeof excerpt?.text === "string" ? excerpt.text : "";
  const encoder = new TextEncoder();
  const bytes = encoder.encode(text);
  const from = 0;
  const to = bytes.length;
  let first = Math.max(from, Math.min(to, Number.isSafeInteger(excerpt?.start) ? excerpt.start : 0));
  let last = Math.max(first, Math.min(to, Number.isSafeInteger(excerpt?.end) ? excerpt.end : first));
  const isContinuation = byte => (byte & 0xc0) === 0x80;
  while (first < last && isContinuation(bytes[first])) first++;
  while (last > first && last < bytes.length && isContinuation(bytes[last])) last--;
  const decoder = new TextDecoder();
  const part = (a, b) => decoder.decode(bytes.subarray(a, b));
  const fragment = doc.createDocumentFragment();
  fragment.append(doc.createTextNode(part(from, first)));
  if (last > first) {
    const mark = doc.createElement("mark");
    mark.textContent = part(first, last);
    fragment.append(mark);
  }
  fragment.append(doc.createTextNode(part(last, to)));
  return fragment;
}

export function createSearchPane({ document: doc = globalThis.document, labels, i18n, onQuery, onNavigate, onSelect, onClose } = {}) {
  if (!doc || !labels) throw new TypeError("A document and localized labels are required.");
  // Named parameters and number formatting come from the injected translator; without it, labels are plain templates.
  const tr = (key, values) => i18n ? i18n.t(key, values) : interpolate(label(labels, key), values);

  const root = doc.createElement("section");
  const paneId = ++nextPaneId;
  root.className = "search-pane";
  root.hidden = true;
  root.setAttribute("aria-labelledby", `search-pane-title-${paneId}`);
  const heading = doc.createElement("h2");
  heading.id = `search-pane-title-${paneId}`;
  heading.textContent = label(labels, "searchTitle");
  const closeButton = doc.createElement("button");
  closeButton.type = "button";
  closeButton.className = "search-pane-close";
  closeButton.dataset.searchClose = "";
  closeButton.textContent = label(labels, "searchClose");
  closeButton.setAttribute("aria-label", label(labels, "searchClose"));
  const headingRow = doc.createElement("div");
  headingRow.className = "search-pane-heading";
  headingRow.append(heading, closeButton);

  const query = doc.createElement("input");
  query.type = "search";
  query.className = "search-pane-query";
  query.dataset.searchQuery = "";
  query.setAttribute("aria-label", label(labels, "searchQuery"));
  query.autocomplete = "off";
  const optionsNode = doc.createElement("fieldset");
  optionsNode.className = "search-pane-options";
  const legend = doc.createElement("legend");
  legend.textContent = label(labels, "searchOptions");
  optionsNode.append(legend);
  const checkboxes = new Map();
  for (const [key, labelKey] of OPTION_LABELS) {
    const input = doc.createElement("input");
    input.type = "checkbox";
    input.dataset.searchOption = key;
    input.id = `search-option-${paneId}-${key}`;
    const text = doc.createElement("span");
    text.textContent = label(labels, labelKey);
    const row = doc.createElement("label");
    row.className = "search-pane-option";
    row.htmlFor = input.id;
    row.append(input, text);
    optionsNode.append(row);
    checkboxes.set(key, input);
  }

  const status = doc.createElement("p");
  status.className = "search-pane-status";
  status.dataset.searchCount = "";
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const navigation = doc.createElement("div");
  navigation.className = "search-pane-navigation";
  const previous = doc.createElement("button");
  previous.type = "button";
  previous.dataset.searchPrevious = "";
  previous.textContent = label(labels, "searchPrevious");
  const next = doc.createElement("button");
  next.type = "button";
  next.dataset.searchNext = "";
  next.textContent = label(labels, "searchNext");
  navigation.append(previous, next);
  const results = doc.createElement("div");
  results.className = "search-pane-results";
  results.setAttribute("role", "list");
  results.setAttribute("aria-label", label(labels, "searchResults"));
  const replacementSlot = doc.createElement("div");
  root.append(headingRow, query, optionsNode, status, navigation, replacementSlot, results);

  let state = { items: [], index: -1, truncated: false, busy: false, error: null, disabled: false, wrapped: false };
  let returnFocus = null;
  let composing = false;
  let disposed = false;
  let renderedItems = [];
  let renderedSourceItems = null;
  let resultButtons = [];

  const getOptions = () => Object.fromEntries(OPTION_LABELS.map(([key]) => [key, checkboxes.get(key).checked]));
  const requestQuery = () => { if (!disposed && !state.disabled) onQuery?.({ query: query.value, options: getOptions() }); };
  const makeCount = () => {
    if (state.disabled) return label(labels, "searchDisabled");
    if (state.busy) return label(labels, "searchBusy");
    if (state.error) return label(labels, "searchError");
    if (!query.value) return label(labels, "searchEmptyQuery");
    const total = renderedItems.length;
    if (!total) return label(labels, "searchNoResults");
    const current = Number.isSafeInteger(state.index) && state.index >= 0 && state.index < total ? state.index + 1 : 0;
    const count = tr("searchCount", { count: current, total });
    return [count, state.truncated ? label(labels, "searchTruncated") : "", state.wrapped ? label(labels, "searchWrapped") : ""].filter(Boolean).join(" — ");
  };
  const render = () => {
    if (disposed) return;
    const disabled = Boolean(state.disabled || state.busy);
    query.disabled = Boolean(state.disabled);
    for (const checkbox of checkboxes.values()) checkbox.disabled = Boolean(state.disabled);
    const sourceItems = Array.isArray(state.items) ? state.items : [];
    const itemsChanged = sourceItems !== renderedSourceItems;
    if (itemsChanged) {
      results.replaceChildren();
      renderedSourceItems = sourceItems;
      renderedItems = sourceItems.slice(0, MAX_ITEMS);
      resultButtons = [];
      renderedItems.forEach((item, index) => {
        const listItem = doc.createElement("div");
        listItem.setAttribute("role", "listitem");
        const button = doc.createElement("button");
        button.type = "button";
        button.className = "search-pane-result";
        button.dataset.searchResult = String(index);
        const excerptText = typeof item?.excerpt?.text === "string" ? item.excerpt.text : "";
        button.setAttribute("aria-label", tr("searchResult", { index: index + 1, excerpt: excerptText }));
        button.append(safeExcerpt(doc, item));
        listItem.append(button);
        results.append(listItem);
        resultButtons.push(button);
      });
    }
    status.textContent = makeCount();
    const hasItems = renderedItems.length > 0;
    previous.disabled = disabled || !hasItems;
    next.disabled = disabled || !hasItems;
    previous.setAttribute("aria-disabled", String(previous.disabled));
    next.setAttribute("aria-disabled", String(next.disabled));
    const focusedIndex = itemsChanged ? -1 : resultButtons.indexOf(doc.activeElement);
    const activeIndex = Number.isSafeInteger(state.index) && state.index >= 0 && state.index < resultButtons.length ? state.index : 0;
    const tabStop = focusedIndex >= 0 ? focusedIndex : activeIndex;
    resultButtons.forEach((button, index) => {
      button.disabled = Boolean(state.disabled);
      button.setAttribute("aria-current", String(index === state.index));
      button.tabIndex = index === tabStop && !state.disabled ? 0 : -1;
    });
  };
  const onInput = event => {
    if (composing || event?.isComposing) return;
    state = { ...state, wrapped: false };
    requestQuery();
  };
  const onCompositionStart = () => { composing = true; };
  const onCompositionEnd = () => { composing = false; onInput(); };
  const onQueryKeydown = event => {
    if (event.key !== "Enter" || composing || event.isComposing || event.keyCode === 229 || event.altKey || event.ctrlKey || event.metaKey) return;
    event.preventDefault();
    if (!state.disabled && !state.busy && renderedItems.length) onNavigate?.(event.shiftKey ? -1 : 1);
  };
  const onRootKeydown = event => {
    if (event.key !== "Escape" || composing || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    close();
  };
  const onOptionChange = () => { state = { ...state, wrapped: false }; requestQuery(); };
  const onPrevious = () => { if (!previous.disabled) onNavigate?.(-1); };
  const onNext = () => { if (!next.disabled) onNavigate?.(1); };
  const onResultClick = event => {
    const button = event.target.closest?.("[data-search-result]");
    if (!button || !results.contains(button) || state.disabled) return;
    const index = Number(button.dataset.searchResult);
    if (!Number.isSafeInteger(index) || index < 0 || index >= renderedItems.length) return;
    onSelect?.(index);
  };
  const onResultsKeydown = event => {
    if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) || state.disabled || !resultButtons.length) return;
    const current = resultButtons.indexOf(doc.activeElement);
    if (current < 0) return;
    const nextIndex = event.key === "Home" ? 0
      : event.key === "End" ? resultButtons.length - 1
        : (current + (event.key === "ArrowDown" ? 1 : resultButtons.length - 1)) % resultButtons.length;
    event.preventDefault();
    resultButtons[nextIndex].focus();
  };
  const onResultFocusIn = event => {
    const button = event.target.closest?.("[data-search-result]");
    if (!button || !results.contains(button)) return;
    const index = Number(button.dataset.searchResult);
    if (!Number.isSafeInteger(index) || resultButtons[index] !== button) return;
    resultButtons.forEach((result, resultIndex) => { result.tabIndex = resultIndex === index && !state.disabled ? 0 : -1; });
  };
  const onCloseButton = () => close();
  query.addEventListener("input", onInput);
  query.addEventListener("compositionstart", onCompositionStart);
  query.addEventListener("compositionend", onCompositionEnd);
  query.addEventListener("keydown", onQueryKeydown);
  for (const checkbox of checkboxes.values()) checkbox.addEventListener("change", onOptionChange);
  previous.addEventListener("click", onPrevious);
  next.addEventListener("click", onNext);
  results.addEventListener("click", onResultClick);
  results.addEventListener("keydown", onResultsKeydown);
  results.addEventListener("focusin", onResultFocusIn);
  closeButton.addEventListener("click", onCloseButton);
  root.addEventListener("keydown", onRootKeydown);

  const close = () => {
    if (disposed || root.hidden) return;
    root.hidden = true;
    state = { ...state, wrapped: false };
    onClose?.();
    returnFocus?.focus?.();
  };
  const open = ({ query: initialQuery = "", returnFocusTo = doc.activeElement } = {}) => {
    if (disposed || state.disabled) return false;
    returnFocus = returnFocusTo;
    root.hidden = false;
    query.value = String(initialQuery ?? "");
    state = { ...state, wrapped: false };
    render();
    query.focus();
    requestQuery();
    return true;
  };
  render();
  return Object.freeze({
    element: root,
    replacementSlot,
    open,
    close,
    setState(nextState = {}) {
      if (disposed) return;
      const hadFocus = doc.activeElement === query;
      state = { ...state, ...nextState };
      render();
      if (hadFocus && !query.disabled && doc.activeElement !== query) query.focus();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      query.removeEventListener("input", onInput);
      query.removeEventListener("compositionstart", onCompositionStart);
      query.removeEventListener("compositionend", onCompositionEnd);
      query.removeEventListener("keydown", onQueryKeydown);
      for (const checkbox of checkboxes.values()) checkbox.removeEventListener("change", onOptionChange);
      previous.removeEventListener("click", onPrevious);
      next.removeEventListener("click", onNext);
      results.removeEventListener("click", onResultClick);
      results.removeEventListener("keydown", onResultsKeydown);
      results.removeEventListener("focusin", onResultFocusIn);
      closeButton.removeEventListener("click", onCloseButton);
      root.removeEventListener("keydown", onRootKeydown);
      root.remove();
    },
  });
}
