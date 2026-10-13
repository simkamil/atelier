// SPDX-License-Identifier: AGPL-3.0-or-later

export async function run({ assert, fixture }) {
  const stylesheet = document.createElement("link");
  stylesheet.rel = "stylesheet";
  stylesheet.href = "/zoom-controls.css";
  document.head.append(stylesheet);
  await new Promise((resolve, reject) => {
    stylesheet.addEventListener("load", resolve, { once: true });
    stylesheet.addEventListener("error", reject, { once: true });
  });

  const { createZoomControls } = await import("/zoom-controls.mjs");
  const labels = {
    group: "Zoom",
    range: "Niveau de zoom",
    percent: "Ouvrir le zoom",
    title: "Zoom du document",
    value: "Pourcentage",
    custom: "Personnalisé",
    width: "Largeur",
    page: "Une page",
    pages: "Deux pages",
    apply: "Appliquer",
    cancel: "Annuler",
    close: "Fermer",
    invalid: "Saisissez une valeur comprise entre 10 et 500.",
  };
  const changes = [];
  const controls = createZoomControls({ document, labels, onChange: change => changes.push(change) });
  fixture.append(controls.element);

  const range = controls.element.querySelector("[data-zoom-range]");
  const percent = controls.element.querySelector("[data-zoom-percent]");
  const dialog = document.querySelector("[data-zoom-dialog]");
  const value = dialog.querySelector("[data-zoom-value]");
  const apply = dialog.querySelector("[data-zoom-apply]");
  const cancel = dialog.querySelector(".zoom-dialog-cancel");
  const close = dialog.querySelector(".ui-dialog-close");
  const radio = mode => dialog.querySelector(`input[name="zoom-mode"][value="${mode}"]`);
  const open = () => percent.click();
  const submit = () => dialog.querySelector("form").requestSubmit();
  const count = () => changes.length;

  controls.setState({ zoom: 1, mode: "custom" });
  assert(percent.textContent === "100%", "factor 1 displays 100 percent");
  controls.setState({ zoom: 1.25, mode: "width" });
  assert(percent.textContent === "125%", "fractional view factor displays its real percentage");
  assert(percent.title === "Largeur — 125%" && percent.getAttribute("aria-label") === "Largeur — 125%", "percentage button names its current mode and real percentage");
  assert(range.getAttribute("aria-valuetext") === "125%", "range accessible value reports the real factor when its step rounds");
  range.value = "250";
  range.dispatchEvent(new Event("input", { bubbles: true }));
  assert(changes.at(-1)?.zoom === 2.5 && changes.at(-1)?.mode === "custom", "slider at 250 percent emits factor 2.5 in custom mode");

  for (const invalidValue of ["", "NaN", "12.5", "501", "9"]) {
    open();
    radio("custom").checked = true;
    value.value = invalidValue;
    const before = count();
    submit();
    assert(count() === before && dialog.open && value.getAttribute("aria-invalid") === "true", `custom input ${JSON.stringify(invalidValue)} is rejected without closing or callback`);
    cancel.click();
  }
  for (const boundary of [10, 500]) {
    open();
    radio("custom").checked = true;
    value.value = String(boundary);
    submit();
    assert(changes.at(-1)?.zoom === boundary / 100 && changes.at(-1)?.mode === "custom", `custom boundary ${boundary} percent is accepted`);
  }

  for (const mode of ["width", "page", "pages"]) {
    open();
    radio(mode).checked = true;
    value.value = "123.45";
    submit();
    assert(changes.at(-1)?.zoom === 1.2345 && changes.at(-1)?.mode === mode, `fractional percentage is accepted for fit mode ${mode}`);
  }

  let beforeClose = count();
  open();
  cancel.click();
  assert(!dialog.open && count() === beforeClose, "Cancel closes without applying");
  open();
  const cancelEvent = new Event("cancel", { bubbles: true, cancelable: true });
  dialog.dispatchEvent(cancelEvent);
  assert(cancelEvent.defaultPrevented && !dialog.open && count() === beforeClose, "Escape cancel closes without applying");
  open();
  close.click();
  assert(!dialog.open && count() === beforeClose, "shared dialog close button closes without applying");

  open();
  apply.focus();
  const tab = new KeyboardEvent("keydown", { key: "Tab", bubbles: true, cancelable: true });
  dialog.dispatchEvent(tab);
  assert(tab.defaultPrevented && document.activeElement === close, "Tab from the last dialog control wraps to the first");
  const shiftTab = new KeyboardEvent("keydown", { key: "Tab", shiftKey: true, bubbles: true, cancelable: true });
  dialog.dispatchEvent(shiftTab);
  assert(shiftTab.defaultPrevented && document.activeElement === apply, "Shift+Tab from the first dialog control wraps to the last");
  controls.setState({ zoom: 1, mode: "custom", disabled: true });
  assert(dialog.open && !cancel.disabled && !close.disabled, "disabling while open keeps Cancel and shared close usable");
  cancel.click();
  assert(!dialog.open, "Cancel remains functional while controls are disabled");
  controls.setState({ disabled: false });
  open();
  controls.setState({ disabled: true });
  close.click();
  assert(!dialog.open, "shared close remains functional while controls are disabled");

  controls.setState({ disabled: false });
  open();
  if (matchMedia("(forced-colors: active)").matches) {
    const applyStyle = getComputedStyle(apply);
    assert(applyStyle.forcedColorAdjust === "none" && applyStyle.color !== applyStyle.backgroundColor,
      "apply button keeps a readable explicit forced-colors pair without automatic backplates");
  }
  const beforeDispose = count();
  controls.dispose();
  range.value = "250";
  range.dispatchEvent(new Event("input", { bubbles: true }));
  assert(!controls.element.isConnected && !dialog.isConnected && count() === beforeDispose, "dispose removes control and modal DOM and clears listeners");

  stylesheet.remove();
}
