// SPDX-License-Identifier: AGPL-3.0-or-later
// Choice of the editor engine (R7c): "v2" rich editor by default, "v1" plain-text editor on request.
// `?engine=v1|v2` in the page URL wins; otherwise the persisted option `atelier.engine` ("v1" asks for v1, "v2" for v2);
// anything else is v2. To go back to v1: open the page with `?engine=v1`, or persist the choice with
// `localStorage.setItem("atelier.engine", "v1")`; `localStorage.removeItem("atelier.engine")` returns to the default.

export const ENGINE_STORAGE_KEY = "atelier.engine";

/**
 * @param {{search?: string, storage?: {getItem(key: string): string | null} | null}} [sources]
 * @returns {"v1" | "v2"} (default "v2")
 */
export function chooseEngine({ search = globalThis.location?.search ?? "", storage = null } = {}) {
  let requested = null;
  try { requested = new URLSearchParams(search).get("engine"); } catch { /* malformed query: ignored */ }
  if (requested === "v1" || requested === "v2") return requested;
  try {
    const stored = storage?.getItem(ENGINE_STORAGE_KEY);
    if (stored === "v1" || stored === "v2") return stored;
  } catch { /* storage may be blocked */ }
  return "v2";
}
