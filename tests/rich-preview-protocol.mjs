// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { test } from "node:test";
import { createNativeHost } from "../web/native-host.mjs";
import { decodeEditorResponse } from "../web/editor-protocol.mjs";

function page() {
  const bytes = new Uint8Array(44), view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode("ATDL")); view.setUint16(4, 1, true);
  view.setFloat64(24, 595.276, true); view.setFloat64(32, 841.89, true);
  return bytes;
}
function response(text = "Titre é", pages = [page()]) {
  const utf8 = new TextEncoder().encode(text);
  const bytes = new Uint8Array(20 + utf8.length + pages.reduce((n,p) => n + 4 + p.length, 0));
  const view = new DataView(bytes.buffer);
  bytes.set(new TextEncoder().encode("ATRP")); view.setUint16(4, 1, true);
  view.setUint32(8, pages.length, true); view.setUint32(12, utf8.length, true); view.setUint32(16, 2, true);
  bytes.set(utf8, 20); let offset = 20 + utf8.length;
  for (const p of pages) { view.setUint32(offset, p.length, true); bytes.set(p, offset + 4); offset += 4 + p.length; }
  return bytes;
}
const decode = bytes => decodeEditorResponse(0x8020, bytes);
test("ATRP owns pages, preserves Unicode/notices and rejects corrupt envelopes", () => {
  const bytes = response(), result = decode(bytes);
  assert.deepEqual([result.kind,result.text,result.noticeCount,result.pages.length], ["preview","Titre é",2,1]);
  bytes.fill(0); assert.equal(result.pages[0][0], 65);
  for (const mutate of [b => b.subarray(0, 19), b => { b[4] = 2; return b; }, b => { b[6] = 1; return b; },
    b => { new DataView(b.buffer).setUint32(8, 257, true); return b; },
    b => { new DataView(b.buffer).setUint32(12, 131073, true); return b; },
    b => { b[20] = 0xff; return b; }, b => b.subarray(0,b.length-1),
    b => { const extended=new Uint8Array(b.length+1); extended.set(b); return extended; }]) {
    assert.throws(() => decode(mutate(response())));
  }
  const p = page(); new DataView(p.buffer).setFloat64(24, 600, true);
  assert.throws(() => decode(response("x", [p])));
});
class WebView {
  listeners = new Set(); posted=[];
  addEventListener(_,f) { this.listeners.add(f); }
  removeEventListener(_,f) { this.listeners.delete(f); }
  postMessage(value) { this.posted.push(value); }
  reply(status,payload) { const id=this.posted.at(-1).split("|")[2]; for (const f of this.listeners) f({data:`us120-file|1|${id}|${status}|${payload}`}); }
}
test("Native DOCX reads up to 8 MiB with bounded reservations, strict base64 and cancellation", async () => {
  const view=new WebView(), host=createNativeHost({webview:view});
  try {
    let pending=host.openDocx(); assert.match(view.posted.at(-1), /\|openDocx\|$/);
    await assert.rejects(host.openDocx(), /HOST_QUEUE_LIMIT/);
    view.reply("ok", Buffer.alloc(8*1024*1024, 42).toString("base64"));
    assert.equal((await pending).bytes.length,8*1024*1024);
    pending=host.open(); view.reply("ok", Buffer.alloc(131076).toString("base64"));
    assert.deepEqual(await pending, {kind:"error",key:"HOST_FILE_TOO_LARGE"});
    pending=host.openDocx(); view.reply("ok", Buffer.alloc(8*1024*1024+1).toString("base64"));
    assert.deepEqual(await pending,{kind:"error",key:"HOST_FILE_TOO_LARGE"});
    for (const invalid of ["a===","AB==","!!!!"]) {
      pending=host.openDocx(); view.reply("ok", invalid);
      assert.deepEqual(await pending,{kind:"error",key:"HOST_INVALID_RESPONSE"});
    }
    pending=host.openDocx(); view.reply("cancelled",""); assert.deepEqual(await pending,{kind:"cancelled"});
  } finally { host.close(); }
  assert.equal(view.listeners.size,0);
});
