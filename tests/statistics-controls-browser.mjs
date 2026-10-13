// SPDX-License-Identifier: AGPL-3.0-or-later

export async function run({ assert, fixture }) {
  const stylesheet = document.createElement("link");
  stylesheet.rel = "stylesheet";
  stylesheet.href = "/statistics-controls.css";
  document.head.append(stylesheet);
  await new Promise((resolve, reject) => {
    stylesheet.addEventListener("load", resolve, { once: true });
    stylesheet.addEventListener("error", reject, { once: true });
  });

  const { createStatisticsControls } = await import("/statistics-controls.mjs");
  const previousLanguage = document.documentElement.lang;
  document.documentElement.lang = "fr";
  const labels = {
    statisticsMetric: "Mesure",
    statisticsDialogTitle: "Statistiques",
    statisticsClose: "Fermer",
    statisticsScopeDocument: "Document",
    statisticsScopeSelection: "Sélection",
    statusDocument: "{words} mots",
    statusSelection: "{selected} / {words} mots",
    statisticsFieldPages: "Pages",
    statisticsFieldWords: "Mots",
    statisticsFieldCharacters: "Caractères sans espaces",
    statisticsFieldCharactersWithSpaces: "Caractères avec espaces",
    statisticsFieldParagraphs: "Paragraphes",
    statisticsFieldLines: "Lignes",
    statisticsLoading: "Chargement des statistiques…",
    statisticsError: "Statistiques indisponibles",
    statisticsUnavailableData: "Statistiques non disponibles",
    statisticsNotesTextBoxes: "Notes et zones de texte",
    statisticsNotesUnavailable: "Indisponibles dans le modèle actuel.",
  };
  let refreshes = 0;
  const controls = createStatisticsControls({ document, labels, onOpen: () => refreshes++ });
  fixture.append(controls.element);
  const status = controls.element.querySelector("[data-statistics-status]");
  const live = controls.element.querySelector(".statistics-live");
  const dialog = controls.element.querySelector("[data-statistics-dialog]");
  const table = dialog.querySelector(".statistics-table");
  const dialogStatus = dialog.querySelector(".statistics-dialog-status");
  const count = (scope, field) => controls.element.querySelector(`[data-statistics-count="${scope}:${field}"]`);

  assert(status.textContent === labels.statisticsLoading && status.getAttribute("aria-live") !== "assertive", "initial pending state is explicit and polite");
  assert(table.getAttribute("aria-describedby") === dialogStatus.id && Boolean(dialogStatus.id), "statistics table is described by the dialog's state explanation");
  controls.setState({ busy: false });
  assert(status.textContent === labels.statisticsUnavailableData && dialogStatus.textContent === labels.statisticsUnavailableData,
    "missing snapshot is reported to status and table users instead of a successful zero");
  controls.setState({ busy: false, document: { words: 12, pages: 2, characters: 51, charactersWithSpaces: 63, paragraphs: 4, lines: 9 } });
  assert(status.textContent === "12 mots" && count("document", "words").textContent === "12", "document word count appears in status and table");
  controls.setState({ document: { words: 12345, pages: 2, characters: 12345, charactersWithSpaces: 12345, paragraphs: 4, lines: 9 } });
  const frenchNumber = new Intl.NumberFormat("fr").format(12345);
  assert(status.textContent === `${frenchNumber} mots` && count("document", "characters").textContent === frenchNumber, "counts use the document language instead of the browser locale");
  let liveMutations = 0;
  const liveObserver = new MutationObserver(records => { liveMutations += records.length; });
  liveObserver.observe(live, { childList: true, characterData: true, subtree: true });
  controls.setState({ document: { words: 12345, pages: 2, characters: 12345, charactersWithSpaces: 12345, paragraphs: 4, lines: 9 } });
  await new Promise(resolve => setTimeout(resolve, 0));
  liveObserver.disconnect();
  assert(liveMutations === 0, "unchanged live status does not generate a duplicate announcement");
  controls.setState({ document: { words: 12, pages: 2, characters: 51, charactersWithSpaces: 63, paragraphs: 4, lines: 9 } });
  assert(count("selection", "words").textContent === "—", "missing selection is shown as unavailable instead of a fabricated zero");
  controls.setState({ hasSelection: true, selection: { words: 3, pages: 1, characters: 11, charactersWithSpaces: 14, paragraphs: 1, lines: 2 } });
  assert(status.textContent === "3 / 12 mots" && count("selection", "words").textContent === "3", "selection changes update the selected/document word counts");

  status.click();
  assert(dialog.open && refreshes === 1 && status.getAttribute("aria-expanded") === "true", "opening uses the shared modal and asks the parent to refresh");
  const close = dialog.querySelector(".ui-dialog-close");
  close.focus();
  controls.setState({ document: { words: 14, pages: 3, characters: 60, charactersWithSpaces: 73, paragraphs: 5, lines: 11 } });
  assert(document.activeElement === close && count("document", "words").textContent === "14", "snapshot updates do not steal focus while the dialog is open");
  controls.open();
  assert(refreshes === 1 && document.querySelectorAll("[data-statistics-dialog]").length === 1, "reopening an already open dialog does not duplicate it or refresh twice");
  assert(dialog.querySelector("table thead th:nth-child(2)").textContent === "Document" && dialog.querySelector("table thead th:nth-child(3)").textContent === "Sélection", "table headings clearly distinguish document and selection scopes");
  assert(count("document", "charactersWithSpaces").textContent === "73" && count("selection", "characters").textContent === "11", "semantic count fields map into their matching table rows");
  const unavailable = dialog.querySelector(".statistics-unavailable input");
  assert(unavailable.disabled && dialog.querySelector(".statistics-unavailable-reason").textContent === labels.statisticsNotesUnavailable, "notes/text boxes are visibly disabled with an explanation");

  controls.setState({ busy: true });
  assert(dialogStatus.textContent === labels.statisticsLoading && table.getAttribute("aria-describedby") === dialogStatus.id, "loading explanation remains linked to the statistics table");
  controls.setState({ busy: false, error: new Error("query failed") });
  assert(dialogStatus.textContent === labels.statisticsError && status.textContent === labels.statisticsError
    && table.getAttribute("aria-describedby") === dialogStatus.id, "error explanation remains linked to the table and does not become zero counts");
  controls.setState({ error: null, disabled: true });
  assert(status.disabled && dialog.open, "disabled controls cannot reopen while an existing dialog stays dismissible");
  const closed = new Promise(resolve => dialog.addEventListener("close", resolve, { once: true }));
  dialog.requestClose();
  await closed;
  assert(!dialog.open && status.getAttribute("aria-expanded") === "false", "native modal cancellation closes and updates expanded state");
  controls.setState({ disabled: false });
  status.click();
  assert(dialog.open && refreshes === 2, "a subsequent open requests a fresh snapshot");

  controls.dispose();
  status.click();
  assert(!controls.element.isConnected && !dialog.isConnected && refreshes === 2, "dispose removes status and modal and clears listeners");
  document.documentElement.lang = previousLanguage;
  stylesheet.remove();
}
