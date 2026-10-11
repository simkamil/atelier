// SPDX-License-Identifier: AGPL-3.0-or-later
/** Decode the complete, bounded ATDL 1.0 stream produced by atelier-render. */
export const DISPLAY_LIST_LIMITS = Object.freeze({
  maxBytes: 64 * 1024 * 1024, maxCommands: 1_000_000,
  maxPathElements: 1_000_000, maxGlyphs: 1_000_000,
  maxStringBytes: 4096, maxStackDepth: 128,
  maxCoordinate: 1e150,
});

export class DisplayListError extends Error {
  constructor(kind, message) {
    super(message);
    this.name = "DisplayListError";
    this.kind = kind;
  }
}
const fail = (kind, message) => { throw new DisplayListError(kind, message); };

export function decodeDisplayList(input) {
  if (!(input instanceof Uint8Array) && !(input instanceof ArrayBuffer)) {
    throw new TypeError("ATDL input must be an ArrayBuffer or Uint8Array");
  }
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  if (bytes.byteLength > DISPLAY_LIST_LIMITS.maxBytes) fail("limit", "ATDL stream exceeds 64 MiB");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  let pathTotal = 0;
  let glyphTotal = 0;
  let clipDepth = 0;
  const transforms = [{ a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }];
  const need = length => {
    if (!Number.isSafeInteger(length) || length < 0 || length > bytes.length - at) {
      fail("format", "truncated ATDL stream");
    }
  };
  const u8 = () => {
    need(1);
    return view.getUint8(at++);
  };
  const u16 = () => {
    need(2);
    const value = view.getUint16(at, true);
    at += 2;
    return value;
  };
  const u32 = () => {
    need(4);
    const value = view.getUint32(at, true);
    at += 4;
    return value;
  };
  const u64 = () => {
    need(8);
    const value = view.getBigUint64(at, true);
    at += 8;
    return value;
  };
  const f64 = () => {
    need(8);
    const value = view.getFloat64(at, true);
    at += 8;
    if (!Number.isFinite(value) || Math.abs(value) > DISPLAY_LIST_LIMITS.maxCoordinate) {
      fail("value", "invalid ATDL coordinate");
    }
    return value;
  };
  const f32 = () => {
    need(4);
    const value = view.getFloat32(at, true);
    at += 4;
    if (!Number.isFinite(value) || value < 0 || value > 1) {
      fail("value", "invalid ATDL color component");
    }
    return value;
  };
  const point = () => ({ x: f64(), y: f64() });
  const rect = () => {
    const min = point(), max = point();
    if (max.x < min.x || max.y < min.y) fail("value", "invalid ATDL rectangle");
    return { min, max, x: min.x, y: min.y,
      width: max.x - min.x, height: max.y - min.y };
  };
  const color = () => ({ r: f32(), g: f32(), b: f32(), a: f32() });
  const count = (limit, label) => {
    const value = u32();
    if (value > limit) fail("limit", `${label} limit exceeded`);
    return value;
  };
  const string = () => {
    const length = count(DISPLAY_LIST_LIMITS.maxStringBytes, "string");
    need(length);
    let value;
    try {
      value = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true })
        .decode(bytes.subarray(at, at + length));
    } catch {
      fail("format", "invalid UTF-8 in ATDL string");
    }
    at += length;
    return value;
  };
  const path = () => {
    const n = count(DISPLAY_LIST_LIMITS.maxPathElements, "path element");
    pathTotal += n; if (pathTotal > DISPLAY_LIST_LIMITS.maxPathElements) fail("limit", "cumulative path element limit exceeded");
    if (n > bytes.length - at) fail("format", "path element count exceeds remaining bytes");
    const elements = [];
    let open = false;
    for (let i = 0; i < n; i++) {
      const op = u8();
      if (op === 0) { elements.push({ type: "moveTo", point: point() }); open = true; }
      else if (op === 1) { if (!open) fail("geometry", "line before move in path"); elements.push({ type: "lineTo", point: point() }); }
      else if (op === 2) { if (!open) fail("geometry", "quadratic segment before move in path"); elements.push({ type: "quadraticTo", control: point(), to: point() }); }
      else if (op === 3) { if (!open) fail("geometry", "cubic segment before move in path"); elements.push({ type: "cubicTo", control1: point(), control2: point(), to: point() }); }
      else if (op === 4) { if (!open) fail("geometry", "close before move in path"); elements.push({ type: "close" }); }
      else fail("format", `unknown path opcode ${op}`);
    }
    return elements;
  };
  need(8);
  if (bytes[at++] !== 65 || bytes[at++] !== 84 || bytes[at++] !== 68 || bytes[at++] !== 76) fail("format", "invalid ATDL signature");
  if (u16() !== 1 || u16() !== 0) fail("format", "unsupported ATDL version");
  const pageBounds = rect();
  if (!(pageBounds.width > 0 && pageBounds.height > 0)) fail("value", "page dimensions must be positive");
  const n = count(DISPLAY_LIST_LIMITS.maxCommands, "command");
  if (n > bytes.length - at) fail("format", "command count exceeds remaining bytes");
  const commands = [];
  for (let i = 0; i < n; i++) {
    const op = u8(); let c;
    switch (op) {
      case 1: { const rule = u8(); if (rule > 1) fail("format", "unknown fill rule"); c = { type: "fillPath", rule: rule ? "evenOdd" : "nonZero", color: color(), path: path() }; break; }
      case 2: { const ink = color(), width = f64(), cap = u8(), join = u8(), miterLimit = f64(); if (!(width > 0) || width > DISPLAY_LIST_LIMITS.maxCoordinate || cap > 2 || join > 2 || miterLimit < 1 || miterLimit > DISPLAY_LIST_LIMITS.maxCoordinate) fail("value", "invalid stroke style"); c = { type: "strokePath", color: ink, width, cap: ["butt", "round", "square"][cap], join: ["miter", "round", "bevel"][join], miterLimit, path: path() }; break; }
      case 3: { const fontId = u64(), ink = color(), length = count(DISPLAY_LIST_LIMITS.maxGlyphs, "glyph"); glyphTotal += length; if (glyphTotal > DISPLAY_LIST_LIMITS.maxGlyphs) fail("limit", "cumulative glyph limit exceeded"); if (length > Math.floor((bytes.length-at)/36)) fail("format", "glyph count exceeds remaining bytes"); const glyphs = []; for (let j = 0; j < length; j++) glyphs.push({ glyphId: u32(), position: point(), advance: point() }); c = { type: "drawGlyphRun", fontId, color: ink, glyphs }; break; }
      case 4: c = { type: "drawImage", imageId: u64(), bounds: rect() }; break;
      case 5: c = { type: "pushClip", bounds: rect() }; if (++clipDepth > DISPLAY_LIST_LIMITS.maxStackDepth) fail("limit", "clip stack limit exceeded"); break;
      case 6: if (--clipDepth < 0) fail("state", "clip stack underflow"); c = { type: "popClip" }; break;
      case 7: {
        const transform = { a: f64(), b: f64(), c: f64(), d: f64(), e: f64(), f: f64() };
        if (transforms.length > DISPLAY_LIST_LIMITS.maxStackDepth) fail("limit", "transform stack limit exceeded");
        const current=transforms.at(-1), composed={
          a:transform.a*current.a+transform.c*current.b,
          b:transform.b*current.a+transform.d*current.b,
          c:transform.a*current.c+transform.c*current.d,
          d:transform.b*current.c+transform.d*current.d,
          e:transform.a*current.e+transform.c*current.f+transform.e,
          f:transform.b*current.e+transform.d*current.f+transform.f,
        };
        if(Object.values(composed).some(v=>!Number.isFinite(v)||Math.abs(v)>DISPLAY_LIST_LIMITS.maxCoordinate))fail("value","composed transform is out of range");
        transforms.push(composed);c={type:"pushTransform",transform};break;
      }
      case 8: if (transforms.length===1) fail("state", "transform stack underflow"); transforms.pop(); c = { type: "popTransform" }; break;
      case 9: c = { type: "link", bounds: rect(), target: string() }; break;
      case 10: c = { type: "structure", role: string(), nodeId: u64() }; break;
      default: fail("format", `unknown command opcode ${op}`);
    }
    commands.push(c);
  }
  if (clipDepth !== 0 || transforms.length !== 1) fail("state", "unbalanced ATDL state stack");
  if (at !== bytes.length) fail("format", "trailing bytes after ATDL display list");
  return { page: { bounds: pageBounds, width: pageBounds.width, height: pageBounds.height }, commands };
}
