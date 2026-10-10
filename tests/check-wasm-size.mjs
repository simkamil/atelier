// SPDX-License-Identifier: AGPL-3.0-or-later
import { readFile, stat } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const referencePath = resolve(repoRoot, "tools/bench/references/wasm-size.json");
const defaultArtifact = resolve(
  repoRoot,
  "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm",
);
const artifactPath = resolve(process.argv[2] ?? defaultArtifact);

try {
  const reference = JSON.parse(await readFile(referencePath, "utf8"));
  if (reference.schema_version !== 1
      || reference.target !== "wasm32-unknown-unknown"
      || reference.profile !== "wasm-release"
      || !Number.isSafeInteger(reference.reference_bytes)
      || reference.reference_bytes <= 0
      || !Number.isSafeInteger(reference.allowed_growth_percent)
      || reference.allowed_growth_percent < 0
      || reference.allowed_growth_percent > 100) {
    throw new Error("invalid or unsupported WASM size reference fields");
  }

  const artifactBytes = (await stat(artifactPath)).size;
  const referenceBytes = BigInt(reference.reference_bytes);
  const allowedPercent = BigInt(reference.allowed_growth_percent);
  const exceedsLimit = BigInt(artifactBytes) * 100n > referenceBytes * (100n + allowedPercent);
  const maximumBytes = Number(
    (referenceBytes * (100n + allowedPercent)) / 100n,
  );

  if (exceedsLimit) {
    throw new Error(
      `${artifactBytes} bytes exceeds the ${maximumBytes}-byte limit `
      + `(${reference.allowed_growth_percent}% above ${reference.reference_bytes} bytes).`,
    );
  }
  console.log(
    `${artifactBytes} bytes / ${maximumBytes} byte limit `
    + `(${reference.allowed_growth_percent}% growth allowed from ${reference.reference_bytes} bytes).`,
  );
} catch (error) {
  console.error(`WASM size check failed: ${error.message}`);
  process.exitCode = 1;
}
