// SPDX-License-Identifier: AGPL-3.0-or-later
// Choice of the editor engine (R7a): "v1" plain-text editor (default) or "v2" rich editor.
// `?engine=v1|v2` in the page URL wins; otherwise the persisted option `atelier.engine` ("v2" asks for v2);
// anything else is v1. R7b makes v2 the default.

export const ENGINE_STORAGE_KEY = "atelier.engine";

/**
 * @param {{search?: string, storage?: {getItem(key: string): string | null} | null}} [sources]
 * @returns {"v1" | "v2"}
 */
export function chooseEngine({ search = globalThis.location?.search ?? "", storage = null } = {}) {
  let requested = null;
  try { requested = new URLSearchParams(search).get("engine"); } catch { /* malformed query: ignored */ }
  if (requested === "v1" || requested === "v2") return requested;
  try { if (storage?.getItem(ENGINE_STORAGE_KEY) === "v2") return "v2"; } catch { /* storage may be blocked */ }
  return "v1";
}
