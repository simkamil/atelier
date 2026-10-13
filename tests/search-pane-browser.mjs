// SPDX-License-Identifier: AGPL-3.0-or-later

export async function run({ assert, fixture }) {
  const stylesheet = document.createElement("link");
  stylesheet.rel = "stylesheet";
  stylesheet.href = "/search-pane.css";
  document.head.append(stylesheet);
  await new Promise((resolve, reject) => {
    stylesheet.addEventListener("load", resolve, { once: true });
    stylesheet.addEventListener("error", reject, { once: true });
  });
  const { createSearchPane } = await import("/search-pane.mjs");
  const labels = {
    searchTitle: "Search", searchQuery: "Find text", searchOptions: "Options", searchResults: "Results",
    searchMatchCase: "Match case", searchWholeWord: "Whole word", searchIgnoreAccents: "Ignore accents", searchMatchNbsp: "Match NBSP",
    searchPrevious: "Previous", searchNext: "Next", searchClose: "Close", searchCount: "{count} / {total}",
    searchEmptyQuery: "Enter a search", searchNoResults: "No results", searchTruncated: "(list limited)",
    searchBusy: "Searching", searchError: "Search failed", searchDisabled: "Unavailable", searchWrapped: "Wrapped",
    searchResult: "Result {index}: {excerpt}",
  };
  const queries = [], navigations = [], selections = [];
  let closed = 0;
  const pane = createSearchPane({
    document, labels,
    onQuery: value => queries.push(value),
    onNavigate: direction => navigations.push(direction),
    onSelect: index => selections.push(index),
    onClose: () => { closed++; },
  });
  fixture.append(pane.element);
  const opener = document.createElement("button");
  opener.textContent = "Open search";
  fixture.append(opener);
  opener.focus();
  pane.open({ query: "café", returnFocusTo: opener });
  const input = pane.element.querySelector("[data-search-query]");
  assert(document.activeElement === input && queries.at(-1).query === "café", "open seeds the query, requests results, and focuses the named input");
  assert(pane.element.querySelectorAll("[data-search-option]").length === 4, "pane exposes four named search option controls");

  input.value = "literal <b> café";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  assert(queries.at(-1).query === "literal <b> café", "typed query is passed as text without interpreting markup");
  const wholeWord = pane.element.querySelector('[data-search-option="wholeWord"]');
  wholeWord.checked = true;
  wholeWord.dispatchEvent(new Event("change", { bubbles: true }));
  assert(queries.at(-1).options.wholeWord === true, "option changes issue a query with named option values");

  let beforeComposition = queries.length;
  input.dispatchEvent(new Event("compositionstart", { bubbles: true }));
  input.value = "にほん";
  input.dispatchEvent(new Event("input", { bubbles: true, isComposing: true }));
  assert(queries.length === beforeComposition, "IME composition defers intermediate query changes");
  input.dispatchEvent(new Event("compositionend", { bubbles: true }));
  assert(queries.length === beforeComposition + 1 && queries.at(-1).query === "にほん", "composition end submits the completed Unicode query");
  const enter = key => {
    const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
    input.dispatchEvent(event);
    return event;
  };
  const navItem = { start: 5002, end: 5007, excerpt: { text: "A café <script>alert(1)</script>", start: 2, end: 7 } };
  pane.setState({ items: [navItem], index: 0 });
  const result = pane.element.querySelector("[data-search-result]");
  assert(result.querySelector("mark")?.textContent === "café", "UTF-8 byte ranges create a bounded mark without splitting a multibyte scalar");
  assert(result.textContent === "A café <script>alert(1)</script>" && result.closest('[role="listitem"]'), "excerpt byte offsets are relative to excerpt text and the button sits inside a list item");
  assert(result.textContent.includes("<script>alert(1)</script>") && !result.querySelector("script"), "literal markup in excerpts stays inert text");
  input.focus();
  const plainEnter = enter("Enter");
  assert(plainEnter.defaultPrevented && navigations.at(-1) === 1, "Enter navigates forward from the query input");
  enter("Enter");
  const reverse = new KeyboardEvent("keydown", { key: "Enter", shiftKey: true, bubbles: true, cancelable: true });
  input.dispatchEvent(reverse);
  assert(navigations.at(-1) === -1, "Shift+Enter navigates backward");
  const ctrlAltEnter = new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
  input.dispatchEvent(ctrlAltEnter);
  assert(!ctrlAltEnter.defaultPrevented, "Ctrl+Alt Enter is preserved for AltGr and unrelated editing");
  const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
  pane.element.querySelector("[data-search-close]").dispatchEvent(escape);
  assert(escape.defaultPrevented && pane.element.hidden && document.activeElement === opener && closed === 1, "Escape closes and restores opener focus");

  pane.open({ query: "café", returnFocusTo: opener });
  const sameItems = [navItem, { ...navItem, start: 9002, end: 9007 }];
  pane.setState({ items: sameItems, index: 0, truncated: true });
  input.focus();
  assert(pane.element.querySelector("[data-search-count]").textContent === "1 / 2 — (list limited)", "status reports the current result, total list size, and truncation notice");
  const focusedResult = pane.element.querySelector('[data-search-result="0"]');
  focusedResult.focus();
  pane.setState({ items: sameItems, index: 1, busy: false });
  assert(document.activeElement === focusedResult && focusedResult.getAttribute("aria-current") === "false", "same-reference result updates preserve button focus and refresh current state");
  assert(pane.element.querySelector('[data-search-result="1"]').getAttribute("aria-current") === "true", "the newly selected result is marked current");
  const resultButtons = [...pane.element.querySelectorAll("[data-search-result]")];
  assert(resultButtons.filter(button => button.tabIndex === 0).length === 1 && focusedResult.tabIndex === 0, "result list preserves exactly one roving Tab stop at the focused result");
  const selectedBeforeRoving = selections.length;
  focusedResult.focus();
  const arrowDown = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true });
  focusedResult.dispatchEvent(arrowDown);
  assert(arrowDown.defaultPrevented && document.activeElement === resultButtons[1] && selections.length === selectedBeforeRoving, "ArrowDown moves result focus without selecting or changing editor navigation");
  assert(resultButtons[1].tabIndex === 0 && resultButtons.filter(button => button.tabIndex === 0).length === 1, "the Tab stop follows focus to the newly focused result");
  const arrowUp = new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true });
  resultButtons[1].dispatchEvent(arrowUp);
  assert(arrowUp.defaultPrevented && document.activeElement === resultButtons[0], "ArrowUp wraps to the previous result");
  const home = new KeyboardEvent("keydown", { key: "Home", bubbles: true, cancelable: true });
  resultButtons[0].dispatchEvent(home);
  assert(home.defaultPrevented && document.activeElement === resultButtons[0], "Home focuses the first result");
  const end = new KeyboardEvent("keydown", { key: "End", bubbles: true, cancelable: true });
  resultButtons[0].dispatchEvent(end);
  assert(end.defaultPrevented && document.activeElement === resultButtons[1], "End focuses the last result");
  const enterResult = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
  resultButtons[1].dispatchEvent(enterResult);
  assert(!enterResult.defaultPrevented, "result buttons preserve native Enter activation");
  const spaceResult = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
  resultButtons[1].dispatchEvent(spaceResult);
  assert(!spaceResult.defaultPrevented, "result buttons preserve native Space activation");
  pane.element.querySelector('[data-search-result="1"]').click();
  assert(selections.at(-1) === 1, "result button selects its bounded list index");
  pane.setState({ items: [], index: -1 });
  assert(pane.element.querySelector("[data-search-next]").disabled && pane.element.querySelector("[data-search-previous]").disabled, "navigation is disabled when the result list is empty");
  pane.setState({ items: [navItem], busy: true });
  assert(pane.element.querySelector("[data-search-next]").disabled, "navigation is disabled while a query is busy");
  pane.setState({ busy: false, disabled: true });
  assert(input.disabled && pane.element.querySelector('[data-search-option="matchCase"]').disabled && !pane.element.querySelector("[data-search-close]").disabled, "disabled state blocks search actions while leaving close available");
  const count = queries.length;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  assert(queries.length === count, "disabled query field does not dispatch work");
  pane.setState({ disabled: false, wrapped: true });
  assert(pane.element.querySelector("[data-search-count]").textContent.includes(labels.searchWrapped), "wrap state is announced alongside the current count");
  const compositionStart = new Event("compositionstart", { bubbles: true });
  input.dispatchEvent(compositionStart);
  const composingEscape = new KeyboardEvent("keydown", { key: "Escape", isComposing: true, bubbles: true, cancelable: true });
  pane.element.querySelector("[data-search-close]").dispatchEvent(composingEscape);
  assert(!composingEscape.defaultPrevented && !pane.element.hidden, "Escape during IME composition does not close the pane");
  const imeEscape = new KeyboardEvent("keydown", { key: "Escape", keyCode: 229, bubbles: true, cancelable: true });
  pane.element.querySelector("[data-search-next]").dispatchEvent(imeEscape);
  assert(!imeEscape.defaultPrevented && !pane.element.hidden, "Escape with IME key code 229 does not close the pane");
  input.dispatchEvent(new Event("compositionend", { bubbles: true }));
  const panelEscape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
  pane.element.querySelector("[data-search-close]").dispatchEvent(panelEscape);
  assert(panelEscape.defaultPrevented && pane.element.hidden, "Escape from another pane control closes the panel after composition");
  pane.dispose();
  assert(!pane.element.isConnected, "dispose removes the pane and its listeners");
  stylesheet.remove();
}
