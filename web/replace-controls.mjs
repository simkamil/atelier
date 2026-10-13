// SPDX-License-Identifier: AGPL-3.0-or-later

const label = (labels, key) => String(labels?.[key] ?? key);
let nextControlsId = 0;

function interpolateCount(value, count) {
  return value.replace(/\{count\}/g, String(count));
}

export function createReplaceControls({ document: doc = globalThis.document, labels, onReplace, onReplaceAll } = {}) {
  if (!doc || !labels) throw new TypeError("A document and localized labels are required.");

  const root = doc.createElement("section");
  root.className = "replace-controls";
  root.hidden = true;
  root.setAttribute("aria-labelledby", `replace-controls-title-${++nextControlsId}`);
  const heading = doc.createElement("h2");
  heading.className = "replace-controls-title";
  heading.id = root.getAttribute("aria-labelledby");
  heading.textContent = label(labels, "replaceTitle");

  const replacement = doc.createElement("textarea");
  replacement.className = "replace-controls-text";
  replacement.dataset.replaceText = "";
  replacement.rows = 2;
  replacement.setAttribute("aria-label", label(labels, "replaceText"));
  replacement.spellcheck = false;

  const status = doc.createElement("p");
  status.className = "replace-controls-status";
  status.dataset.replaceStatus = "";
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  status.setAttribute("aria-atomic", "true");

  const actions = doc.createElement("div");
  actions.className = "replace-controls-actions";
  const replaceOne = doc.createElement("button");
  replaceOne.type = "button";
  replaceOne.className = "replace-controls-action";
  replaceOne.dataset.replaceOne = "";
  replaceOne.textContent = label(labels, "replaceOne");
  const replaceAll = doc.createElement("button");
  replaceAll.type = "button";
  replaceAll.className = "replace-controls-action replace-controls-all";
  replaceAll.dataset.replaceAll = "";
  replaceAll.textContent = label(labels, "replaceAll");
  actions.append(replaceOne, replaceAll);
  root.append(heading, replacement, status, actions);

  let state = { visible: false, disabled: false, busy: false, canReplace: false, canReplaceAll: false, count: null, error: null };
  let disposed = false;
  const setStatus = value => {
    if (status.textContent !== value) status.textContent = value;
  };

  const render = () => {
    if (disposed) return;
    root.hidden = !state.visible;
    const disabled = Boolean(state.disabled);
    replacement.disabled = disabled;
    const actionsDisabled = disabled || Boolean(state.busy);
    replaceOne.disabled = actionsDisabled || !state.canReplace;
    replaceAll.disabled = actionsDisabled || !state.canReplaceAll;

    if (state.error) setStatus(label(labels, "replaceError"));
    else if (state.busy) setStatus(label(labels, "replaceBusy"));
    else if (state.count === null || state.count === undefined) setStatus("");
    else if (state.count === 0) setStatus(label(labels, "replaceNone"));
    else if (state.count === 1) setStatus(label(labels, "replaceCountOne"));
    else if (Number.isSafeInteger(state.count) && state.count > 1) {
      setStatus(interpolateCount(label(labels, "replaceCountMany"), state.count));
    } else setStatus("");
  };

  const requestReplace = () => {
    if (!disposed && !replaceOne.disabled) onReplace?.({ replacement: replacement.value });
  };
  const requestReplaceAll = () => {
    if (!disposed && !replaceAll.disabled) onReplaceAll?.({ replacement: replacement.value });
  };
  replaceOne.addEventListener("click", requestReplace);
  replaceAll.addEventListener("click", requestReplaceAll);
  render();

  return Object.freeze({
    element: root,
    setState(nextState = {}) {
      if (disposed) return;
      state = { ...state, ...nextState };
      render();
    },
    focus() {
      if (!disposed && !replacement.disabled) replacement.focus();
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      replaceOne.removeEventListener("click", requestReplace);
      replaceAll.removeEventListener("click", requestReplaceAll);
      root.remove();
    },
  });
}
