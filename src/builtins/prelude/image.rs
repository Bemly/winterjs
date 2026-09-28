//! WinterJS.image JS 面（prelude 分域；拼接顺序见 mod.rs，须在 namespace 之后）。
pub const IMAGE_JS: &str = r#"
{
  const __wjs_image_table = [
    { name: "png", mime: "image/png", decode: true, encode: true },
    { name: "jpeg", mime: "image/jpeg", decode: true, encode: true },
    { name: "gif", mime: "image/gif", decode: true, encode: true },
    { name: "webp", mime: "image/webp", decode: true, encode: true },
    { name: "tiff", mime: "image/tiff", decode: true, encode: true },
    { name: "tga", mime: "image/x-targa", decode: true, encode: true },
    { name: "bmp", mime: "image/bmp", decode: true, encode: true },
    { name: "ico", mime: "image/x-icon", decode: true, encode: true },
    { name: "hdr", mime: "image/vnd.radiance", decode: true, encode: true },
    { name: "exr", mime: "image/x-exr", decode: true, encode: true },
    { name: "pnm", mime: "image/x-portable-anymap", decode: true, encode: true },
    { name: "farbfeld", mime: "application/octet-stream", decode: true, encode: true },
    { name: "qoi", mime: "image/x-qoi", decode: true, encode: true },
    { name: "svg", mime: "image/svg+xml", decode: true, encode: false },
    { name: "jxl", mime: "image/jxl", decode: true, encode: false },
    { name: "dds", mime: "image/vnd-ms.dds", decode: false, encode: false },
  ];
  const __wjs_image_by_name = (f) => {
    const n = String(f).trim().toLowerCase();
    const alias = { jpg: "jpeg", tif: "tiff", ff: "farbfeld", svgz: "svg", ppm: "pnm", pgm: "pnm", pbm: "pnm", pam: "pnm" };
    return __wjs_image_table.find((r) => r.name === n || r.name === (alias[n] || ""));
  };
  // v1 取舍（文档记录）：decode 走 info+pixels 两遍（单 native 只返单值，
  // JSON 桥 + TypedArray 各一）；动画首帧；16 位截断 8 位；svg 输出解预乘。
  globalThis.WinterJS.image = {
    formats() { return __wjs_image_table.map((r) => ({ ...r })); },
    info(bytes, format) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.image.info requires Uint8Array bytes");
      return JSON.parse(__wjs_image_info(bytes, format === undefined ? undefined : String(format)));
    },
    decode(bytes, format, scale) {
      if (!(bytes instanceof Uint8Array)) throw new TypeError("WinterJS.image.decode requires Uint8Array bytes");
      let fmt = format;
      let sc = scale;
      if (typeof fmt === "number") { sc = fmt; fmt = undefined; }
      const info = this.info(bytes, fmt);
      const row = __wjs_image_by_name(info.format);
      if (!row || !row.decode) throw new TypeError(`WinterJS.image.decode: unsupported format '${info.format}'`);
      const data = __wjs_image_pixels(bytes, info.format, sc === undefined ? 1 : Number(sc));
      return { format: info.format, mime: info.mime, width: info.width, height: info.height, data };
    },
    encode(img, format, options) {
      if (!img || !(img.data instanceof Uint8Array)) {
        throw new TypeError("WinterJS.image.encode requires { data: Uint8Array, width, height }");
      }
      const row = __wjs_image_by_name(format);
      if (!row) throw new TypeError(`WinterJS.image.encode: unknown format '${format}'`);
      if (!row.encode) throw new TypeError(`WinterJS.image.encode: '${row.name}' has no encoder (decode only)`);
      return __wjs_image_encode(img.data, img.width, img.height, row.name, JSON.stringify(options ?? null));
    },
  };
}
"#;
