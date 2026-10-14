// SPDX-License-Identifier: AGPL-3.0-or-later
import { createSymbolDialog } from "../web/symbol-dialog.mjs";

const labels = Object.freeze({
  title: "Symbol", close: "Close", symbols: "Symbols", specialCharacters: "Special characters",
  search: "Search by name or code", block: "Unicode block", allBlocks: "All blocks", font: "Font", recent: "Recent",
  insert: "Insert", noResults: "No results", insertionFailed: "Insertion failed. Keep the selected symbol and try again.",
  previous: "Previous", next: "Next", page: "{current} / {total}",
  specialNames: {
    emDash: "Em dash", enDash: "En dash", noBreakSpace: "No-break space", narrowNoBreakSpace: "Narrow no-break space",
    nonBreakingHyphen: "Non-breaking hyphen", copyright: "Copyright", registered: "Registered", trademark: "Trademark",
    section: "Section", paragraph: "Paragraph", ellipsis: "Ellipsis",
  },
});

function makeCatalogue(count = 300) {
  const symbols = [];
  for (let index = 0; index < count; index++) {
    const codePoint = 0x1000 + index;
    symbols.push({ codePoint, name: index === 0 ? "GEORGIAN LETTER AN" : `TEST CHARACTER ${index}`, block: "test" });
  }
  for (const [codePoint, name] of [[0x00a0, "NO-BREAK SPACE"], [0x00a9, "COPYRIGHT SIGN"], [0x1f642, "SLIGHTLY SMILING FACE"]]) {
    symbols.push({ codePoint, name, block: codePoint === 0x1f642 ? "emoji" : "special" });
  }
  return {
    font: "Carlito",
    blocks: [
      { id: "test", name: "Test Block", start: 0x1000, end: 0x11ff },
      { id: "special", name: "Specials", start: 0, end: 0x2fff },
      { id: "emoji", name: "Emoticons", start: 0x1f600, end: 0x1f64f },
    ],
    symbols,
  };
}

export async function run({ assert, fixture }) {
  const stylesheet = document.createElement("link"); stylesheet.rel = "stylesheet"; stylesheet.href = "/symbol-dialog.css"; document.head.append(stylesheet);
  const opener = document.createElement("button"); opener.textContent = "Open picker"; fixture.append(opener);
  const callbacks = [];
  let outcome = false;
  const picker = createSymbolDialog({
    labels,
    catalogue: makeCatalogue(),
    onInsert: async (value, metadata) => { callbacks.push({ value, metadata }); if (outcome === "reject") throw new Error("refused"); return outcome; },
  });
  fixture.append(picker.element);
  assert(picker.element.classList.contains("symbol-dialog-shell"), "picker uses the isolated shell class");

  picker.open({ returnFocusTo: opener });
  const search = picker.element.querySelector("[data-symbol-search]");
  const block = picker.element.querySelector("[data-symbol-block]");
  const grid = picker.element.querySelector(".symbol-dialog-grid");
  assert(document.activeElement === search, "opening focuses the symbol search field");
  assert(grid.querySelectorAll("[role=gridcell]").length === 256, "visible symbol page is capped at 256 cells");
  assert(picker.element.querySelector("[data-symbol-next]").disabled === false, "additional symbols expose an explicit next page");
  assert(picker.element.querySelector("[data-symbol-block] option").textContent === labels.allBlocks, "all-blocks option has its own injected label");
  assert(grid.querySelector("[role=row] [role=gridcell]"), "gridcells are nested in accessible rows");
  assert(Number(grid.getAttribute("aria-colcount")) === getColumns(grid)
    && Number(grid.querySelector("[role=gridcell]").getAttribute("aria-colindex")) === 1, "grid row and column metadata matches the rendered grid layout");
  picker.element.querySelector("[data-symbol-next]").click();
  assert(picker.element.querySelector(".symbol-dialog-page-status").textContent === "2 / 2", "pagination reports the current page and total");
  picker.element.querySelector("[data-symbol-previous]").click();

  block.value = "test"; block.dispatchEvent(new Event("change", { bubbles: true }));
  search.value = "not-a-character"; search.dispatchEvent(new Event("input", { bubbles: true }));
  assert(grid.querySelectorAll("[role=gridcell]").length === 0 && picker.element.querySelector(".symbol-dialog-status").textContent === labels.noResults
    && picker.element.querySelector("[data-symbol-insert]").disabled, "no-results state is announced and cannot insert without a choice");
  search.value = "letter an"; search.dispatchEvent(new Event("input", { bubbles: true }));
  assert(grid.querySelectorAll("[role=gridcell]").length === 1 && grid.textContent.includes("U+1000"), "name search is case-insensitive and respects the selected block");
  const target = grid.querySelector("[role=gridcell]"); target.click();
  assert(document.activeElement.dataset.symbolCodepoint === "4096", "repainting after a symbol click restores focus by code point");
  assert(picker.element.querySelector("[data-symbol-insert]").disabled === false, "choosing a scalar enables insertion");

  block.value = ""; block.dispatchEvent(new Event("change", { bubbles: true }));
  search.value = "FACE"; search.dispatchEvent(new Event("input", { bubbles: true }));
  assert(grid.querySelectorAll("[role=gridcell]").length === 1 && grid.textContent.includes("U+1F642"), "unprefixed hex-like English names are searched as names");
  const lowerLocale = String.prototype.toLocaleLowerCase;
  String.prototype.toLocaleLowerCase = function () { return this.replace(/I/g, "ı").toLowerCase(); };
  search.value = "smiling face"; search.dispatchEvent(new Event("input", { bubbles: true }));
  String.prototype.toLocaleLowerCase = lowerLocale;
  assert(grid.querySelectorAll("[role=gridcell]").length === 1 && grid.textContent.includes("U+1F642"), "Unicode name search is independent of locale-sensitive lowercase behavior");
  search.value = "U+2011"; search.dispatchEvent(new Event("input", { bubbles: true }));
  assert(grid.querySelectorAll("[role=gridcell]").length === 1 && grid.textContent.includes("U+2011"), "explicit codepoint search exposes the known non-breaking hyphen despite missing font cmap coverage");
  search.value = "U+10FFFF"; search.dispatchEvent(new Event("input", { bubbles: true }));
  assert(grid.querySelectorAll("[role=gridcell]").length === 0, "explicit queries do not expose arbitrary unsupported scalars");
  search.value = "U+1F642"; search.dispatchEvent(new Event("input", { bubbles: true }));
  assert(grid.querySelectorAll("[role=gridcell]").length === 1 && grid.textContent.includes("U+1F642"), "code-point search accepts non-BMP scalars");
  grid.querySelector("[role=gridcell]").click();
  await picker.element.querySelector("[data-symbol-insert]").click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert(callbacks.length === 1 && callbacks[0].value === "🙂" && callbacks[0].metadata.codePoint === 0x1f642, "async refusal receives the exact multibyte scalar once");
  assert(picker.getRecentSymbols().length === 0 && picker.element.open, "refused insertion keeps the choice and does not record a recent symbol");
  assert(picker.element.querySelector(".symbol-dialog-insertion-status").textContent === labels.insertionFailed, "callback false announces insertion failure inside the dialog");
  assert(picker.element.querySelector("[data-symbol-insert]").disabled === false, "choice remains enabled for retry after callback refusal");
  outcome = "reject";
  await picker.element.querySelector("[data-symbol-insert]").click(); await new Promise(resolve => setTimeout(resolve, 0));
  assert(picker.element.querySelector(".symbol-dialog-insertion-status").textContent === labels.insertionFailed
    && picker.element.querySelector('[data-symbol-codepoint="128578"]').getAttribute("aria-pressed") === "true"
    && picker.getRecentSymbols().length === 0, "rejected callback announces failure and retains the selected choice without recording a recent");

  const tabs = picker.element.querySelectorAll("[role=tab]");
  tabs[0].focus();
  const tabArrow = new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }); tabs[0].dispatchEvent(tabArrow);
  assert(tabArrow.defaultPrevented && document.activeElement === tabs[1] && tabs[1].getAttribute("aria-selected") === "true", "tabs support orientation-aware keyboard focus and automatic activation");
  const specialGrid = picker.element.querySelector(".symbol-dialog-special-grid");
  const specialCells = [...specialGrid.querySelectorAll("[role=gridcell]")];
  specialCells[0].focus();
  const down = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }); specialCells[0].dispatchEvent(down);
  assert(down.defaultPrevented && document.activeElement === specialCells[getColumns(specialGrid)], "special-grid vertical arrows use the rendered six-column layout");
  const nbsp = specialGrid.querySelector('[data-symbol-codepoint="160"]');
  assert(nbsp && nbsp.getAttribute("aria-label").includes("U+00A0"), "special tab preserves and labels the NBSP scalar");
  nbsp.click();
  outcome = true;
  await picker.element.querySelectorAll("[data-symbol-insert]")[1].click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert(callbacks.length === 3 && callbacks.at(-1).value === "\u00a0", "special insertion passes the actual non-breaking space");
  assert(picker.getRecentSymbols()[0] === 0x00a0 && picker.element.open === false, "successful insertion records the recent scalar and closes");

  picker.open({ returnFocusTo: opener });
  const specialNbsp = picker.element.querySelector('.symbol-dialog-special-grid [data-symbol-codepoint="160"]');
  specialNbsp.click(); await picker.element.querySelectorAll("[data-symbol-insert]")[1].click();
  await new Promise(resolve => setTimeout(resolve, 0));
  assert(picker.getRecentSymbols().filter(codePoint => codePoint === 0x00a0).length === 1, "recent symbols are deduplicated with the newest occurrence first");

  for (let index = 0; index < 18; index++) {
    picker.open({ returnFocusTo: opener });
    picker.element.querySelectorAll("[role=tab]")[0].click();
    search.value = `U+${(0x1000 + index).toString(16).toUpperCase()}`;
    search.dispatchEvent(new Event("input", { bubbles: true }));
    const cell = grid.querySelector("[role=gridcell]");
    assert(Boolean(cell), `recent-symbol insertion fixture ${index} exists`);
    cell.click(); insertButtonFor(picker, 0).click();
    await new Promise(resolve => setTimeout(resolve, 0));
  }
  const recent = picker.getRecentSymbols();
  assert(recent.length === 16 && recent[0] === 0x1011 && !recent.includes(0x1000), "session recents retain at most the newest sixteen symbols");

  picker.open({ returnFocusTo: opener });
  const recentGrid = picker.element.querySelector(".symbol-dialog-recent");
  const recentCells = [...recentGrid.querySelectorAll("[role=gridcell]")];
  recentCells[0].focus();
  const recentDown = new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }); recentCells[0].dispatchEvent(recentDown);
  assert(recentDown.defaultPrevented && document.activeElement === recentCells[getColumns(recentGrid)], "recent-grid keyboard navigation uses its rendered columns");
  recentCells[getColumns(recentGrid)].click();
  const resultCells = [...grid.querySelectorAll("[role=gridcell]")];
  assert(resultCells.filter(cell => cell.tabIndex === 0).length === 1 && resultCells[0].tabIndex === 0,
    "recent-grid active position does not leak into the results grid");

  picker.open({ returnFocusTo: opener });
  assert(document.activeElement === search, "reopening restores focus to the search field in the active symbols tab");
  tabs[1].click();
  picker.close();
  picker.open({ returnFocusTo: opener });
  assert(specialGrid.contains(document.activeElement), "reopening with the special tab active focuses a visible special-grid cell");
  const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
  picker.element.dispatchEvent(escape);
  assert(escape.defaultPrevented && !picker.element.open && document.activeElement === opener, "Escape closes and restores opener focus");
  picker.dispose();
  assert(!picker.element.isConnected, "dispose removes the dialog and listeners");
  stylesheet.remove();
  opener.remove();

  let invalid = false;
  try { createSymbolDialog({ labels, catalogue: { font: "Carlito", blocks: [], symbols: [{ codePoint: 0xd800, name: "SURROGATE", block: "x" }] }, onInsert() {} }); }
  catch (error) { invalid = error instanceof TypeError; }
  assert(invalid, "invalid surrogate codepoints are rejected at catalogue validation");
}

function insertButtonFor(picker, index) { return picker.element.querySelectorAll("[data-symbol-insert]")[index]; }
function getColumns(grid) { return getComputedStyle(grid).gridTemplateColumns.trim().split(/\s+/).length; }
