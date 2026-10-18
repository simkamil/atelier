// SPDX-License-Identifier: AGPL-3.0-or-later
import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const args = process.argv.slice(2);
const rootArg = args.find(arg => arg.startsWith("--root="));
// --root=<dir> redirects the reference, data/fonts and data/engine/manifest.json. It exists for
// tests/check-wasm-size.test.mjs (synthetic artifacts) and may point outside the repository: never use it
// in CI or to validate the product, which must read the committed reference and fonts.
const repoRoot = rootArg
  ? resolve(rootArg.slice("--root=".length))
  : resolve(dirname(fileURLToPath(import.meta.url)), "..");
const referencePath = resolve(repoRoot, "tools/bench/references/wasm-size.json");
const manifestPath = resolve(repoRoot, "data/engine/manifest.json");
const fontRoot = resolve(repoRoot, "data/fonts");
const defaultArtifact = resolve(
  repoRoot,
  "target/wasm32-unknown-unknown/wasm-release/atelier_core_wasm.wasm",
);
const positional = args.filter(arg => !arg.startsWith("--"));
const artifactPath = resolve(positional[0] ?? defaultArtifact);
const measureOnly = args.includes("--measure");

const FONT_PATH = /^data\/fonts\/(?:[^/\\.:][^/\\:]*\/)*[^/\\.:][^/\\:]*\.(?:ttf|otf)$/i;
const SHA256 = /^[0-9a-f]{64}$/;

async function readJson(path, what) {
  const text = await readFile(path, "utf8");
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`${what} ${path} is not valid JSON (${error.message})`);
  }
}

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
// `disk` lists every font file of data/fonts, `found` those present verbatim in the artifact.
// Byte-identical files are counted once (by SHA-256) so duplicates cannot inflate the font total.
async function embeddedFonts(artifact) {
  const disk = [];
  const byHash = new Map();
  for (const path of (await fontFiles(fontRoot)).sort()) {
    const bytes = await readFile(path);
    const sha256 = createHash("sha256").update(bytes).digest("hex");
    if (!byHash.has(sha256)) byHash.set(sha256, { bytes: bytes.length, count: occurrences(artifact, bytes) });
    disk.push({
      path: relative(repoRoot, path).split(sep).join("/"),
      bytes: bytes.length,
      sha256,
      count: byHash.get(sha256).count,
    });
  }
  const fontBytes = [...byHash.values()].reduce((sum, font) => sum + font.bytes * font.count, 0);
  return { disk, found: disk.filter(font => font.count > 0), fontBytes };
}

function validateReference(reference) {
  if (reference === null || typeof reference !== "object" || Array.isArray(reference)
      || reference.schema_version !== 2
      || reference.target !== "wasm32-unknown-unknown"
      || reference.profile !== "wasm-release"
      || !Number.isSafeInteger(reference.code_reference_bytes)
      || reference.code_reference_bytes <= 0
      || !Number.isSafeInteger(reference.allowed_growth_percent)
      || reference.allowed_growth_percent < 0
      || reference.allowed_growth_percent > 100
      || !Array.isArray(reference.fonts)) {
    throw new Error(`invalid or unsupported WASM size reference fields in ${referencePath}`);
  }
  const seen = new Set();
  reference.fonts.forEach((font, index) => {
    const where = `${referencePath}: fonts[${index}]`;
    if (font === null || typeof font !== "object") throw new Error(`${where} is not an object`);
    if (typeof font.path !== "string" || !FONT_PATH.test(font.path) || font.path.split("/").includes("..")) {
      throw new Error(`${where}.path must be a relative data/fonts/**/*.ttf|otf path, got ${JSON.stringify(font.path)}`);
    }
    if (typeof font.sha256 !== "string" || !SHA256.test(font.sha256)) {
      throw new Error(`${where}.sha256 must be 64 lowercase hexadecimal digits`);
    }
    if (!Number.isSafeInteger(font.bytes) || font.bytes <= 0) throw new Error(`${where}.bytes must be a positive integer`);
    if (seen.has(font.path)) throw new Error(`${where}.path ${font.path} is listed twice`);
    seen.add(font.path);
  });
}

// Why is a listed font not found verbatim in the artifact?
async function absenceCause(frozen, disk) {
  const onDisk = disk.find(font => font.path === frozen.path);
  if (!onDisk) return `${frozen.path} is listed but missing from data/fonts`;
  if (onDisk.sha256 !== frozen.sha256 || onDisk.bytes !== frozen.bytes) {
    return `${frozen.path} is listed but its file in data/fonts changed (reference out of date) and its new bytes are not in the WASM`;
  }
  let recorded;
  try {
    recorded = (await readJson(manifestPath, "engine manifest")).sources_sha256?.[frozen.path];
  } catch {
    recorded = undefined;
  }
  if (recorded !== undefined && recorded !== onDisk.sha256) {
    return `${frozen.path} is listed and unchanged, but the WASM is stale: its manifest records another version of this font; rebuild the engine`;
  }
  return `${frozen.path} is listed and unchanged in data/fonts but its bytes are not in the WASM verbatim: `
    + "the WASM was probably built before this font was embedded, or the font is now stored truncated, compressed or split "
    + "(it would then be counted as code); rebuild the engine and check how the font is embedded";
}

try {
  const artifact = await readFile(artifactPath);
  const { disk, found: fonts, fontBytes } = await embeddedFonts(artifact);
  const codeBytes = artifact.length - fontBytes;
  if (measureOnly) {
    console.log(JSON.stringify({ total_bytes: artifact.length, code_bytes: codeBytes, font_bytes: fontBytes,
      fonts: fonts.map(({ path, bytes, sha256 }) => ({ path, bytes, sha256 })) }, null, 2));
  } else {
    const reference = await readJson(referencePath, "WASM size reference");
    validateReference(reference);

    const problems = [];
    const expected = new Map(reference.fonts.map(font => [font.path, font]));
    for (const font of fonts) {
      const frozen = expected.get(font.path);
      if (!frozen) problems.push(`${font.path} is embedded but not listed in the frozen font budget`);
      else if (frozen.sha256 !== font.sha256 || frozen.bytes !== font.bytes) problems.push(`${font.path} changed without a reference update`);
      if (font.count !== 1) problems.push(`${font.path} is embedded ${font.count} times`);
      expected.delete(font.path);
    }
    for (const frozen of expected.values()) problems.push(await absenceCause(frozen, disk));

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
