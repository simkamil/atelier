// SPDX-License-Identifier: AGPL-3.0-or-later
export async function checkNavigation({ app, assert, waitFor, key, inputText, openText }) {
  const queryNode = document.querySelector("[data-search-query]");
  const slot = document.querySelector("#search-slot");
  const status = document.querySelector("[data-statistics-status]");
  const query = value => { queryNode.value = value; queryNode.dispatchEvent(new Event("input",{bubbles:true})); };
  const option = (name,value) => {
    const node = document.querySelector(`[data-search-option="${name}"]`);
    node.checked = value; node.dispatchEvent(new Event("change",{bubbles:true}));
  };
  const matches = async count => waitFor(() => app.navigation.searchResult?.items.length === count, `${count} real search hits`);
  const text = "café CAFÉ caféine café\nStraße STRASSE\nA\u00a0B A B <script>alerte</script>";
  await openText(text);
  const unchanged = app.getState();
  const direct = await app.editor.search("café",{});
  const statistics = await app.editor.statistics();
  assert(direct.items.length === 4 && statistics.document.pages === app.getState().pages.length
      && app.getState() === unchanged && !unchanged.dirty && !unchanged.canUndo,
    "real Worker search/statistics queries preserve exact TXT state, selection, dirty and undo history");
  key(app.editor.element,"f",{ctrlKey:true});
  assert(!slot.hidden && document.activeElement === queryNode, "Ctrl+F opens the actual named search pane and focuses its query");
  query("café"); await matches(4);
  const first = app.navigation.searchResult.items[0];
  assert(first.start === 0 && first.end === 5 && first.rects.length > 0,
    "Unicode matches carry exact UTF-8 byte ranges and real cached-layout geometry");
  const canvas = document.querySelector("#document-canvas");
  const warmPixels = () => {
    const pixels = canvas.getContext("2d").getImageData(0,0,canvas.width,canvas.height).data;
    let count = 0;
    for (let i=0;i<pixels.length;i+=4) if (pixels[i]>220 && pixels[i+1]>150 && pixels[i+2]<200 && pixels[i+3]===255) count++;
    return count;
  };
  assert(warmPixels() > 20, "all current search hits produce visible warm Canvas highlight pixels");
  app.editor.element.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
  app.editor.element.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "" }));
  assert(warmPixels() > 20, "cancelled composition preserves current search highlights without waiting for caret blink");
  key(queryNode,"Enter");
  await waitFor(() => app.getState().anchor===first.start && app.getState().focus===first.end,"first search navigation selection");
  assert(document.activeElement===queryNode && !app.getState().dirty && !app.getState().canUndo,
    "Enter selects and scrolls to a hit without stealing query focus or editing history");
  key(queryNode,"Enter",{shiftKey:true});
  const last = app.navigation.searchResult.items.at(-1);
  await waitFor(() => app.getState().anchor===last.start && app.getState().focus===last.end,"reverse search wrap");
  assert(document.querySelector("[data-search-count]").textContent.includes("repris"), "reverse search wrap is announced");
  key(queryNode,"Enter"); key(queryNode,"Enter");
  const second = app.navigation.searchResult.items[1];
  await waitFor(() => app.getState().anchor===second.start && app.getState().focus===second.end,"rapid sequential search navigation");
  assert(app.getState().anchor===second.start, "rapid Enter presses advance successively through the asynchronous Worker");
  option("wholeWord",true); await matches(3);
  assert(app.navigation.searchResult.items.every(item => new TextDecoder().decode(app.getState().textBytes.subarray(item.start,item.end)).toLowerCase()==="café"),
    "whole-word option excludes the caféine substring using Rust Unicode rules");
  option("matchCase",true); await matches(2);
  assert(app.navigation.searchResult.items.every(item => new TextDecoder().decode(app.getState().textBytes.subarray(item.start,item.end))==="café"),
    "case-sensitive search preserves exact original spelling");
  option("matchCase",false); option("wholeWord",false); option("ignoreAccents",true); query("cafe"); await matches(4);
  assert(app.navigation.searchResult.items[0].end===5, "accent folding maps back to original UTF-8 bytes");
  query("strasse"); await matches(2);
  assert(app.navigation.searchResult.items.some(item => new TextDecoder().decode(app.getState().textBytes.subarray(item.start,item.end))==="Straße"),
    "full Unicode case folding finds Straße from strasse in the real engine");
  query("A B"); await matches(2);
  option("matchNbsp",true); await matches(1);
  assert(new TextDecoder().decode(app.getState().textBytes.subarray(app.navigation.searchResult.items[0].start,app.navigation.searchResult.items[0].end))==="A B",
    "NBSP option distinguishes nonbreaking and ordinary spaces");
  query("<script>"); await matches(1);
  assert(document.querySelector("[data-search-result]").textContent.includes("<script>")
      && !document.querySelector(".search-pane-results script"), "actual search excerpts preserve literal markup as inert text");
  query("absent"); await matches(0);
  assert(document.querySelector("[data-search-count]").textContent.includes("Aucun"), "empty search results have a clear localized message");
  query("");
  await waitFor(() => app.navigation.searchResult===null,"empty query clears results");
  assert(warmPixels()===0, "empty query removes all Canvas search highlights synchronously");
  query("café"); await matches(4);
  app.editor.setZoom(1.5);
  const scroll = document.querySelector("#document-scroll");
  scroll.scrollTop=0; scroll.scrollLeft=0; scroll.dispatchEvent(new Event("scroll"));
  assert(warmPixels()>20, "search highlights remain correctly painted after zoom and scrolling back to hits");
  key(queryNode,"Escape");
  assert(slot.hidden && document.activeElement===app.editor.element && warmPixels()===0,
    "Escape closes the real pane, removes highlights and restores text input focus");

  await openText("un deux\nété quatre");
  await waitFor(() => status.textContent==="4 mots","document word status");
  const found = await app.editor.search("deux",{wholeWord:true});
  await app.editor.selectSearchResult(found,0);
  await waitFor(() => status.textContent==="1 / 4 mots","selection word status");
  status.click();
  const dialog=document.querySelector("[data-statistics-dialog]");
  await waitFor(() => dialog.open && dialog.querySelector('[data-statistics-count="document:words"]').textContent==="4"
      && !dialog.querySelector(".statistics-dialog-status").textContent,"statistics dialog real snapshot");
  assert(dialog.querySelector('[data-statistics-count="selection:words"]').textContent==="1"
      && dialog.querySelector('[data-statistics-count="document:lines"]').textContent==="2"
      && dialog.querySelector('[data-statistics-count="document:pages"]').textContent==="1"
      && dialog.querySelector('[data-statistics-count="document:paragraphs"]').textContent==="2",
    "statistics dialog maps real document and selected word counts plus actual lines/pages/paragraphs");
  key(dialog.querySelector("button"),"f",{ctrlKey:true});
  assert(slot.hidden, "statistics modal blocks Ctrl+F behind it");
  dialog.querySelector(".ui-dialog-close").click();
  app.editor.focus(); inputText(app.editor.element,"TROIS");
  await waitFor(() => app.getState().text.includes("TROIS"),"edit after statistics");
  await app.editor.undo();
  assert(app.getState().text==="un deux\nété quatre", "statistics and result selection do not interfere with edit undo");

  await openText("a ".repeat(1100));
  app.navigation.openSearch(); query("a"); await matches(1000);
  assert(app.navigation.searchResult.truncated && document.querySelector("[data-search-count]").textContent.includes("1 000"),
    "dense search remains bounded to 1000 hits and announces truncation");
  query("aa"); query("a"); query("absent"); await matches(0);
  assert(app.navigation.searchResult.items.length===0, "rapid query changes leave only the latest real Worker results");
  key(queryNode,"Escape");
}
