//! Blob/File 全局（prelude 分域；拼接顺序见 mod.rs）。
pub const BLOB_JS: &str = r#"
// ---- Blob（Web 全局；fetch/consumers/node:internal/blob 共用；9b）----
const __wjs2_blobBytes = new WeakMap();
globalThis.Blob = class Blob {
  constructor(parts = [], options = {}) {
    const chunks = [];
    let size = 0;
    if (typeof parts === "string" || ArrayBuffer.isView(parts) || parts instanceof ArrayBuffer) {
      throw new TypeError("Blob parts must be an iterable");
    }
    for (const part of parts) {
      if (part instanceof Blob) {
        const u8 = __wjs2_blobBytes.get(part);
        chunks.push(u8); size += u8.byteLength;
      } else if (typeof part === "string") {
        const u8 = new TextEncoder().encode(part);
        chunks.push(u8); size += u8.byteLength;
      } else if (ArrayBuffer.isView(part)) {
        chunks.push(new Uint8Array(part.buffer, part.byteOffset, part.byteLength));
        size += part.byteLength;
      } else if (part instanceof ArrayBuffer) {
        chunks.push(new Uint8Array(part)); size += part.byteLength;
      } else if (part == null) {
        // spec: null/undefined part 跳过
      } else {
        const u8 = new TextEncoder().encode(String(part));
        chunks.push(u8); size += u8.byteLength;
      }
    }
    const bytes = new Uint8Array(size);
    let off = 0;
    for (const c of chunks) { bytes.set(c, off); off += c.byteLength; }
    __wjs2_blobBytes.set(this, bytes);
    const type = typeof options.type === "string" ? options.type : "";
    this.type = type.replace(/[^\x20-\x7E]/g, "").toLowerCase();
  }
  get size() { return __wjs2_blobBytes.get(this).byteLength; }
  slice(start, end, contentType) {
    const b = __wjs2_blobBytes.get(this);
    const s = start === undefined ? 0 : (start < 0 ? Math.max(b.byteLength + start, 0) : Math.min(start, b.byteLength));
    const e = end === undefined ? b.byteLength : (end < 0 ? Math.max(b.byteLength + end, 0) : Math.min(end, b.byteLength));
    const out = new Blob([], { type: contentType === undefined ? this.type : String(contentType) });
    __wjs2_blobBytes.set(out, s < e ? b.slice(s, e) : new Uint8Array(0));
    return out;
  }
  arrayBuffer() {
    return Promise.resolve(__wjs2_blobBytes.get(this).slice().buffer);
  }
  bytes() {
    return Promise.resolve(__wjs2_blobBytes.get(this).slice());
  }
  text() {
    return Promise.resolve(new TextDecoder().decode(__wjs2_blobBytes.get(this)));
  }
  stream() {
    const b = __wjs2_blobBytes.get(this);
    return new ReadableStream({
      start(c) { c.enqueue(b.slice()); c.close(); },
    });
  }
  get [Symbol.toStringTag]() { return "Blob"; }
};
// File（Web/Node 20+ 全局；jsdom/vitest 生态取此面）：Blob 子类 + name/lastModified。
// 状态复用 __wjs2_blobBytes（WeakMap 随原型链命中，§4.23 纪律）。
globalThis.File = class File extends Blob {
  constructor(parts = [], name, options = {}) {
    if (arguments.length < 2 || name === undefined) {
      throw new TypeError("File constructor: name is required");
    }
    super(parts, options);
    this.name = String(name);
    this.lastModified =
      typeof options.lastModified === "number" ? options.lastModified : Date.now();
  }
  get [Symbol.toStringTag]() { return "File"; }
};
"#;
