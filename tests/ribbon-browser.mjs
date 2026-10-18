// SPDX-License-Identifier: AGPL-3.0-or-later
import { createRibbon } from "/ribbon.mjs";

/** Browser checks for the reusable ribbon controller. The parent runner supplies a real DOM fixture. */
export async function run({ assert, fixture }) {
  if (typeof assert !== "function" || !fixture?.ownerDocument) throw new TypeError("Ribbon browser checks need assert and a DOM fixture.");
  const document = fixture.ownerDocument;
  const stylesheet = document.createElement("link");
  stylesheet.rel = "stylesheet";
  stylesheet.href = "/ribbon.css";
  document.head.append(stylesheet);
  await new Promise(resolve => {
    const timer = setTimeout(resolve, 2500);
    stylesheet.addEventListener("load", () => { clearTimeout(timer); resolve(); }, { once: true });
    stylesheet.addEventListener("error", () => { clearTimeout(timer); resolve(); }, { once: true });
  });

  const host = document.createElement("div");
  host.className = "ribbon-test-host";
  host.style.width = "320px";
  host.style.maxWidth = "100%";
  const root = document.createElement("nav");
  root.className = "toolbar";
  root.setAttribute("aria-label", "Actions du document");
  const originalGroup = document.createElement("div");
  originalGroup.className = "toolbar-group";
  const newButton = document.createElement("button");
  newButton.id = "new-document";
  newButton.type = "button";
  newButton.textContent = "Nouveau";
  const openButton = document.createElement("button");
  openButton.id = "open-document";
  openButton.type = "button";
  openButton.className = "caller-owned";
  openButton.dataset.ribbonCommandId = "before-ribbon";
  openButton.textContent = "Ouvrir";
  const undoButton = document.createElement("button");
  undoButton.id = "undo";
  undoButton.type = "button";
  undoButton.textContent = "Annuler";
  originalGroup.append(newButton, openButton, undoButton);
  const options = document.createElement("details");
  options.className = "editor-options";
  const summary = document.createElement("summary");
  summary.textContent = "Options";
  options.append(summary);
  const status = document.createElement("span");
  status.id = "editor-status";
  status.className = "editor-status";
  status.textContent = "Document prêt";
  root.append(originalGroup, options, status);
  host.append(root);
  fixture.append(host);

  let nativeActivations = 0;
  let commandCalls = 0;
  const commandCallsById = [];
  newButton.addEventListener("click", () => { nativeActivations += 1; });
  const disabledSpec = { id: "format-bold", label: "Gras", keyTip: "B", disabled: true };
  const tabs = [
    { id: "file", label: "Fichier", keyTip: "F", groups: [{ id: "file-actions", label: "Document", keyTip: "D", commands: [
      { id: "new", label: "Nouveau", keyTip: "N", element: newButton },
      { id: "open", label: "Ouvrir", keyTip: "O", element: openButton, disabled: true },
    ] }] },
    { id: "home", label: "Accueil", keyTip: "A", groups: [
      { id: "home-edit", label: "Édition", keyTip: "E", commands: [
        { id: "undo", label: "Annuler", keyTip: "Z", element: undoButton },
        { id: "format-bold", label: "Gras", keyTip: "B", disabled: true },
        { id: "probe", label: "Action de test", keyTip: "P" },
      ] },
      { id: "home-options", label: "Préférences", commands: [
        { id: "options", label: "Options", keyTip: "Q", element: options },
      ] },
    ] },
    { id: "insert", label: "Insertion", keyTip: "I", groups: [{ id: "insert-tools", label: "Outils", commands: [
      { id: "unsupported-table", label: "Tableau indisponible", disabled: true },
    ] }] },
    { id: "layout", label: "Mise en page", keyTip: "M", groups: [{ id: "page-layout", label: "Page", commands: [
      { id: "unsupported-margins", label: "Marges indisponibles", disabled: true },
    ] }] },
    { id: "references", label: "Références", keyTip: "R", groups: [{ id: "references-tools", label: "Références", commands: [
      { id: "unsupported-citation", label: "Citation indisponible", disabled: true },
    ] }] },
    { id: "view", label: "Affichage", keyTip: "V", groups: [{ id: "view-tools", label: "Affichage", commands: [
      { id: "unsupported-zoom", label: "Zoom indisponible", disabled: true },
    ] }] },
  ];
  // This extra entry deliberately exercises the caller-owned command element contract.
  tabs[1].groups[0].commands[1] = disabledSpec;
  const controller = createRibbon({
    root,
    tabs,
    initialTab: "home",
    labels: { collapse: "Replier le ruban", expand: "Développer le ruban", navigation: "Navigation du ruban" },
    onCommand(id) { commandCalls += 1; commandCallsById.push(id); },
  });

  assert(root.querySelector('[role="tablist"][aria-label="Navigation du ruban"]'), "ribbon exposes a named semantic tablist");
  assert(root.querySelectorAll('[role="tab"]').length === 6, "ribbon renders the six declared tabs");
  assert(root.querySelector('[role="tab"][aria-selected="true"]').dataset.ribbonTabId === "home", "initial tab is selected and synchronized");
  assert(root.querySelector('[role="tabpanel"]:not([hidden])')?.dataset.ribbonPanelId === "home", "selected tab has one labeled visible panel");
  assert(document.getElementById("new-document") === newButton && document.querySelectorAll("#new-document").length === 1,
    "existing command element keeps its ID and identity without cloning");
  assert(root.querySelector(".editor-options") === options && options.closest(".ribbon-group")
    && root.querySelector("#editor-status") === status,
    "supplied details command stays in its declared group while status and preview control remain available");
  assert(openButton.disabled, "supplied command applies an explicit disabled=true spec");
  tabs[0].groups[0].commands[1].disabled = false;
  controller.refresh();
  assert(!openButton.disabled, "refresh applies disabled=false to re-enable a supplied command");
  undoButton.disabled = true;
  openButton.classList.add("caller-update");

  const homeTab = root.querySelector('[data-ribbon-tab-id="home"]');
  homeTab.focus();
  homeTab.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true, cancelable: true }));
  assert(root.querySelector('[role="tab"][aria-selected="true"]').dataset.ribbonTabId === "insert", "right arrow selects the next tab");
  const insertTab = root.querySelector('[data-ribbon-tab-id="insert"]');
  insertTab.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true, cancelable: true }));
  assert(root.querySelector('[role="tab"][aria-selected="true"]').dataset.ribbonTabId === "view", "End selects the last tab");
  assert(controller.selectTab("file") && !controller.selectTab("missing"), "public tab selection reports success and rejects unknown IDs");

  const collapse = root.querySelector(".ribbon-collapse");
  const collapseEvent = new KeyboardEvent("keydown", { key: "F1", ctrlKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(collapseEvent);
  assert(collapseEvent.defaultPrevented && root.dataset.ribbonCollapsed === "true" && collapse.getAttribute("aria-expanded") === "false",
    "Ctrl+F1 collapses the ribbon and updates the accessible state");
  controller.setCollapsed(false);
  assert(root.dataset.ribbonCollapsed === "false" && collapse.getAttribute("aria-expanded") === "true", "public collapse control expands the ribbon");
  controller.setCollapsed(true);
  root.querySelector('[data-ribbon-tab-id="file"]').click();
  assert(root.dataset.ribbonCollapsed === "false", "clicking a tab reveals its panel while the ribbon is collapsed");
  controller.setCollapsed(true);
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true }));
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "f", bubbles: true, cancelable: true }));
  assert(root.dataset.ribbonCollapsed === "false", "key-tip tab selection reveals a collapsed command panel");
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "n", bubbles: true, cancelable: true }));
  assert(nativeActivations === 1, "a command key tip activates after revealing the collapsed panel");

  const altGraphEvent = new KeyboardEvent("keydown", { key: "@", ctrlKey: true, altKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(altGraphEvent);
  assert(!altGraphEvent.defaultPrevented && !root.hasAttribute("data-keytips-active"), "AltGr-style modified input is not intercepted");
  const composingAlt = new KeyboardEvent("keydown", { key: "Alt", altKey: true, isComposing: true, bubbles: true, cancelable: true });
  document.dispatchEvent(composingAlt);
  assert(!composingAlt.defaultPrevented && !root.hasAttribute("data-keytips-active"), "composing input does not start key tips");
  document.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true }));
  const activeAlt = new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(activeAlt);
  assert(!activeAlt.defaultPrevented && !root.hasAttribute("data-keytips-active"), "active IME composition leaves bare Alt untouched");
  document.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true }));

  newButton.focus();
  const alt = new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(alt);
  assert(alt.defaultPrevented && root.dataset.keytipsActive === "tabs" && root.querySelectorAll(".ribbon-keytip").length === 6,
    "bare Alt opens the first key-tip level for all enabled tabs");
  const fileTip = new KeyboardEvent("keydown", { key: "f", bubbles: true, cancelable: true });
  document.dispatchEvent(fileTip);
  assert(fileTip.defaultPrevented && root.dataset.keytipsActive === "commands"
    && root.querySelector('[role="tab"][aria-selected="true"]').dataset.ribbonTabId === "file",
    "first key-tip level selects a tab and displays its second level");
  const newTip = new KeyboardEvent("keydown", { key: "n", bubbles: true, cancelable: true });
  document.dispatchEvent(newTip);
  assert(newTip.defaultPrevented && !root.hasAttribute("data-keytips-active") && nativeActivations === 2 && commandCalls === 0,
    "second-level key tip activates the original command exactly once without routing it through onCommand");

  newButton.focus();
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true }));
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true }));
  const disabledButton = root.querySelector('[data-ribbon-command-id="format-bold"]');
  assert(disabledButton?.disabled && disabledButton.getAttribute("aria-disabled") === null,
    "unsupported command is visibly disabled using native disabled semantics");
  const disabledTip = new KeyboardEvent("keydown", { key: "b", bubbles: true, cancelable: true });
  document.dispatchEvent(disabledTip);
  assert(commandCalls === 0 && !disabledTip.defaultPrevented, "disabled command key tip never dispatches a callback");

  controller.selectTab("home");
  const probeTip = key => document.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }));
  newButton.focus();
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true }));
  probeTip("a");
  probeTip("p");
  assert(commandCalls === 1 && commandCallsById[0] === "probe", "internal command invokes exactly its own onCommand callback");

  options.open = false;
  newButton.focus();
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true }));
  probeTip("a");
  probeTip("q");
  assert(options.open && document.activeElement === summary, "details command key tip focuses and activates its summary");
  options.open = false;

  newButton.focus();
  const focusBeforeEscape = document.activeElement;
  document.dispatchEvent(new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true }));
  const escape = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
  document.dispatchEvent(escape);
  assert(escape.defaultPrevented && !root.hasAttribute("data-keytips-active") && document.activeElement === focusBeforeEscape,
    "Escape closes key tips and restores the focus that preceded Alt");
  const ctrlO = new KeyboardEvent("keydown", { key: "o", ctrlKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(ctrlO);
  assert(!ctrlO.defaultPrevented, "unrelated modified shortcuts pass through the ribbon listener");

  root.style.width = "320px";
  controller.selectTab("home");
  const group = root.querySelector('[role="tabpanel"]:not([hidden]) .ribbon-group');
  assert(getComputedStyle(root.querySelector(".ribbon-commands")).flexWrap === "wrap", "command groups wrap instead of clipping at narrow widths");
  assert(root.scrollWidth <= root.clientWidth + 1, "ribbon stays within the narrow viewport without horizontal shell overflow");
  const modal = document.createElement("dialog");
  document.body.append(modal);
  modal.showModal();
  const callsBeforeModal = commandCalls;
  const modalAlt = new KeyboardEvent("keydown", { key: "Alt", altKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(modalAlt);
  const modalF1 = new KeyboardEvent("keydown", { key: "F1", ctrlKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(modalF1);
  assert(!modalAlt.defaultPrevented && !modalF1.defaultPrevented && !root.hasAttribute("data-keytips-active")
    && root.dataset.ribbonCollapsed === "false" && commandCalls === callsBeforeModal,
    "modal dialog keeps global ribbon shortcuts and command activation outside its scope");
  modal.close();
  modal.remove();
  controller.dispose();
  assert(root.firstElementChild === originalGroup && originalGroup.firstElementChild === newButton
    && root.children[1] === options && root.children[2] === status,
    "dispose restores the caller's original DOM nodes and order");
  assert(openButton.classList.contains("caller-owned") && openButton.classList.contains("caller-update")
    && !openButton.classList.contains("ribbon-command") && openButton.dataset.ribbonCommandId === "before-ribbon" && !openButton.disabled,
    "dispose removes its class, preserves caller class updates, and restores explicitly overridden state and data");
  assert(undoButton.disabled, "dispose preserves business-owned disabled changes when the spec left disabled undefined");
  const afterDispose = new KeyboardEvent("keydown", { key: "F1", ctrlKey: true, bubbles: true, cancelable: true });
  document.dispatchEvent(afterDispose);
  assert(!afterDispose.defaultPrevented && !root.classList.contains("ribbon-root"), "dispose removes global keyboard handling and ribbon state");
  newButton.click();
  assert(nativeActivations === 3, "the original command listener still works after disposal");

  stylesheet.remove();
  host.remove();
}
