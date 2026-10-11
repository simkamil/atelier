// SPDX-License-Identifier: AGPL-3.0-or-later
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { decodeDisplayList, DisplayListError } from "../web/display-list.mjs";

const proc = spawnSync("cargo", ["run", "-p", "atelier-render", "--example", "display_list_oracle", "--locked", "--offline"], { encoding: "utf8" });
assert.equal(proc.status, 0, proc.stderr);
const hex = proc.stdout.trim().split(/\r?\n/).at(-1);
assert.match(hex, /^(?:[0-9a-f]{2})+$/);
const bytes = Uint8Array.from(hex.match(/../g), b => Number.parseInt(b, 16));
assert.throws(() => decodeDisplayList(1), TypeError);
assert.throws(() => decodeDisplayList({ length: 10 }), TypeError);
assert.throws(() => decodeDisplayList(undefined), TypeError);
const list = decodeDisplayList(bytes);
assert.equal(decodeDisplayList(Buffer.from(bytes)).commands.length, 10, "accepts Node Buffer as a Uint8Array");
assert.equal(list.commands.length, 10);
assert.deepEqual(list.commands.map(c => c.type), ["fillPath","strokePath","drawGlyphRun","drawImage","pushClip","popClip","pushTransform","popTransform","link","structure"]);
assert.equal(list.commands[2].fontId, 0xfedcba9876543210n);
assert.equal(list.commands[2].glyphs[0].glyphId, 0xffffffff);
assert.equal(list.commands[3].imageId, 0x8000000000000001n);
assert.equal(list.commands[7].type, "popTransform");
assert.equal(list.commands[8].target, "\ufeffhttps://example.test/é");
assert.equal(list.commands[9].nodeId, 0xfffffffffffffffen);
const padded=new Uint8Array(bytes.length+5);padded.set(bytes,3);
assert.equal(decodeDisplayList(new Uint8Array(padded.buffer,3,bytes.length)).commands.length,10,"respects typed-array byte offset");

function rejected(candidate, kind) {
  assert.throws(() => decodeDisplayList(candidate), e => e instanceof DisplayListError && (!kind || e.kind === kind));
}
rejected(bytes.subarray(0, bytes.length-1), "format");
rejected(Uint8Array.from([...bytes, 0]), "format");
let bad=bytes.slice();bad[0]=0;rejected(bad,"format");
bad=bytes.slice();bad[4]=2;rejected(bad,"format");
// PopClip as the only command exercises independent clip-stack underflow.
const underflow=bytes.slice(0,45);new DataView(underflow.buffer).setUint32(40,1,true);underflow[44]=6;rejected(underflow,"state");
// Rust emits UTF-8 `é` as C3 A9; corrupt its first byte without changing length.
bad=bytes.slice();let utf=bad.findIndex((v,i)=>v===0xc3&&bad[i+1]===0xa9);assert(utf>=0);bad[utf]=0xff;rejected(bad,"format");
bad=bytes.slice();bad[44]=255;rejected(bad,"format");
bad=bytes.slice();new DataView(bad.buffer).setUint32(40,1_000_001,true);rejected(bad,"limit");
bad=bytes.slice();new DataView(bad.buffer).setFloat64(8,1e151,true);rejected(bad,"value");
bad=bytes.slice();new DataView(bad.buffer).setFloat32(46,2,true);rejected(bad,"value");

function stream(rawCommands,count=rawCommands.length){
  const out=bytes.slice(0,44), view=new DataView(out.buffer);view.setUint32(40,count,true);
  return Uint8Array.from([...out,...rawCommands]);
}
const pushClip=[5,...Array(32).fill(0)], popClip=[6];
rejected(stream([...Array(129).fill(pushClip).flat(),...Array(129).fill(popClip).flat()],258),"limit");
const pushTransform=[7,...[1,0,0,1,0,0].flatMap(v=>{const a=new Uint8Array(8);new DataView(a.buffer).setFloat64(0,v,true);return [...a];})];
rejected(stream([...Array(129).fill(pushTransform).flat(),...Array(129).fill([8]).flat()],258),"limit");
const hugeTransform=[7,...[1e150,0,0,1e150,0,0].flatMap(v=>{const a=new Uint8Array(8);new DataView(a.buffer).setFloat64(0,v,true);return [...a];})];
rejected(stream([...hugeTransform,...hugeTransform,8,8],4),"value");
const invalidPath=stream([1,0,...Array(16).fill(0),1,0,0,0,0],1);
// The fixture above is not a complete command; truncated variable fields remain format errors.
rejected(invalidPath,"format");
const overPathCount=stream([1,0,...Array(16).fill(0),0x41,0x42,0x0f,0x00],1);
rejected(overPathCount,"limit");
const overGlyphCount=stream([3,...Array(8+16).fill(0),0x41,0x42,0x0f,0x00],1);
rejected(overGlyphCount,"limit");
const overString=stream([9,...Array(32).fill(0),1,0x10,0,0],1);
rejected(overString,"limit");
const cumulativePath=new Uint8Array(44+22+17+999_999+23);
cumulativePath.set(bytes.subarray(0,44));new DataView(cumulativePath.buffer).setUint32(40,2,true);
let cursor=44;
cumulativePath[cursor++]=1;cursor++;cursor+=16;new DataView(cumulativePath.buffer).setUint32(cursor,1_000_000,true);cursor+=4;
cumulativePath[cursor++]=0;cursor+=16;cumulativePath.fill(4,cursor,cursor+999_999);cursor+=999_999;
cumulativePath[cursor++]=1;cursor++;cursor+=16;new DataView(cumulativePath.buffer).setUint32(cursor,1,true);cursor+=4;cumulativePath[cursor]=4;
rejected(cumulativePath,"limit");
console.log(JSON.stringify({status:"passed",checks:["Rust-produced all-opcode ATDL, valid close-following segments, and u64 identifiers","strict truncation/tag/version/trailing/UTF-8/opcode rejection","stack underflow/depth and composed-transform limits","coordinate/color validation and subview offsets"]}));
