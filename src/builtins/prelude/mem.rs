//! 本体内存 JS 面：`WinterJS2.memory/alloc/unsafe*`（prelude 分域）。
//! 薄壳规则：校验在调 native 之前；unsafe* 未授权即 `--allow-ffi` 可读错。
pub const MEM_JS: &str = r#"
{
  const __mem_face = {
    memory() { return JSON.parse(__wjs2_mem_info()); },
    alloc(size) {
      if (typeof size !== "number" || !Number.isInteger(size) || size < 0 || size > 67108864) throw new TypeError("mem alloc requires a non-negative integer within 64MB");
      return __wjs2_mem_alloc(size);
    },
    unsafeAlloc(size) {
      if (typeof size !== "number" || !Number.isInteger(size) || size < 0 || size > 67108864) throw new TypeError("mem unsafeAlloc requires a non-negative integer within 64MB");
      return __wjs2_mem_unsafe_alloc(size);
    },
    unsafeSize(id) { return __wjs2_mem_unsafe_size(Number(id)); },
    unsafeWrite(id, off, data) {
      let u8 = data;
      if (typeof data === "string") u8 = new TextEncoder().encode(data);
      else if (data instanceof ArrayBuffer) u8 = new Uint8Array(data);
      else if (ArrayBuffer.isView(data)) u8 = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      return __wjs2_mem_unsafe_write(Number(id), Number(off), u8);
    },
    unsafeRead(id, off, len) { return __wjs2_mem_unsafe_read(Number(id), Number(off), Number(len)); },
    unsafeFree(id) { return __wjs2_mem_unsafe_free(Number(id)); },
    unsafeList() { return JSON.parse(__wjs2_mem_unsafe_list()); },
  };
  try {
    if (globalThis.WinterJS2 && globalThis.WinterJS2.memory === undefined) {
      globalThis.WinterJS2.memory = __mem_face.memory;
      globalThis.WinterJS2.alloc = __mem_face.alloc;
      globalThis.WinterJS2.unsafeAlloc = __mem_face.unsafeAlloc;
      globalThis.WinterJS2.unsafeSize = __mem_face.unsafeSize;
      globalThis.WinterJS2.unsafeWrite = __mem_face.unsafeWrite;
      globalThis.WinterJS2.unsafeRead = __mem_face.unsafeRead;
      globalThis.WinterJS2.unsafeFree = __mem_face.unsafeFree;
      globalThis.WinterJS2.unsafeList = __mem_face.unsafeList;
    }
  } catch {}
}
"#;
