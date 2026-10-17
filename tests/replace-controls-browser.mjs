// SPDX-License-Identifier: AGPL-3.0-or-later

export async function run({ assert, fixture }) {
  const stylesheet = document.createElement("link");
  stylesheet.rel = "stylesheet";
  stylesheet.href = "/replace-controls.css";
  document.head.append(stylesheet);
  await new Promise((resolve, reject) => {
    stylesheet.addEventListener("load", resolve, { once: true });
    stylesheet.addEventListener("error", reject, { once: true });
  });

  const { createReplaceControls } = await import("/replace-controls.mjs");
  const labels = {
    replaceTitle: "Replace", replaceText: "Replacement text", replaceOne: "Replace",
    replaceAll: "Replace all", replaceBusy: "Replacing…", replaceError: "Replacement failed",
    replaceNone: "No matches replaced",
  };
  const { createI18n } = await import("/i18n.mjs");
  const i18n = createI18n({ catalog: { replaceCount: "{count, plural, one {# occurrence replaced} other {# occurrences replaced}}" } });
  const oneCalls = [], allCalls = [];
  const controls = createReplaceControls({
    document, i18n,
    labels,
    onReplace: value => oneCalls.push(value),
    onReplaceAll: value => allCalls.push(value),
  });
  fixture.append(controls.element);

  const text = controls.element.querySelector("[data-replace-text]");
  const one = controls.element.querySelector("[data-replace-one]");
  const all = controls.element.querySelector("[data-replace-all]");
  const status = controls.element.querySelector("[data-replace-status]");
  assert(controls.element.hidden && text.rows === 2 && text.getAttribute("aria-label") === labels.replaceText,
    "component starts hidden and exposes a named two-row replacement textarea");

  controls.setState({ visible: true, canReplace: true, canReplaceAll: true });
  text.value = "";
  one.click();
  assert(oneCalls.at(-1)?.replacement === "", "an empty replacement is passed through without substitution or trimming");
  text.value = "first\nsecond <b>literal</b>";
  controls.focus();
  controls.setState({ count: 0 });
  assert(status.textContent === labels.replaceNone && document.activeElement === text && text.value === "first\nsecond <b>literal</b>",
    "zero count is announced while state updates preserve the focused textarea and its literal multiline value");
  one.click(); all.click();
  assert(oneCalls.at(-1)?.replacement === text.value && allCalls.at(-1)?.replacement === text.value,
    "both callbacks receive the exact replacement string, including newline and markup characters");
  assert(status.getAttribute("role") === "status" && status.getAttribute("aria-live") === "polite",
    "replacement result is exposed through a polite live status");

  controls.setState({ count: 1 });
  assert(status.textContent === "1 occurrence replaced", "one occurrence uses the singular of the plural message");
  let statusMutations = 0;
  const statusObserver = new MutationObserver(records => { statusMutations += records.length; });
  statusObserver.observe(status, { childList: true, characterData: true, subtree: true });
  controls.setState({ count: 1 });
  await new Promise(resolve => setTimeout(resolve, 0));
  assert(statusMutations === 0, "re-rendering an unchanged result does not create duplicate live-region announcements");
  statusObserver.disconnect();
  controls.setState({ count: 17 });
  assert(status.textContent === "17 occurrences replaced", "multiple occurrences interpolate the count into the localized label");
  controls.setState({ count: null });
  assert(status.textContent === "", "null count does not announce a result");

  controls.setState({ busy: true });
  assert(one.disabled && all.disabled && !text.disabled && status.textContent === labels.replaceBusy,
    "busy state disables replacement actions while preserving textarea editing");
  const callsBeforeBusy = oneCalls.length + allCalls.length;
  one.click(); all.click();
  assert(oneCalls.length + allCalls.length === callsBeforeBusy, "disabled busy actions cannot dispatch callbacks");
  controls.setState({ busy: false, error: "opaque internal failure" });
  assert(status.textContent === labels.replaceError && !status.textContent.includes("opaque"),
    "error state uses the localized error label without exposing internal details");
  controls.setState({ error: null, disabled: true });
  assert(text.disabled && one.disabled && all.disabled, "disabled state blocks the textarea and replacement actions");
  controls.setState({ disabled: false, canReplace: false, canReplaceAll: false });
  assert(one.disabled && all.disabled, "each action follows its own match availability state");
  controls.setState({ canReplace: true });
  assert(!one.disabled && all.disabled, "single replacement can be enabled independently of replace all");

  controls.dispose();
  const callsAfterDispose = oneCalls.length + allCalls.length;
  one.click(); all.click();
  controls.setState({ visible: true, count: 1 });
  controls.focus();
  assert(!controls.element.isConnected && oneCalls.length + allCalls.length === callsAfterDispose,
    "dispose removes the view and prevents late actions or focus changes");
  stylesheet.remove();
}
