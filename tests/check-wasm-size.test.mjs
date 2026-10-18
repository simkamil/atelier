// SPDX-License-Identifier: AGPL-3.0-or-later
// Tests of tests/check-wasm-size.mjs on synthetic artifacts (never the real engine).
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { after, test } from "node:test";
import { fileURLToPath } from "node:url";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const script = join(repoRoot, "tests/check-wasm-size.mjs");
const scratchParent = join(repoRoot, "target", "check-wasm-size-tests");
mkdirSync(scratchParent, { recursive: true });
const scratch = mkdtempSync(join(scratchParent, "run-"));
after(() => rmSync(scratch, { recursive: true, force: true }));

const sha = bytes => createHash("sha256").update(bytes).digest("hex");
// Deterministic incompressible-looking bytes, distinct per seed.
function noise(seed, length) {
  const out = Buffer.alloc(length);
  for (let at = 0, block = 0; at < length; block++) {
    const digest = createHash("sha256").update(`${seed}:${block}`).digest();
    at += digest.copy(out, at);
  }
  return out;
}
const fontA = noise("font-a", 3000);
const fontB = noise("font-b", 2000);
const CODE = 10000;

let counter = 0;
// Builds a fixture root: data/fonts files, reference, optional manifest, and a wasm. Returns the paths.
function fixture({ fonts = { "a/A.ttf": fontA, "a/B.ttf": fontB }, listed, wasmParts, code = CODE,
  reference, manifest } = {}) {
  const root = join(scratch, `case-${counter++}`);
  for (const [name, bytes] of Object.entries(fonts)) {
    mkdirSync(dirname(join(root, "data/fonts", name)), { recursive: true });
    writeFileSync(join(root, "data/fonts", name), bytes);
  }
  const entries = (listed ?? Object.keys(fonts)).map(name => ({
    path: `data/fonts/${name}`, bytes: fonts[name].length, sha256: sha(fonts[name]),
  }));
  mkdirSync(join(root, "tools/bench/references"), { recursive: true });
  const body = reference ?? JSON.stringify({
    schema_version: 2, target: "wasm32-unknown-unknown", profile: "wasm-release",
    code_reference_bytes: CODE, allowed_growth_percent: 10, fonts: entries,
  });
  writeFileSync(join(root, "tools/bench/references/wasm-size.json"), body);
  if (manifest) {
    mkdirSync(join(root, "data/engine"), { recursive: true });
    writeFileSync(join(root, "data/engine/manifest.json"), JSON.stringify(manifest));
  }
  const wasm = join(root, "engine.wasm");
  writeFileSync(wasm, Buffer.concat([noise("code", code), ...(wasmParts ?? [fontA, fontB])]));
  return { root, wasm };
}

function run({ root, wasm }, extra = []) {
  const result = spawnSync(process.execPath, [script, `--root=${root}`, wasm, ...extra], { encoding: "utf8", timeout: 120000 });
  return { status: result.status, out: result.stdout, err: result.stderr };
}

test("passes with exact fonts and code within budget", () => {
  const { status, out } = run(fixture());
  assert.equal(status, 0);
  assert.match(out, /code 10000 \/ 11000 byte limit/);
  assert.match(out, /fonts 5000 bytes in 2 frozen files/);
});

test("fails when the code is too large", () => {
  const { status, err } = run(fixture({ code: 11001 }));
  assert.equal(status, 1);
  assert.match(err, /code is 11001 bytes, above the 11000-byte limit/);
});

test("fails when a font is embedded twice", () => {
  const { status, err } = run(fixture({ wasmParts: [fontA, fontB, fontA] }));
  assert.equal(status, 1);
  assert.match(err, /data\/fonts\/a\/A\.ttf is embedded 2 times/);
});

test("a listed font absent from the WASM names the probable cause", () => {
  const { status, err } = run(fixture({ wasmParts: [fontA, fontB.subarray(0, 1000)] }));
  assert.equal(status, 1);
  assert.match(err, /data\/fonts\/a\/B\.ttf is listed and unchanged in data\/fonts but its bytes are not in the WASM verbatim/);
  assert.match(err, /truncated, compressed or split/);
  assert.doesNotMatch(err, /no longer embedded/);
});

test("a WASM older than data/fonts is reported as stale through the manifest", () => {
  const fixtureCase = fixture({
    wasmParts: [fontA], manifest: { sources_sha256: { "data/fonts/a/B.ttf": sha(noise("old-font-b", 10)) } },
  });
  const { status, err } = run(fixtureCase);
  assert.equal(status, 1);
  assert.match(err, /B\.ttf is listed and unchanged, but the WASM is stale/);
});

test("a font changed on disk without a reference update is not blamed on the WASM", () => {
  const stale = fixture();
  writeFileSync(join(stale.root, "data/fonts/a/B.ttf"), noise("font-b-v2", 2000));
  const { status, err } = run(stale);
  assert.equal(status, 1);
  assert.match(err, /B\.ttf is listed but its file in data\/fonts changed/);
});

test("a listed font missing from data/fonts is named", () => {
  const { status, err } = run(fixture({ fonts: { "a/A.ttf": fontA }, listed: ["a/A.ttf"], wasmParts: [fontA],
    reference: JSON.stringify({
      schema_version: 2, target: "wasm32-unknown-unknown", profile: "wasm-release", code_reference_bytes: CODE,
      allowed_growth_percent: 10,
      fonts: [{ path: "data/fonts/a/A.ttf", bytes: 3000, sha256: sha(fontA) },
        { path: "data/fonts/a/Gone.ttf", bytes: 5, sha256: "a".repeat(64) }],
    }) }));
  assert.equal(status, 1);
  assert.match(err, /Gone\.ttf is listed but missing from data\/fonts/);
});

test("a corrupt reference names its file", () => {
  const { status, err } = run(fixture({ reference: "{ not json" }));
  assert.equal(status, 1);
  assert.match(err, /WASM size reference .*wasm-size\.json is not valid JSON/);
  assert.doesNotMatch(err, /SyntaxError/);
});

for (const [name, mutate, expected] of [
  ["absolute path", font => { font.path = "/etc/a.ttf"; }, /fonts\[0\]\.path must be a relative/],
  ["drive path", font => { font.path = "C:/data/fonts/a.ttf"; }, /fonts\[0\]\.path must be a relative/],
  ["parent segment", font => { font.path = "data/fonts/../x/a.ttf"; }, /fonts\[0\]\.path must be a relative/],
  ["outside data/fonts", font => { font.path = "data/other/a.ttf"; }, /fonts\[0\]\.path must be a relative/],
  ["wrong extension", font => { font.path = "data/fonts/a/A.txt"; }, /fonts\[0\]\.path must be a relative/],
  ["short sha256", font => { font.sha256 = "abc"; }, /fonts\[0\]\.sha256 must be 64/],
  ["uppercase sha256", font => { font.sha256 = "A".repeat(64); }, /fonts\[0\]\.sha256 must be 64/],
  ["negative bytes", font => { font.bytes = -1; }, /fonts\[0\]\.bytes must be a positive integer/],
]) {
  test(`an invalid font entry is rejected: ${name}`, () => {
    const base = JSON.parse(readReference());
    mutate(base.fonts[0]);
    const { status, err } = run(fixture({ reference: JSON.stringify(base) }));
    assert.equal(status, 1);
    assert.match(err, expected);
  });
}

function readReference() {
  return JSON.stringify({
    schema_version: 2, target: "wasm32-unknown-unknown", profile: "wasm-release", code_reference_bytes: CODE,
    allowed_growth_percent: 10,
    fonts: [{ path: "data/fonts/a/A.ttf", bytes: 3000, sha256: sha(fontA) },
      { path: "data/fonts/a/B.ttf", bytes: 2000, sha256: sha(fontB) }],
  });
}

test("byte-identical font files are counted once", () => {
  const twin = { "a/A.ttf": fontA, "a/A-copy.ttf": fontA, "a/B.ttf": fontB };
  const { status, out, err } = run(fixture({ fonts: twin }));
  assert.equal(status, 0, err);
  // Two copies of A on disk, one in the WASM: 5000 font bytes, not 8000 (code stays 10000).
  assert.match(out, /code 10000 \/ 11000/);
  assert.match(out, /fonts 5000 bytes in 3 frozen files/);
});

test("--measure counts identical files once", () => {
  const twin = { "a/A.ttf": fontA, "a/A-copy.ttf": fontA };
  const { status, out } = run(fixture({ fonts: twin, wasmParts: [fontA] }), ["--measure"]);
  assert.equal(status, 0);
  assert.equal(JSON.parse(out).font_bytes, 3000);
});
