// SPDX-License-Identifier: AGPL-3.0-or-later
import { attachTabs } from "/ui-components.mjs";

let ribbonSequence = 0;

const text = value => String(value ?? "");
const focusableSelector = 'button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),summary,a[href],[tabindex]:not([tabindex="-1"])';
const interactionTarget = element => {
  if (!element) return null;
  if (element.matches?.(focusableSelector)) return element;
  return element.querySelector?.(focusableSelector) ?? null;
};
const disabledElement = element => {
  const target = interactionTarget(element) ?? element;
  return Boolean(element?.getAttribute?.("aria-disabled") === "true" || target?.disabled || target?.getAttribute?.("aria-disabled") === "true");
};
const keyName = value => text(value).trim().normalize("NFKC").toLocaleLowerCase();
const isComposingEvent = event => event.isComposing || event.key === "Dead" || event.key === "Process" || event.key === "Compose";
const altGraph = event => event.getModifierState?.("AltGraph") === true || (event.ctrlKey && event.altKey);

function assertUniqueKeyTips(entries, owner) {
  const seen = new Map();
  for (const entry of entries) {
    if (!entry.keyTip) continue;
    const key = keyName(entry.keyTip);
    if ([...key].length !== 1) throw new TypeError(`${owner} key tips must be one character.`);
    if (seen.has(key)) throw new TypeError(`Key tip collision in ${owner}: ${entry.keyTip}.`);
    seen.set(key, entry.id ?? entry.label ?? key);
  }
}

function keyTipBadge(document, value) {
  const badge = document.createElement("span");
  badge.className = "ribbon-keytip";
  badge.setAttribute("aria-hidden", "true");
  badge.textContent = text(value).toLocaleUpperCase();
  return badge;
}

/**
 * Build an accessible, data-driven ribbon around caller-owned command elements.
 *
 * @param {{root:HTMLElement,tabs:Array<{id:string,label:string,keyTip?:string,disabled?:boolean,groups:Array<{id:string,label:string,keyTip?:string,hidden?:boolean,commands:Array<{id:string,label:string,keyTip?:string,element?:HTMLElement,disabled?:boolean,hidden?:boolean}>}>}>,onCommand?:(id:string)=>void,labels:{collapse:string,expand:string,navigation:string},initialTab?:string}} options
 */
export function createRibbon({ root, tabs, onCommand, labels, initialTab }) {
  if (!root || root.nodeType !== 1) throw new TypeError("A ribbon root element is required.");
  if (!Array.isArray(tabs) || tabs.length === 0) throw new TypeError("At least one ribbon tab is required.");
  if (!labels || !text(labels.collapse) || !text(labels.expand) || !text(labels.navigation)) {
    throw new TypeError("Ribbon collapse, expand, and navigation labels are required.");
  }
  const document = root.ownerDocument;
  const ids = new Set();
  for (const tab of tabs) {
    if (!tab?.id || ids.has(tab.id)) throw new TypeError("Ribbon tab ids must be present and unique.");
    ids.add(tab.id);
    if (!Array.isArray(tab.groups)) throw new TypeError(`Ribbon tab ${tab.id} needs groups.`);
    for (const group of tab.groups) {
      if (!Array.isArray(group.commands)) throw new TypeError(`Ribbon group ${group.id ?? ""} needs commands.`);
    }
    assertUniqueKeyTips(tab.groups.flatMap(group => [
      ...(group.keyTip ? [group] : []),
      ...group.commands.filter(command => command.keyTip),
    ]), `tab ${tab.id}`);
  }
  assertUniqueKeyTips(tabs, "ribbon tabs");

  const commandElements = new Set();
  for (const tab of tabs) for (const group of tab.groups) for (const command of group.commands) {
    if (command.element) {
      if (commandElements.has(command.element)) throw new TypeError(`Ribbon command element is reused: ${command.id}.`);
      commandElements.add(command.element);
    }
  }

  const instanceId = ++ribbonSequence;
  const rootHadLabel = root.hasAttribute("aria-label");
  const previousLabel = root.getAttribute("aria-label");
  const originalParents = [];
  const originalOrder = [];
  const descendants = [...root.querySelectorAll("*")];
  for (const element of descendants) {
    originalParents.push({ element, parent: element.parentNode, next: element.nextSibling });
  }
  for (const node of root.childNodes) originalOrder.push(node);

  const shell = document.createElement("div");
  shell.className = "ribbon-shell";
  const tablist = document.createElement("div");
  tablist.className = "ribbon-tabs";
  tablist.setAttribute("aria-label", text(labels.navigation));
  const collapse = document.createElement("button");
  collapse.className = "ribbon-collapse";
  collapse.type = "button";
  collapse.setAttribute("aria-expanded", "true");
  collapse.setAttribute("aria-label", text(labels.collapse));
  collapse.setAttribute("title", text(labels.collapse));
  collapse.setAttribute("aria-controls", `ribbon-${instanceId}-body`);
  collapse.textContent = "⌃";
  const navigation = document.createElement("div");
  navigation.className = "ribbon-navigation";
  navigation.append(tablist, collapse);

  const body = document.createElement("div");
  body.className = "ribbon-body";
  body.id = `ribbon-${instanceId}-body`;
  const tabEntries = [];
  const buttonByTab = new Map();
  const commandRecords = [];
  const groupRecords = [];
  const keyTipNodes = new Set();

  tabs.forEach((tab, tabIndex) => {
    const tabButton = document.createElement("button");
    tabButton.className = "ribbon-tab";
    tabButton.type = "button";
    tabButton.textContent = text(tab.label);
    tabButton.disabled = Boolean(tab.disabled);
    tabButton.id = `ribbon-${instanceId}-tab-${tab.id}`;
    tabButton.dataset.ribbonTabId = tab.id;
    const panel = document.createElement("section");
    panel.className = "ribbon-panel";
    panel.id = `ribbon-${instanceId}-panel-${tab.id}`;
    panel.dataset.ribbonPanelId = tab.id;
    const tabGroupKeys = [];
    tab.groups.forEach(group => {
      const wrapper = document.createElement("section");
      wrapper.className = "ribbon-group";
      wrapper.setAttribute("role", "group");
      wrapper.setAttribute("aria-label", text(group.label));
      wrapper.dataset.ribbonGroupId = group.id ?? "";
      const groupLabel = document.createElement("span");
      groupLabel.className = "ribbon-group-label";
      groupLabel.textContent = text(group.label);
      const commands = document.createElement("div");
      commands.className = "ribbon-commands";
      wrapper.append(groupLabel, commands);
      panel.append(wrapper);
      const groupRecord = { spec: group, wrapper, commands, groupLabel, tabId: tab.id };
      groupRecords.push(groupRecord);
      if (group.keyTip) tabGroupKeys.push({ id: `group:${group.id}`, keyTip: group.keyTip, element: wrapper, groupRecord });

      for (const command of group.commands) {
        const supplied = Boolean(command.element);
        const element = command.element ?? document.createElement("button");
        const original = supplied ? {
          hadRibbonCommandClass: element.classList.contains("ribbon-command"),
          dataCommandId: element.getAttribute("data-ribbon-command-id"),
          disabled: "disabled" in element ? element.disabled : undefined,
        } : null;
        if (!supplied) {
          element.type = "button";
          element.classList.add("ribbon-command");
          element.textContent = text(command.label);
          element.disabled = Boolean(command.disabled);
          if (command.label) element.setAttribute("aria-label", text(command.label));
        } else {
          element.classList.add("ribbon-command");
          if (command.disabled) element.disabled = true;
        }
        element.dataset.ribbonCommandId = command.id;
        if (command.keyTip) tabGroupKeys.push({ id: `command:${command.id}`, keyTip: command.keyTip, element, command, groupRecord });
        const onInternalClick = supplied ? null : () => {
          if (!disabledElement(element)) onCommand?.(command.id);
        };
        if (onInternalClick) element.addEventListener("click", onInternalClick);
        commands.append(element);
        commandRecords.push({
          spec: command, element, commands, supplied, onInternalClick, groupRecord, disabledOverridden: false,
          original,
        });
      }
    });
    // Group key tips and command key tips share the second key level.
    assertUniqueKeyTips(tabGroupKeys, `tab ${tab.id} commands and groups`);
    body.append(panel);
    const entry = { tab: tabButton, panel };
    tabEntries.push(entry);
    buttonByTab.set(tab.id, tabButton);
  });

  const utility = document.createElement("div");
  utility.className = "ribbon-utility";
  const retained = [...root.querySelectorAll("#close-preview, .editor-options, #editor-status")]
    .filter(element => !commandElements.has(element));
  for (const element of retained) utility.append(element);

  shell.append(navigation, body, utility);
  root.replaceChildren(shell);
  root.classList.add("ribbon-root");
  root.dataset.ribbonCollapsed = "false";
  root.setAttribute("aria-label", text(labels.navigation));

  let selectedIndex = Math.max(0, tabs.findIndex(tab => tab.id === initialTab && !tab.disabled));
  const tabController = attachTabs({
    tablist,
    tabs: tabEntries,
    orientation: "horizontal",
    onSelect(index) {
      selectedIndex = index;
      if (root.dataset.ribbonCollapsed === "true") renderCollapse(false);
    },
  });
  tabController.select(selectedIndex);

  const renderCollapse = collapsed => {
    root.dataset.ribbonCollapsed = String(Boolean(collapsed));
    collapse.setAttribute("aria-expanded", String(!collapsed));
    collapse.setAttribute("aria-label", text(collapsed ? labels.expand : labels.collapse));
    collapse.setAttribute("title", text(collapsed ? labels.expand : labels.collapse));
    collapse.textContent = collapsed ? "⌄" : "⌃";
  };
  const setCollapsed = collapsed => renderCollapse(Boolean(collapsed));
  const toggleCollapse = () => setCollapsed(root.dataset.ribbonCollapsed !== "true");
  const selectTab = id => {
    const index = tabs.findIndex(tab => tab.id === id);
    if (index < 0 || !tabController.select(index)) return false;
    return true;
  };
  const refresh = () => {
    tabs.forEach((tab, index) => {
      const tabButton = tabEntries[index].tab;
      if (typeof tab.disabled === "boolean") tabButton.disabled = tab.disabled;
    });
    for (const group of groupRecords) {
      group.wrapper.hidden = Boolean(group.spec.hidden);
      let visible = false;
      for (const record of commandRecords.filter(candidate => candidate.groupRecord === group)) {
        if (record.spec.hidden !== undefined) record.element.hidden = Boolean(record.spec.hidden);
        visible ||= !record.element.hidden;
        if (record.spec.disabled !== undefined && "disabled" in record.element) {
          record.element.disabled = Boolean(record.spec.disabled);
          if (record.supplied) record.disabledOverridden = true;
        }
      }
      if (!visible && group.spec.commands.length > 0) group.wrapper.hidden = true;
    }
  };
  refresh();

  let activeTips = false;
  let composing = false;
  let focusBeforeTips = null;
  let visibleTipNodes = [];
  const allCommandElements = () => commandRecords.map(record => record.element);
  const clearTips = ({ restoreFocus = false } = {}) => {
    if (!activeTips) return;
    activeTips = false;
    visibleTipNodes.forEach(node => node.remove());
    visibleTipNodes = [];
    keyTipNodes.forEach(node => node.classList.remove("ribbon-keytip-target"));
    keyTipNodes.clear();
    root.removeAttribute("data-keytips-active");
    if (restoreFocus && focusBeforeTips?.isConnected) focusBeforeTips.focus();
    focusBeforeTips = null;
  };
  const addTip = (target, value, record = null) => {
    target = interactionTarget(target) ?? target;
    const badge = keyTipBadge(document, value);
    target.classList.add("ribbon-keytip-target");
    target.append(badge);
    visibleTipNodes.push(badge);
    keyTipNodes.add(target);
    if (record) record.keyTipBadge = badge;
  };
  const showTabTips = () => {
    visibleTipNodes.forEach(node => node.remove()); visibleTipNodes = []; keyTipNodes.forEach(node => node.classList.remove("ribbon-keytip-target")); keyTipNodes.clear();
    tabs.forEach((tab, index) => { if (tab.keyTip && !tabEntries[index].tab.disabled) addTip(tabEntries[index].tab, tab.keyTip); });
    root.dataset.keytipsActive = "tabs";
  };
  const showCommandTips = () => {
    visibleTipNodes.forEach(node => node.remove()); visibleTipNodes = []; keyTipNodes.forEach(node => node.classList.remove("ribbon-keytip-target")); keyTipNodes.clear();
    const tab = tabs[selectedIndex];
    for (const group of tab.groups) {
      const groupRecord = groupRecords.find(record => record.spec === group);
      if (!groupRecord || groupRecord.wrapper.hidden) continue;
      if (group.keyTip) addTip(groupRecord.groupLabel, group.keyTip);
      for (const record of commandRecords.filter(candidate => candidate.groupRecord === groupRecord)) {
        if (record.spec.keyTip && !record.element.hidden) addTip(record.element, record.spec.keyTip, record);
      }
    }
    root.dataset.keytipsActive = "commands";
  };
  const beginTips = () => {
    if (activeTips) return;
    activeTips = true;
    focusBeforeTips = document.activeElement;
    root.setAttribute("data-keytips-active", "tabs");
    showTabTips();
  };
  const activateTip = key => {
    const normalized = keyName(key);
    if (!normalized) return false;
    if (root.dataset.keytipsActive === "tabs") {
      const index = tabs.findIndex((tab, tabIndex) => keyName(tab.keyTip) === normalized && !tabEntries[tabIndex].tab.disabled);
      if (index < 0) return false;
      tabController.select(index);
      tabEntries[index].tab.focus();
      showCommandTips();
      return true;
    }
    const tab = tabs[selectedIndex];
    for (const group of tab.groups) {
      if (keyName(group.keyTip) === normalized) {
        const groupRecord = groupRecords.find(record => record.spec === group);
        const target = commandRecords.find(record => record.groupRecord === groupRecord && !record.element.hidden && !disabledElement(record.element));
        if (!target) return false;
        const focusTarget = interactionTarget(target.element) ?? target.element;
        clearTips();
        focusTarget.focus?.();
        return true;
      }
    }
    const record = commandRecords.find(candidate => candidate.groupRecord.spec && tabs[selectedIndex].groups.includes(candidate.groupRecord.spec)
      && keyName(candidate.spec.keyTip) === normalized && !candidate.element.hidden && !disabledElement(candidate.element));
    if (!record) return false;
    const focusTarget = interactionTarget(record.element) ?? record.element;
    clearTips();
    focusTarget.focus?.();
    focusTarget.click?.();
    return true;
  };
  const onDocumentKeydown = event => {
    if (disposed || composing || isComposingEvent(event)) return;
    const modal = document.querySelector("dialog:modal");
    if (modal && !root.contains(modal)) {
      if (activeTips) clearTips();
      return;
    }
    if (event.key === "F1" && event.ctrlKey && !event.altKey && !event.metaKey) {
      event.preventDefault();
      event.stopPropagation();
      toggleCollapse();
      return;
    }
    if (event.key === "Alt" && !event.ctrlKey && !event.metaKey && !altGraph(event)) {
      event.preventDefault();
      beginTips();
      return;
    }
    if (!activeTips || event.ctrlKey || event.metaKey || altGraph(event)) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      clearTips({ restoreFocus: true });
      return;
    }
    if (event.key === "Alt" || event.key.length !== 1) return;
    if (activateTip(event.key)) {
      event.preventDefault();
      event.stopPropagation();
    } else {
      clearTips();
    }
  };
  const onCompositionStart = () => { composing = true; if (activeTips) clearTips(); };
  const onCompositionEnd = () => { composing = false; };
  const onOutsidePointer = () => { if (activeTips) clearTips(); };
  const onCollapseClick = () => toggleCollapse();
  document.addEventListener("keydown", onDocumentKeydown, true);
  document.addEventListener("compositionstart", onCompositionStart, true);
  document.addEventListener("compositionend", onCompositionEnd, true);
  document.addEventListener("pointerdown", onOutsidePointer, true);
  collapse.addEventListener("click", onCollapseClick);

  // Keep unassigned source nodes reachable during parent integration and restore them on dispose.
  const restoreMap = originalParents.slice();
  const cleanupInternalHandlers = () => {
    for (const record of commandRecords) if (record.onInternalClick) record.element.removeEventListener("click", record.onInternalClick);
  };
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    clearTips();
    cleanupInternalHandlers();
    for (const record of commandRecords) if (record.supplied) {
      if (!record.original.hadRibbonCommandClass) record.element.classList.remove("ribbon-command");
      if (record.original.dataCommandId === null) record.element.removeAttribute("data-ribbon-command-id");
      else record.element.setAttribute("data-ribbon-command-id", record.original.dataCommandId);
      if (record.disabledOverridden && record.original.disabled !== undefined) record.element.disabled = record.original.disabled;
    }
    tabController.dispose();
    document.removeEventListener("keydown", onDocumentKeydown, true);
    document.removeEventListener("compositionstart", onCompositionStart, true);
    document.removeEventListener("compositionend", onCompositionEnd, true);
    document.removeEventListener("pointerdown", onOutsidePointer, true);
    collapse.removeEventListener("click", onCollapseClick);
    for (const entry of restoreMap.reverse()) {
      if (entry.parent && entry.element !== entry.parent && entry.parent.nodeType === 1) {
        entry.parent.insertBefore(entry.element, entry.next?.parentNode === entry.parent ? entry.next : null);
      }
    }
    root.replaceChildren(...originalOrder);
    root.classList.remove("ribbon-root");
    delete root.dataset.ribbonCollapsed;
    if (rootHadLabel) root.setAttribute("aria-label", previousLabel);
    else root.removeAttribute("aria-label");
    root.removeAttribute("data-keytips-active");
  };

  const initialIndex = tabs.findIndex(tab => tab.id === initialTab && !tab.disabled);
  if (initialIndex >= 0) tabController.select(initialIndex);
  else tabController.select(tabs.findIndex(tab => !tab.disabled) >= 0 ? tabs.findIndex(tab => !tab.disabled) : 0);

  return Object.freeze({ selectTab, setCollapsed, refresh, dispose });
}
