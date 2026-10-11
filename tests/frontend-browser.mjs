// SPDX-License-Identifier: AGPL-3.0-or-later
const report = { status: "failed", suites: [], userAgent: navigator.userAgent };
try {
  // Sequential suites keep timing and focus tests independent in the same DOM.
  const requested = new URL(location.href).searchParams.get("suite") ?? "all";
  if (!["all", "canvas", "input"].includes(requested)) throw new Error("Invalid suite");
  for (const name of requested === "all" ? ["canvas", "input"] : [requested]) {
    const module = await import(`./${name}-browser.mjs`);
    const run = module[name === "canvas" ? "runCanvasChecks" : "runInputChecks"];
    const checks = await run();
    if (name === "input") {
      const { runTextInputEditingChecks } = await import("./text-input-editing.mjs");
      checks.push(...await runTextInputEditingChecks());
    }
    if (!Array.isArray(checks) || !checks.length) throw new Error(`Empty ${name} suite`);
    report.suites.push({ name, checks });
  }
  if (requested === "all") {
    const { CanvasRenderer } = await import("../web/canvas-renderer.mjs");
    const { attachTextInput } = await import("../web/text-input.mjs");
    const canvas = document.createElement("canvas");
    canvas.style.width = "100px";
    canvas.style.height = "100px";
    document.body.append(canvas);
    const renderer = new CanvasRenderer(canvas);
    renderer.setViewport({ width: 100, height: 100, dpr: 1 });
    const commands = [];
    const input = attachTextInput({
      getSelection: () => ({ anchor: 2, focus: 4 }),
      getCaret: () => ({ x: 12, y: 20, width: 1, height: 14 }),
      onCommand: command => commands.push(command),
      onComposition: state => {
        // Fixed layout fixture: this check covers the API connection, not shaping.
        if (state.phase === "start" || state.phase === "update") {
          renderer.setOverlay([{ x: state.caret.x, y: state.caret.y,
            width: state.text.length * 7, height: state.caret.height }]);
        } else renderer.clearOverlay();
      },
    });
    try {
      input.element.dispatchEvent(new CompositionEvent("compositionstart"));
      input.element.dispatchEvent(new CompositionEvent("compositionupdate", { data: "日本" }));
      if (renderer.ctx.getImageData(18, 34, 1, 1).data[3] === 0) {
        throw new Error("Composition callback did not draw a Canvas underline");
      }
      input.element.dispatchEvent(new CompositionEvent("compositionend", { data: "日本" }));
      if (renderer.ctx.getImageData(18, 34, 1, 1).data[3] !== 0 || commands.length !== 1
          || commands[0].text !== "日本" || commands[0].selection.focus !== 4) {
        throw new Error("Composition commit did not clear the underline and preserve selection");
      }
      report.suites.push({ name: "composition-integration", checks: [
        "DOM composition callback draws then clears a real Canvas underline with fixture geometry",
      ] });
    } finally {
      input.dispose();
      renderer.destroy();
      canvas.remove();
    }
  }
  report.status = "passed";
} catch (error) {
  report.error = error.stack ?? String(error);
}
document.querySelector("#result").textContent = JSON.stringify(report, null, 2);
if (new URL(location.href).searchParams.get("automated") === "1") {
  await fetch("/__frontend_result", { method: "POST", body: JSON.stringify(report) });
}
