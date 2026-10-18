// SPDX-License-Identifier: AGPL-3.0-or-later
// US-113 : protocole du pont natif étendu (dialogues, tickets, SharedBuffer, polices, URL) face à un
// WebView factice. Voir docs/us-113-native-bridge.md.
import assert from "node:assert/strict";
import { BRIDGE_ERROR_KEYS, DIALOG_FORMATS, WRITE_CHUNK_BYTES, buildDialogSpec, createNativeHost, validateUrl } from "../web/native-host.mjs";
import { readFileSync } from "node:fs";

const encoder = new TextEncoder();
const b64 = text => Buffer.from(typeof text === "string" ? encoder.encode(text) : text).toString("base64");

function fakeWebview({ shared = true } = {}) {
  const listeners = { message: [], sharedbufferreceived: [] };
  const sent = [];
  const released = [];
  const webview = {
    postMessage(message) { sent.push(message); },
    addEventListener(type, listener) { listeners[type].push(listener); },
    removeEventListener(type, listener) { listeners[type] = listeners[type].filter(item => item !== listener); },
  };
  if (shared) webview.releaseBuffer = buffer => released.push(buffer);
  return {
    webview, sent, released, listeners,
    reply(id, status, payload = "") { for (const listener of listeners.message) listener({ data: `us120-file|1|${id}|${status}|${payload}` }); },
    buffer(reply, bytes, v = 1) {
      const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength);
      for (const listener of listeners.sharedbufferreceived) listener({ additionalData: { v, reply }, getBuffer: () => buffer });
      return buffer;
    },
    last() { const fields = sent.at(-1).split("|"); return { id: fields[2], op: fields[3], tail: fields.slice(4).join("|") }; },
  };
}

const OPEN = { title: "Ouvrir", filters: [{ format: "docx", label: "Document Word" }, { format: "txt", label: "Texte brut" }] };

// --- fonctions pures --------------------------------------------------------------------------
assert.equal(buildDialogSpec(OPEN), "Ouvrir\nfilter:docx=Document Word\nfilter:txt=Texte brut");
assert.equal(buildDialogSpec({ ...OPEN, suggestedName: "a.txt" }, true).endsWith("\nname:a.txt"), true);
for (const bad of [
  undefined, {}, { title: "", filters: OPEN.filters }, { title: "T", filters: [] },
  { title: "T", filters: [{ format: "pdf", label: "PDF" }] },
  { title: "T", filters: [{ format: "txt", label: "A" }, { format: "txt", label: "B" }] },
  { title: "T", filters: [{ format: "txt", label: "" }] },
  { title: "T\nU", filters: [{ format: "txt", label: "A" }] },
  { title: " T", filters: [{ format: "txt", label: "A" }] },
  { title: "T", filters: [{ format: "txt", label: "A" }], suggestedName: "a.txt" },
  { title: "T", filters: DIALOG_FORMATS.concat(DIALOG_FORMATS).map(format => ({ format, label: "L" })) },
]) assert.equal(buildDialogSpec(bad), null, JSON.stringify(bad));
for (const name of ["..\\x.txt", "a/b.txt", "x.txt.", "..", "a*b"]) {
  assert.equal(buildDialogSpec({ title: "T", filters: [{ format: "txt", label: "A" }], suggestedName: name }, true), null, name);
}
for (const good of ["https://example.org", "http://example.org/a?b=c#d", "HTTPS://Example.ORG:8443/p%20q", "https://[::1]:8080/"]) assert.equal(validateUrl(good), null, good);
for (const [bad, key] of [
  ["", "url.error.invalid"], [null, "url.error.invalid"], ["javascript:alert(1)", "url.error.scheme_not_allowed"],
  ["file:///C:/x", "url.error.scheme_not_allowed"], ["mailto:a@b.c", "url.error.scheme_not_allowed"], ["example.org", "url.error.invalid"],
  ["https://u:p@example.org", "url.error.invalid"], ["https://exa mple.org", "url.error.invalid"], ["https://example.org/\"", "url.error.invalid"],
  ["https://exämple.org", "url.error.invalid"], ["https://", "url.error.invalid"], [`https://e.org/${"a".repeat(2048)}`, "url.error.too_long"],
]) assert.equal(validateUrl(bad), key, String(bad));

// --- dialogues et tickets ---------------------------------------------------------------------
{
  const fake = fakeWebview();
  const host = createNativeHost({ webview: fake.webview, timeoutMs: 1000 });
  const opened = host.pickOpen(OPEN);
  const { id, op, tail } = fake.last();
  assert.equal(op, "pickOpen");
  assert.equal(Buffer.from(tail, "base64").toString("utf8"), "Ouvrir\nfilter:docx=Document Word\nfilter:txt=Texte brut");
  fake.reply(id, "ok", `r12;docx;1048576;${b64("Lettre é.docx")}`);
  assert.deepEqual(await opened, { kind: "ok", ticket: "r12", format: "docx", size: 1048576, name: "Lettre é.docx" });

  const cancelled = host.pickOpen(OPEN);
  fake.reply(fake.last().id, "cancelled");
  assert.deepEqual(await cancelled, { kind: "cancelled" });

  const failed = host.pickOpen(OPEN);
  fake.reply(fake.last().id, "error", "files.error.dialog_failed");
  assert.deepEqual(await failed, { kind: "error", key: "files.error.dialog_failed" });

  const malformed = host.pickOpen(OPEN);
  fake.reply(fake.last().id, "ok", "r12;pdf;1;QQ==");
  assert.deepEqual(await malformed, { kind: "error", key: "HOST_INVALID_RESPONSE" });

  const sentBefore = fake.sent.length;
  assert.deepEqual(await host.pickOpen({ title: "T", filters: [] }), { kind: "error", key: "files.error.invalid_request" });
  assert.deepEqual(await host.readTicket("w3"), { kind: "error", key: "files.error.unknown_ticket" });
  assert.deepEqual(await host.readTicket("C:\\secret.txt"), { kind: "error", key: "files.error.unknown_ticket" });
  assert.deepEqual(await host.readFont("C:\\Windows\\Fonts\\arial.ttf"), { kind: "error", key: "fonts.error.unknown_id" });
  assert.equal(fake.sent.length, sentBefore, "invalid arguments never reach the native side");

  const saved = host.pickSave({ title: "Enregistrer", filters: [{ format: "docx", label: "Word" }], suggestedName: "a.docx" });
  assert.equal(fake.last().op, "pickSave");
  fake.reply(fake.last().id, "ok", `w5;docx;${b64("a.docx")}`);
  assert.deepEqual(await saved, { kind: "ok", ticket: "w5", format: "docx", name: "a.docx" });
  host.close();
}

// --- lecture : en ligne, SharedBuffer, copie, tampon invalide ou tardif -----------------------------
{
  const fake = fakeWebview();
  const host = createNativeHost({ webview: fake.webview, timeoutMs: 40 });
  let read = host.readTicket("r7");
  assert.deepEqual(fake.last(), { id: fake.last().id, op: "readTicket", tail: "r7" });
  fake.reply(fake.last().id, "ok", `b64;${b64("petit")}`);
  let result = await read;
  assert.equal(result.kind, "ok");
  assert.equal(new TextDecoder().decode(result.bytes), "petit");
  assert.equal(result.shared, false);

  const big = new Uint8Array(300000).map((_, index) => index % 251);
  read = host.readTicket("r8");
  let id = fake.last().id;
  const buffer = fake.buffer(`us120-file|1|${id}|ok|sb;${big.byteLength}`, big);
  result = await read;
  assert.equal(result.shared, true);
  assert.equal(result.bytes.byteLength, big.byteLength);
  assert.equal(result.bytes[250], 250);
  assert.equal(fake.released.length, 0, "the buffer lives until release()");
  assert.equal(host.stats().held, 1);
  result.release(); result.release();
  assert.deepEqual(fake.released, [buffer], "released exactly once");
  assert.equal(host.stats().held, 0);

  read = host.readTicket("r9", { copy: true });
  id = fake.last().id;
  fake.buffer(`us120-file|1|${id}|ok|sb;${big.byteLength}`, big);
  result = await read;
  assert.equal(fake.released.length, 2, "copy mode releases at once");
  assert.equal(result.bytes[7], 7);
  assert.equal(host.stats().held, 0);

  read = host.readTicket("r10");
  id = fake.last().id;
  fake.buffer(`us120-file|1|${id}|ok|sb;12`, big);
  assert.deepEqual(await read, { kind: "error", key: "HOST_INVALID_RESPONSE" });
  assert.equal(fake.released.length, 3, "a buffer with the wrong size is released");

  read = host.readTicket("r11");
  id = fake.last().id;
  fake.buffer(`us120-file|1|${id}|ok|sb;${big.byteLength}`, big, 2);
  assert.deepEqual(await read, { kind: "error", key: "HOST_TIMEOUT" });
  assert.equal(fake.released.length, 4, "a buffer with the wrong version is released and ignored");

  fake.buffer("us120-file|1|99999|ok|sb;4", new Uint8Array(4));
  fake.buffer("garbage", new Uint8Array(4));
  assert.equal(fake.released.length, 6, "late and unknown buffers are released");

  read = host.readTicket("r12");
  id = fake.last().id;
  fake.reply(id, "ok", "sb;5");
  assert.deepEqual(await read, { kind: "error", key: "HOST_INVALID_RESPONSE" }, "a shared reply must carry its buffer");

  read = host.readTicket("r13");
  id = fake.last().id;
  fake.reply(id, "ok", `fb;${b64("repli")}`);
  result = await read;
  assert.equal(new TextDecoder().decode(result.bytes), "repli");
  assert.equal(host.stats().fallbackReceived, 1);

  read = host.readTicket("r14");
  fake.buffer(`us120-file|1|${fake.last().id}|ok|sb;${big.byteLength}`, big);
  const held = (await read);
  assert.equal(host.stats().held, 1);
  host.close();
  assert.equal(host.stats().held, 0, "close() releases every outstanding buffer");
  held.release();
  assert.equal(fake.released.length, 7, "release after close is a no-op");
  assert.equal(fake.listeners.sharedbufferreceived.length, 0);
  assert.equal(fake.listeners.message.length, 0);
}

// --- sans releaseBuffer le pont n'écoute pas les tampons --------------------------------------------
{
  const fake = fakeWebview({ shared: false });
  const host = createNativeHost({ webview: fake.webview });
  assert.equal(fake.listeners.sharedbufferreceived.length, 0);
  assert.equal(host.stats().sharedBufferSupported, false);
  host.close();
}

// --- écriture atomique par morceaux ------------------------------------------------------------------
{
  const fake = fakeWebview();
  const host = createNativeHost({ webview: fake.webview, timeoutMs: 1000 });
  const bytes = new Uint8Array(WRITE_CHUNK_BYTES * 2 + 5).fill(65);
  const writing = host.writeTicket("w3", bytes);
  const seen = [];
  for (let step = 0; step < 3; step += 1) {
    await new Promise(resolve => setTimeout(resolve, 0));
    const { id, op, tail } = fake.last();
    assert.equal(op, "writeTicket");
    const [ticket, seq, last, data] = tail.split(";");
    seen.push([ticket, seq, last, Buffer.from(data, "base64").length]);
    fake.reply(id, "ok");
  }
  assert.deepEqual(await writing, { kind: "ok" });
  assert.deepEqual(seen, [["w3", "0", "0", WRITE_CHUNK_BYTES], ["w3", "1", "0", WRITE_CHUNK_BYTES], ["w3", "2", "1", 5]]);
  assert.ok(fake.sent.every(message => message.length < 175000));

  const empty = host.writeTicket("w4", new Uint8Array(0));
  await new Promise(resolve => setTimeout(resolve, 0));
  assert.equal(fake.last().tail, "w4;0;1;");
  fake.reply(fake.last().id, "ok");
  assert.deepEqual(await empty, { kind: "ok" });

  const failing = host.writeTicket("w5", bytes);
  await new Promise(resolve => setTimeout(resolve, 0));
  const count = fake.sent.length;
  fake.reply(fake.last().id, "error", "files.error.save_failed");
  assert.deepEqual(await failing, { kind: "error", key: "files.error.save_failed" });
  assert.equal(fake.sent.length, count, "no further chunk after a native error");

  assert.deepEqual(await host.writeTicket("r5", bytes), { kind: "error", key: "files.error.unknown_ticket" });
  assert.deepEqual(await host.writeTicket("w6", new Uint8Array(64 * 1024 * 1024 + 1)), { kind: "error", key: "files.error.too_large" });
  await assert.rejects(async () => host.writeTicket("w6", "text"), TypeError);

  const release = host.releaseTicket("w9");
  assert.equal(fake.last().tail, "w9");
  fake.reply(fake.last().id, "ok");
  assert.deepEqual(await release, { kind: "ok" });
  host.close();
}

// --- polices ------------------------------------------------------------------------------------------
{
  const fake = fakeWebview();
  const host = createNativeHost({ webview: fake.webview, timeoutMs: 1000 });
  const tsv = "f2-0\tArial\tRegular\t0\t915000\nf2-1\tCambria\tBold\t1\t1200000\n";
  let listed = host.listFonts();
  fake.reply(fake.last().id, "ok", `b64;${b64(tsv)}`);
  assert.deepEqual(await listed, { kind: "ok", fonts: [
    { id: "f2-0", family: "Arial", style: "Regular", face: 0, size: 915000 },
    { id: "f2-1", family: "Cambria", style: "Bold", face: 1, size: 1200000 },
  ] });
  listed = host.listFonts();
  fake.reply(fake.last().id, "ok", `b64;${b64("C:\\Windows\\Fonts\\arial.ttf\tArial\tRegular\t0\t1\n")}`);
  assert.deepEqual(await listed, { kind: "error", key: "HOST_INVALID_RESPONSE" });
  listed = host.listFonts();
  const sharedList = encoder.encode(tsv.repeat(2000));
  fake.buffer(`us120-file|1|${fake.last().id}|ok|sb;${sharedList.byteLength}`, sharedList);
  assert.equal((await listed).fonts.length, 4000);
  assert.equal(fake.released.length, 1, "the font list buffer is released after parsing");

  const font = host.readFont("f2-1");
  assert.equal(fake.last().tail, "f2-1");
  fake.reply(fake.last().id, "error", "fonts.error.unknown_id");
  assert.deepEqual(await font, { kind: "error", key: "fonts.error.unknown_id" });
  host.close();
}

// --- URL ----------------------------------------------------------------------------------------------
{
  const fake = fakeWebview();
  const host = createNativeHost({ webview: fake.webview, timeoutMs: 1000 });
  assert.deepEqual(await host.openUrl("javascript:alert(1)"), { kind: "error", key: "url.error.scheme_not_allowed" });
  assert.equal(fake.sent.length, 0);
  const opening = host.openUrl("https://example.org/é".replace("é", "%C3%A9"));
  assert.equal(fake.last().op, "openUrl");
  assert.equal(Buffer.from(fake.last().tail, "base64").toString(), "https://example.org/%C3%A9");
  fake.reply(fake.last().id, "error", "url.error.no_handler");
  assert.deepEqual(await opening, { kind: "error", key: "url.error.no_handler" });
  host.close();
}

// --- ouverture (US-115) par SharedBuffer ---------------------------------------------------------------
{
  const fake = fakeWebview();
  const host = createNativeHost({ webview: fake.webview, timeoutMs: 1000 });
  const docx = new Uint8Array(500000).fill(7);
  const taking = host.takeLaunch();
  fake.buffer(`us120-file|1|${fake.last().id}|ok|docx;2;@sb`, docx);
  const result = await taking;
  assert.equal(result.document.type, "docx");
  assert.equal(result.document.skipped, 2);
  assert.equal(result.document.bytes.byteLength, 500000);
  assert.equal(fake.released.length, 1, "the launch document is copied and its buffer released");
  const legacy = host.takeLaunch();
  fake.reply(fake.last().id, "ok", `txt;0;${b64("bonjour")}`);
  assert.equal(new TextDecoder().decode((await legacy).document.bytes), "bonjour");
  host.close();
}

// --- catalogue : chaque clé du pont existe en français -------------------------------------------------
{
  const catalogue = new Set(readFileSync(new URL("../web/i18n/fr-FR.txt", import.meta.url), "utf8").split("\n").map(line => line.split("=")[0]));
  const missing = BRIDGE_ERROR_KEYS.filter(key => !catalogue.has(key));
  assert.deepEqual(missing, []);
}

console.log("native bridge protocol checks passed: dialog specs, tickets, shared buffers (release, copy, late, invalid), chunked writes, fonts, URL, launch");
