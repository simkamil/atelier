// SPDX-License-Identifier: AGPL-3.0-or-later
import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const referencePath = resolve(repoRoot, "tools/bench/references/wasm-size.json");
const fontRoot = resolve(repoRoot, "data/fonts");
const defaultArtifact = resolve(
  repoRoot,
  "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm",
);
const artifactPath = resolve(process.argv.filter(arg => arg !== "--measure")[2] ?? defaultArtifact);
const measureOnly = process.argv.includes("--measure");

async function fontFiles(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) files.push(...await fontFiles(path));
    else if (/\.(ttf|otf)$/i.test(entry.name)) files.push(path);
  }
  return files;
}

function occurrences(haystack, needle) {
  let count = 0;
  for (let at = haystack.indexOf(needle); at !== -1; at = haystack.indexOf(needle, at + needle.length)) count++;
  return count;
}

// Embedded fonts are frozen data: they are measured exactly, apart from the code budget.
async function embeddedFonts(artifact) {
  const found = [];
  for (const path of (await fontFiles(fontRoot)).sort()) {
    const bytes = await readFile(path);
    const count = occurrences(artifact, bytes);
    if (count > 0) {
      found.push({
        path: relative(repoRoot, path).split(sep).join("/"),
        bytes: bytes.length,
        sha256: createHash("sha256").update(bytes).digest("hex"),
        count,
      });
    }
  }
  return found;
}

try {
  const artifact = await readFile(artifactPath);
  const fonts = await embeddedFonts(artifact);
  const fontBytes = fonts.reduce((sum, font) => sum + font.bytes * font.count, 0);
  const codeBytes = artifact.length - fontBytes;
  if (measureOnly) {
    console.log(JSON.stringify({ total_bytes: artifact.length, code_bytes: codeBytes, font_bytes: fontBytes,
      fonts: fonts.map(({ path, bytes, sha256 }) => ({ path, bytes, sha256 })) }, null, 2));
  } else {
    const reference = JSON.parse(await readFile(referencePath, "utf8"));
    if (reference.schema_version !== 2
        || reference.target !== "wasm32-unknown-unknown"
        || reference.profile !== "wasm-release"
        || !Number.isSafeInteger(reference.code_reference_bytes)
        || reference.code_reference_bytes <= 0
        || !Number.isSafeInteger(reference.allowed_growth_percent)
        || reference.allowed_growth_percent < 0
        || reference.allowed_growth_percent > 100
        || !Array.isArray(reference.fonts)) {
      throw new Error("invalid or unsupported WASM size reference fields");
    }

    const problems = [];
    const expected = new Map(reference.fonts.map(font => [font.path, font]));
    for (const font of fonts) {
      const frozen = expected.get(font.path);
      if (!frozen) problems.push(`${font.path} is embedded but not listed in the frozen font budget`);
      else if (frozen.sha256 !== font.sha256 || frozen.bytes !== font.bytes) problems.push(`${font.path} changed without a reference update`);
      if (font.count !== 1) problems.push(`${font.path} is embedded ${font.count} times`);
      expected.delete(font.path);
    }
    for (const path of expected.keys()) problems.push(`${path} is listed but no longer embedded`);

    const allowedPercent = BigInt(reference.allowed_growth_percent);
    const codeReference = BigInt(reference.code_reference_bytes);
    const codeLimit = Number((codeReference * (100n + allowedPercent)) / 100n);
    if (BigInt(codeBytes) * 100n > codeReference * (100n + allowedPercent)) {
      problems.push(`code is ${codeBytes} bytes, above the ${codeLimit}-byte limit `
        + `(${reference.allowed_growth_percent}% above ${reference.code_reference_bytes} bytes)`);
    }
    if (problems.length) throw new Error(problems.join("; "));
    console.log(
      `${artifact.length} bytes: code ${codeBytes} / ${codeLimit} byte limit `
      + `(${reference.allowed_growth_percent}% growth allowed from ${reference.code_reference_bytes} bytes), `
      + `fonts ${fontBytes} bytes in ${fonts.length} frozen files.`,
    );
  }
} catch (error) {
  console.error(`WASM size check failed: ${error.message}`);
  process.exitCode = 1;
}
