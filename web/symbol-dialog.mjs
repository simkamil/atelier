// SPDX-License-Identifier: AGPL-3.0-or-later

import { attachTabs, createDialog } from "./ui-components.mjs";

const PAGE_SIZE = 256;
const MAX_SYMBOLS = 100_000;
const MAX_QUERY_LENGTH = 128;
const MAX_RECENT = 16;
const SPECIALS = Object.freeze([
  [0x2014, "emDash"], [0x2013, "enDash"], [0x00a0, "noBreakSpace"],
  [0x202f, "narrowNoBreakSpace"], [0x2011, "nonBreakingHyphen"],
  [0x00a9, "copyright"], [0x00ae, "registered"], [0x2122, "trademark"],
  [0x00a7, "section"], [0x00b6, "paragraph"], [0x2026, "ellipsis"],
]);

const requiredLabels = ["title", "close", "symbols", "specialCharacters", "search", "block", "allBlocks", "font", "recent", "insert", "noResults", "insertionFailed", "previous", "next", "page"];
const text = value => typeof value === "string" ? value : "";
const scalar = value => Number.isInteger(value) && value >= 0 && value <= 0x10ffff && !(value >= 0xd800 && value <= 0xdfff);
const formatCodePoint = value => `U+${value.toString(16).toUpperCase().padStart(value <= 0xffff ? 4 : 5, "0")}`;
const make = (doc, tag, className, value) => {
  const node = doc.createElement(tag);
  if (className) node.className = className;
  if (value !== undefined) node.textContent = String(value);
  return node;
};

function validateCatalogue(catalogue) {
  if (!catalogue || typeof catalogue.font !== "string" || !catalogue.font || !Array.isArray(catalogue.blocks) || !Array.isArray(catalogue.symbols)
      || catalogue.symbols.length > MAX_SYMBOLS) throw new TypeError("Invalid symbol catalogue.");
  const blocks = new Map();
  for (const block of catalogue.blocks) {
    if (!block || typeof block.id !== "string" || !block.id || typeof block.name !== "string"
        || !scalar(block.start) || !scalar(block.end) || block.start > block.end || blocks.has(block.id)) {
      throw new TypeError("Invalid Unicode block catalogue.");
    }
    blocks.set(block.id, block);
  }
  const symbols = new Map();
  for (const entry of catalogue.symbols) {
    if (!entry || !scalar(entry.codePoint) || typeof entry.name !== "string" || !blocks.has(entry.block)
        || entry.codePoint < blocks.get(entry.block).start || entry.codePoint > blocks.get(entry.block).end || symbols.has(entry.codePoint)) {
      throw new TypeError("Invalid Unicode symbol catalogue.");
    }
    symbols.set(entry.codePoint, Object.freeze({ codePoint: entry.codePoint, name: entry.name, block: entry.block }));
  }
  return { blocks, symbols };
}

/**
 * Create the standalone Unicode symbol picker for the editable TXT surface.
 * `labels.specialNames` maps the eleven keys `emDash`, `enDash`, `noBreakSpace`,
 * `narrowNoBreakSpace`, `nonBreakingHyphen`, `copyright`, `registered`,
 * `trademark`, `section`, `paragraph`, and `ellipsis` to localized names.
 */
export function createSymbolDialog({ labels = {}, catalogue, onInsert } = {}) {
  if (typeof onInsert !== "function") throw new TypeError("onInsert callback is required.");
  const { blocks, symbols } = validateCatalogue(catalogue);
  const doc = globalThis.document;
  if (!doc?.createElement) throw new TypeError("A browser document is required.");
  for (const key of requiredLabels) if (!text(labels[key])) throw new TypeError(`Missing symbol dialog label: ${key}.`);
  for (const [, key] of SPECIALS) if (!text(labels.specialNames?.[key])) throw new TypeError(`Missing localized special symbol name: ${key}.`);
  const localized = Object.fromEntries(SPECIALS.map(([, key]) => [key, text(labels.specialNames?.[key]) || key]));
  const localizedSpecialByCodePoint = new Map(SPECIALS.map(([codePoint, key]) => [codePoint, localized[key]]));
  const byName = new Map([...symbols.values()].map(entry => [entry.codePoint, entry]));
  const specialEntries = SPECIALS.map(([codePoint, key]) => byName.get(codePoint) ?? Object.freeze({
    codePoint, name: localized[key], block: blocks.get(catalogue.blocks.find(block => codePoint >= block.start && codePoint <= block.end)?.id)?.id ?? "",
  }));

  const content = make(doc, "div", "symbol-dialog");
  const tablist = make(doc, "div", "symbol-dialog-tabs");
  const symbolsTab = make(doc, "button", "symbol-dialog-tab", labels.symbols); symbolsTab.type = "button";
  const specialTab = make(doc, "button", "symbol-dialog-tab", labels.specialCharacters); specialTab.type = "button";
  tablist.append(symbolsTab, specialTab);
  const symbolsPanel = make(doc, "section", "symbol-dialog-panel");
  const specialPanel = make(doc, "section", "symbol-dialog-panel");
  const font = make(doc, "p", "symbol-dialog-font");
  font.append(make(doc, "span", "symbol-dialog-font-label", `${labels.font}: `), make(doc, "span", "symbol-dialog-font-name", catalogue.font));

  const searchLabel = make(doc, "label", "symbol-dialog-field-label", labels.search);
  const search = make(doc, "input", "symbol-dialog-search"); search.type = "search"; search.maxLength = MAX_QUERY_LENGTH; search.autocomplete = "off";
  search.dataset.symbolSearch = ""; searchLabel.append(search);
  const blockLabel = make(doc, "label", "symbol-dialog-field-label", labels.block);
  const blockSelect = make(doc, "select", "symbol-dialog-block"); blockSelect.dataset.symbolBlock = "";
  const allOption = make(doc, "option", "", labels.allBlocks); allOption.value = ""; blockSelect.append(allOption);
  for (const block of catalogue.blocks) { const option = make(doc, "option", "", block.name); option.value = block.id; blockSelect.append(option); }
  blockLabel.append(blockSelect);
  const controls = make(doc, "div", "symbol-dialog-controls"); controls.append(searchLabel, blockLabel);
  const recentHeading = make(doc, "h3", "symbol-dialog-section-title", labels.recent);
  const recentList = make(doc, "div", "symbol-dialog-recent"); recentList.setAttribute("role", "grid"); recentList.setAttribute("aria-label", labels.recent); recentList.tabIndex = -1;
  recentList.dataset.symbolGrid = "recent";
  const results = make(doc, "div", "symbol-dialog-grid"); results.setAttribute("role", "grid"); results.setAttribute("aria-label", labels.symbols); results.tabIndex = -1;
  results.dataset.symbolGrid = "results";
  const status = make(doc, "p", "symbol-dialog-status"); status.setAttribute("role", "status"); status.setAttribute("aria-live", "polite");
  const insertionStatus = make(doc, "p", "symbol-dialog-insertion-status"); insertionStatus.setAttribute("role", "status"); insertionStatus.setAttribute("aria-live", "assertive");
  const pager = make(doc, "div", "symbol-dialog-pager");
  const previous = make(doc, "button", "symbol-dialog-page-button", labels.previous); previous.type = "button"; previous.dataset.symbolPrevious = "";
  const pageStatus = make(doc, "span", "symbol-dialog-page-status");
  const next = make(doc, "button", "symbol-dialog-page-button", labels.next); next.type = "button"; next.dataset.symbolNext = "";
  pager.append(previous, pageStatus, next);
  symbolsPanel.append(font, controls, recentHeading, recentList, results, status, insertionStatus, pager);

  const specialHeading = make(doc, "h3", "symbol-dialog-section-title", labels.specialCharacters);
  const specialGrid = make(doc, "div", "symbol-dialog-grid symbol-dialog-special-grid"); specialGrid.setAttribute("role", "grid"); specialGrid.setAttribute("aria-label", labels.specialCharacters); specialGrid.tabIndex = -1;
  specialGrid.dataset.symbolGrid = "special";
  specialPanel.append(font.cloneNode(true), specialHeading, specialGrid);
  content.append(tablist, symbolsPanel, specialPanel);
  const dialog = createDialog({ label: labels.title, closeLabel: labels.close, content, initialFocus: () => mode === "special"
    ? specialGrid.querySelector("[role=gridcell]") ?? specialTab : search });
  dialog.element.classList.add("symbol-dialog-shell");

  let selected = null;
  let page = 0;
  let recent = [];
  let pending = false;
  let disposed = false;
  let mode = "symbols";
  let matching = [];
  let initialized = false;
  const activeCodePoints = new Map();
  const tabController = attachTabs({ tablist, tabs: [
    { tab: symbolsTab, panel: symbolsPanel }, { tab: specialTab, panel: specialPanel },
  ], label: labels.title, onSelect(index) { mode = index === 0 ? "symbols" : "special"; if (initialized) renderCurrent(); } });
  const description = entry => `${entry.name || localizedSpecialByCodePoint.get(entry.codePoint) || ""} (${formatCodePoint(entry.codePoint)})`;
  const choose = entry => {
    if (!entry || !scalar(entry.codePoint)) return;
    selected = entry;
    insertionStatus.textContent = "";
    for (const button of content.querySelectorAll("[data-symbol-codepoint]")) {
      const active = Number(button.dataset.symbolCodepoint) === entry.codePoint;
      button.setAttribute("aria-pressed", String(active));
      button.classList.toggle("is-selected", active);
    }
    insertButton.disabled = pending;
  };
  const insertButton = make(doc, "button", "symbol-dialog-insert", labels.insert); insertButton.type = "button"; insertButton.dataset.symbolInsert = "";
  const insertRow = make(doc, "div", "symbol-dialog-insert-row"); insertRow.append(insertButton);
  symbolsPanel.append(insertRow); specialPanel.append(insertRow.cloneNode(true));
  // Both panels share one action button; keep it in the active panel when tabs switch.
  const specialInsert = specialPanel.querySelector("[data-symbol-insert]");
  const currentInsertButton = () => mode === "special" ? specialInsert : insertButton;

  function gridColumns(grid) {
    const columns = doc.defaultView?.getComputedStyle(grid).gridTemplateColumns;
    if (columns && columns !== "none" && !columns.includes("(")) return columns.trim().split(/\s+/).length;
    if (doc.defaultView?.matchMedia("(max-width: 580px)").matches) return 4;
    return grid.classList.contains("symbol-dialog-special-grid") ? 6 : 8;
  }
  function renderCell(entry, parent, rowIndex, columnIndex) {
    const cell = make(doc, "button", "symbol-dialog-cell"); cell.type = "button"; cell.setAttribute("role", "gridcell");
    cell.dataset.symbolCodepoint = String(entry.codePoint);
    cell.setAttribute("aria-rowindex", String(rowIndex)); cell.setAttribute("aria-colindex", String(columnIndex));
    cell.setAttribute("aria-label", description(entry)); cell.setAttribute("aria-pressed", String(selected?.codePoint === entry.codePoint));
    cell.tabIndex = -1;
    const glyph = make(doc, "span", "symbol-dialog-glyph", String.fromCodePoint(entry.codePoint)); glyph.setAttribute("aria-hidden", "true"); glyph.style.fontFamily = `"${catalogue.font.replace(/["\\]/g, "\\$&")}"`;
    const code = make(doc, "span", "symbol-dialog-code", formatCodePoint(entry.codePoint));
    cell.append(glyph, code);
    cell.addEventListener("click", () => { choose(entry); activeCodePoints.set(parent.closest("[data-symbol-grid]").dataset.symbolGrid, entry.codePoint); cell.focus(); renderCurrent(); });
    parent.append(cell);
    return cell;
  }
  function renderGrid(entries, grid) {
    const gridKey = grid.dataset.symbolGrid;
    const focused = grid.contains(doc.activeElement) ? Number(doc.activeElement.dataset.symbolCodepoint) : null;
    const wanted = focused ?? activeCodePoints.get(gridKey);
    const columns = Math.max(1, gridColumns(grid));
    const rowCount = Math.ceil(entries.length / columns);
    grid.replaceChildren();
    grid.setAttribute("aria-colcount", String(columns));
    grid.setAttribute("aria-rowcount", String(rowCount));
    const rendered = new Map();
    for (let offset = 0; offset < entries.length; offset += columns) {
      const rowIndex = Math.floor(offset / columns) + 1;
      const row = make(doc, "div", "symbol-dialog-row"); row.setAttribute("role", "row"); row.setAttribute("aria-rowindex", String(rowIndex));
      const rowEntries = entries.slice(offset, offset + columns);
      rowEntries.forEach((entry, index) => rendered.set(entry.codePoint, renderCell(entry, row, rowIndex, index + 1)));
      grid.append(row);
    }
    const active = wanted !== undefined && wanted !== null && rendered.has(wanted) ? wanted : entries[0]?.codePoint;
    if (active !== undefined) {
      activeCodePoints.set(gridKey, active);
      rendered.get(active).tabIndex = 0;
    } else activeCodePoints.delete(gridKey);
    if (focused !== null && rendered.has(focused)) rendered.get(focused).focus();
    return { columns, active };
  }
  function addRecent(entry) {
    recent = [entry, ...recent.filter(item => item.codePoint !== entry.codePoint)].slice(0, MAX_RECENT);
    renderRecent();
  }
  function renderRecent() {
    renderGrid(recent, recentList);
  }
  function parseCodeQuery(query) {
    const normalized = query.trim();
    const match = normalized.match(/^(?:u\+|0x)([0-9a-f]{1,6})$/i);
    if (!match) return null;
    const value = Number.parseInt(match[1], 16);
    return scalar(value) ? value : -1;
  }
  function renderSymbols() {
    const query = search.value.slice(0, MAX_QUERY_LENGTH).trim().toLowerCase();
    const code = parseCodeQuery(query);
    const blockId = blockSelect.value;
    matching = [];
    if (typeof code === "number" && code >= 0) {
      const entry = symbols.get(code) ?? specialEntries.find(item => item.codePoint === code);
      if (entry && (!blockId || entry.block === blockId)) matching.push(entry);
    } else if (code === null) {
      for (const entry of symbols.values()) {
        if ((!blockId || entry.block === blockId)
          && `${entry.name} ${formatCodePoint(entry.codePoint)} ${entry.codePoint.toString(16)}`.toLowerCase().includes(query)) matching.push(entry);
      }
    }
    const pages = Math.max(1, Math.ceil(matching.length / PAGE_SIZE));
    page = Math.min(page, pages - 1);
    const visible = matching.slice(page * PAGE_SIZE, (page + 1) * PAGE_SIZE);
    renderGrid(visible, results);
    status.textContent = visible.length ? "" : labels.noResults;
    pageStatus.textContent = labels.page.replace("{current}", String(page + 1)).replace("{total}", String(pages));
    previous.disabled = page <= 0; next.disabled = page >= pages - 1;
    insertButton.disabled = pending || !selected;
  }
  function renderSpecials() {
    renderGrid(specialEntries, specialGrid);
    specialInsert.disabled = pending || !selected || !specialEntries.some(entry => entry.codePoint === selected.codePoint);
  }
  function renderCurrent() {
    renderSymbols(); renderSpecials(); renderRecent();
  }
  const onSearch = () => { page = 0; activeCodePoints.delete("results"); renderCurrent(); };
  const onBlock = () => { page = 0; activeCodePoints.delete("results"); renderSymbols(); };
  const onPrevious = () => { if (page > 0) { page--; activeCodePoints.delete("results"); renderSymbols(); results.querySelector("[role=gridcell]")?.focus(); } };
  const onNext = () => { if ((page + 1) * PAGE_SIZE < matching.length) { page++; activeCodePoints.delete("results"); renderSymbols(); results.querySelector("[role=gridcell]")?.focus(); } };
  const onGridKey = event => {
    const cells = [...event.currentTarget.querySelectorAll("[role=gridcell]")];
    if (!cells.length) return;
    const current = Math.max(0, cells.indexOf(doc.activeElement));
    const columns = gridColumns(event.currentTarget);
    let target = current;
    if (event.key === "ArrowRight") target = Math.min(current + 1, cells.length - 1);
    else if (event.key === "ArrowLeft") target = Math.max(0, current - 1);
    else if (event.key === "ArrowDown") target = Math.min(current + columns, cells.length - 1);
    else if (event.key === "ArrowUp") target = Math.max(0, current - columns);
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = cells.length - 1;
    else return;
    event.preventDefault();
    activeCodePoints.set(event.currentTarget.dataset.symbolGrid, Number(cells[target].dataset.symbolCodepoint));
    cells.forEach((cell, index) => { cell.tabIndex = index === target ? 0 : -1; });
    cells[target].focus();
  };
  const onDialogKey = event => {
    if (event.key !== "Escape" || event.isComposing || event.keyCode === 229) return;
    event.preventDefault();
    dialog.close();
  };
  async function commit() {
    const button = currentInsertButton();
    if (!selected || pending || button.disabled || disposed) return;
    const choice = selected;
    pending = true; insertButton.disabled = true; specialInsert.disabled = true;
    insertionStatus.textContent = "";
    try {
      const accepted = await onInsert(String.fromCodePoint(choice.codePoint), Object.freeze({ codePoint: choice.codePoint, name: choice.name, block: choice.block, font: catalogue.font }));
      if (accepted === false) { insertionStatus.textContent = labels.insertionFailed; return; }
      if (disposed) return;
      addRecent(choice);
      dialog.close();
    } catch {
      // A rejected parent edit leaves the chosen scalar available for retry.
      insertionStatus.textContent = labels.insertionFailed;
    } finally {
      pending = false;
      if (!disposed) { insertButton.disabled = !selected; specialInsert.disabled = !selected || !specialEntries.some(entry => entry.codePoint === selected.codePoint); }
    }
  }
  search.addEventListener("input", onSearch);
  blockSelect.addEventListener("change", onBlock);
  previous.addEventListener("click", onPrevious); next.addEventListener("click", onNext);
  results.addEventListener("keydown", onGridKey); recentList.addEventListener("keydown", onGridKey); specialGrid.addEventListener("keydown", onGridKey);
  dialog.element.addEventListener("keydown", onDialogKey);
  insertButton.addEventListener("click", commit); specialInsert.addEventListener("click", commit);
  initialized = true;
  renderCurrent();
  const onResize = () => { if (!disposed && dialog.element.open) renderCurrent(); };
  doc.defaultView?.addEventListener("resize", onResize);

  const open = options => {
    if (disposed) return;
    dialog.open(options);
    renderCurrent();
    const target = mode === "special" ? specialGrid.querySelector("[role=gridcell]") ?? specialTab : search;
    target.focus();
  };
  const close = () => { if (disposed) return; dialog.close(); };
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    doc.defaultView?.removeEventListener("resize", onResize);
    search.removeEventListener("input", onSearch); blockSelect.removeEventListener("change", onBlock);
    previous.removeEventListener("click", onPrevious); next.removeEventListener("click", onNext);
    results.removeEventListener("keydown", onGridKey); recentList.removeEventListener("keydown", onGridKey); specialGrid.removeEventListener("keydown", onGridKey);
    dialog.element.removeEventListener("keydown", onDialogKey);
    insertButton.removeEventListener("click", commit); specialInsert.removeEventListener("click", commit);
    tabController.dispose(); dialog.dispose();
  };
  return Object.freeze({ element: dialog.element, open, close, dispose, getRecentSymbols: () => recent.map(entry => entry.codePoint) });
}
