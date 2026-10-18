// SPDX-License-Identifier: AGPL-3.0-or-later
import { createI18n, parseCatalog } from "../web/i18n.mjs";
import { createPropertiesDialog } from "../web/properties-dialog.mjs";

const snapshot = () => ({
  editable: { title: "Titre", subject: "", author: "Ada", keywords: "a; b", comments: "", category: "" },
  limits: { title: 255, subject: 255, author: 255, keywords: 1024, comments: 4096, category: 255 },
  created: { valid: true, text: "2026-10-13T08:30:00Z" }, modified: { valid: false, text: "hier soir" }, lastPrinted: null,
  lastModifiedBy: "Grace", revision: "3", application: null,
  statistics: { pages: 2, words: 1234, characters: 5678, charactersWithSpaces: null, lines: 40, paragraphs: 9 },
});
const tick = () => new Promise(resolve => setTimeout(resolve, 0));

export async function run({ assert, fixture }) {
  const catalog = new Map([...parseCatalog(await (await fetch("/i18n/fr-FR.txt")).text()).entries].map(([key, entry]) => [key, entry.value]));
  const i18n = createI18n({ locale: "fr-FR", catalog });
  const opener = document.createElement("button"); opener.textContent = "Ouvrir"; fixture.append(opener);
  let loads = 0, applied = [], nextLoad = () => Promise.resolve(snapshot()), nextApply = () => Promise.resolve({ kind: "ok" });
  const dialog = createPropertiesDialog({ i18n, load: () => { loads += 1; return nextLoad(); }, apply: changes => { applied.push(changes); return nextApply(); } });
  fixture.append(dialog.element);
  const root = dialog.element;
  const input = name => root.querySelector(`label[for$="-${name}"]`).control;
  const type = (name, value) => { input(name).value = value; input(name).dispatchEvent(new Event("input", { bubbles: true })); };
  const ok = () => root.querySelector(".properties-ok"), cancel = () => root.querySelector(".properties-cancel");
  const key = name => root.dispatchEvent(new KeyboardEvent("keydown", { key: name, bubbles: true, cancelable: true }));
  const terms = () => Object.fromEntries([...root.querySelectorAll(".properties-readonly dt")].map(dt => [dt.textContent, dt.nextElementSibling.textContent]));

  opener.focus();
  assert(await dialog.open({ returnFocusTo: opener }) === true && root.open && loads === 1, "dialog loads the snapshot then opens");
  assert(await dialog.open() === false && loads === 1, "opening an open dialog is a no-op");
  const shown = terms();
  assert(/13 octobre 2026/.test(shown["Création"]) && /\d{2}:\d{2}/.test(shown["Création"]), "valid W3CDTF date is formatted by the French locale");
  assert(shown["Dernière modification"] === "hier soir", "unreadable date text is shown verbatim");
  assert(shown["Dernière impression"] === "Non renseigné" && shown["Application"] === "Non renseigné" && shown["Révision"] === "3", "absent values are labelled");
  assert(shown["Mots"].replace(/\s/g, "") === "1234" && shown["Mots"] !== "1234" && shown["Caractères (espaces compris)"] === "Non renseigné", "statistics use locale number grouping");
  assert(root.querySelectorAll(".properties-input[type=text], textarea.properties-input").length === 6, "exactly six fields are editable");
  assert([...root.querySelectorAll(".properties-readonly input,.properties-readonly textarea")].length === 0, "dates and statistics are plain text, not controls");

  ok().click(); await tick();
  assert(applied.length === 0 && !root.open && document.activeElement === opener, "OK without change closes without calling the engine");

  await dialog.open({ returnFocusTo: opener });
  type("title", "Nouveau"); type("category", "Rapports");
  nextApply = () => Promise.resolve({ kind: "tooLong", field: "title", max: 10, length: 99 });
  ok().click(); await tick();
  assert(root.open && applied.length === 1 && Object.keys(applied[0]).sort().join() === "category,title", "only changed fields are sent");
  assert(input("title").getAttribute("aria-invalid") === "true" && /10/.test(root.querySelector(`#${input("title").id}-count`).textContent) && document.activeElement === input("title"),
    "a TooLong answer from the engine marks the field and keeps the dialog open");
  type("title", "Court");
  nextApply = () => Promise.reject(new Error("boom"));
  ok().click(); await tick();
  assert(root.open && root.querySelector(".properties-status").textContent !== "" && root.querySelector(".properties-status").getAttribute("role") === "alert" && !ok().disabled,
    "an engine failure is announced and the dialog stays editable");
  let release;
  nextApply = () => new Promise(resolve => { release = resolve; });
  ok().click(); await tick();
  assert(ok().disabled, "OK is disabled while the engine works");
  key("Escape"); cancel().click();
  assert(root.open, "Escape and Cancel are ignored while the change is being applied");
  release({ kind: "ok" }); await tick(); await tick();
  assert(!root.open && document.activeElement === opener, "dialog closes and restores focus when the engine answers");

  nextLoad = () => Promise.reject(new Error("unreadable"));
  let rejected = false;
  try { await dialog.open({ returnFocusTo: opener }); } catch { rejected = true; }
  assert(rejected && !root.open, "a failed load rejects and leaves the dialog closed");
  nextLoad = () => Promise.resolve(snapshot());
  assert(await dialog.open({ returnFocusTo: opener }) === true && input("title").value === "Titre", "reopening shows the stored values, not the abandoned edit");
  type("title", "x".repeat(300));
  assert(ok().disabled, "over-limit text disables OK");
  key("Escape"); await tick();
  assert(!root.open, "Escape closes the dialog");
  dialog.dispose();
  assert(!root.open, "dispose closes the dialog");
}
