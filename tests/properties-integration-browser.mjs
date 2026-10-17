// SPDX-License-Identifier: AGPL-3.0-or-later
// US-328 through the running app: ribbon command, dialog, real Worker/WASM, DOCX preview open.

/** The DOCX preview must be open and ready; leaves it open with the dialog closed. */
export async function checkProperties({ app, assert, waitFor, key }) {
  const dialog = app.properties?.element;
  assert(dialog instanceof HTMLDialogElement, "properties dialog is part of the app");
  const command = document.querySelector("#properties-command");
  assert(command && !command.disabled, "Properties command is enabled while a DOCX preview is ready");
  const field = name => document.querySelector(`#${dialog.querySelector(`label[for$="-${name}"]`).htmlFor}`);
  const counter = name => document.getElementById(`${field(name).id}-count`).textContent.replace(/[\s  ]/g, " ");
  const error = name => document.getElementById(`${field(name).id}-error`);
  const ok = () => dialog.querySelector(".properties-ok");
  const type = (name, value) => { const input = field(name); input.value = value; input.dispatchEvent(new Event("input", { bubbles: true })); };
  const openFromCommand = async () => {
    command.focus(); command.click();
    await waitFor(() => dialog.open, "properties dialog opens");
  };
  const closed = () => waitFor(() => !dialog.open, "properties dialog closes");
  const readOnly = () => [...dialog.querySelectorAll(".properties-readonly dd")].map(node => node.textContent);

  await openFromCommand();
  assert(dialog.getAttribute("aria-labelledby") && document.getElementById(dialog.getAttribute("aria-labelledby")).textContent === "Propriétés du document",
    "dialog is named by its visible title");
  assert(document.activeElement === field("title"), "focus lands on the Title field");
  const names = ["title", "subject", "author", "keywords", "comments", "category"];
  assert(names.every(name => field(name).labels?.length === 1 && field(name).getAttribute("aria-describedby").includes("-count")),
    "six editable fields have labels and counters described to assistive technology");
  assert(names.every(name => field(name).value === ""), "a document without property parts opens with empty fields");
  assert(readOnly().length === 12 && readOnly().every(text => text === "Non renseigné"), "absent dates and statistics are shown as not provided");
  assert(counter("title") === "0 / 255 caractères" && counter("comments") === "0 / 4 096 caractères", "counters show the engine limits");

  // Over the limit: marked invalid, OK disabled, nothing is applied.
  type("title", "é".repeat(256));
  assert(field("title").getAttribute("aria-invalid") === "true" && !error("title").hidden && /256 caractères.*255/.test(error("title").textContent) && ok().disabled,
    "a 256-character title is refused with a message and OK disabled");
  assert(counter("title") === "256 / 255 caractères", "counter keeps counting past the limit");
  field("title").form.requestSubmit();
  assert(dialog.open, "submitting an invalid form keeps the dialog open");
  type("title", "é".repeat(255));
  assert(field("title").getAttribute("aria-invalid") === "false" && error("title").hidden && !ok().disabled, "255 characters (the limit) is accepted");

  // Cancel leaves no trace.
  type("title", "Brouillon");
  type("author", "Quelqu’un");
  dialog.querySelector(".properties-cancel").click();
  await closed();
  assert(document.activeElement === command, "closing restores focus to the Properties command");
  await openFromCommand();
  assert(names.every(name => field(name).value === ""), "Cancel changed nothing: the reopened dialog shows the stored (empty) values");

  // Escape cancels too.
  type("subject", "Jamais gardé");
  key(dialog, "Escape");
  await closed();
  await openFromCommand();
  assert(field("subject").value === "", "Escape cancelled the edit");

  // Focus stays trapped.
  dialog.querySelector(".properties-cancel").focus();
  key(dialog, "Tab");
  assert(dialog.contains(document.activeElement) && document.activeElement !== dialog.querySelector(".properties-cancel"), "Tab from the last button wraps inside the dialog");

  // OK applies through the engine; reopening re-reads the DOCX.
  const edits = { title: "Rapport « Œuvre » — 2026", subject: "Sujet & <test>", author: "Élodie Müller", keywords: "un; deux", comments: "Ligne un\nLigne deux", category: "Finance" };
  for (const [name, value] of Object.entries(edits)) type(name, value);
  ok().click();
  await closed();
  assert(document.activeElement === command, "focus returns to the command after OK");
  await openFromCommand();
  assert(Object.entries(edits).every(([name, value]) => field(name).value === value), "OK then reopen reads the six values back from the DOCX package");
  assert(counter("title") === "24 / 255 caractères", "counter reflects stored text in code points");
  // Unchanged OK closes without touching the document; clearing a field removes it.
  ok().click();
  await closed();
  await openFromCommand();
  type("category", "");
  ok().click();
  await closed();
  await openFromCommand();
  assert(field("category").value === "" && field("title").value === edits.title, "an emptied field is removed and the others are kept");
  dialog.querySelector(".properties-cancel").click();
  await closed();
  assert(app.previewActive && !document.querySelector("#rich-workspace").hidden, "the DOCX preview is still displayed");
}
