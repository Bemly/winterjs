//! `node:internal/zip/entry`（Node lib/internal/zip/entry.js 逐字内嵌，MIT）。
//! ZipEntry: a single archive member。
/// 来源：nodejs/node v26.8.2 `lib/internal/zip/entry.js`（MIT）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。
/// 偏差：`internalBinding('zlib').crc32` 走 `__wjs2_zlib_crc32` 全局 native（见 dos/entry 内包装）。
/// §0.9 按域分块：`entry_head.js`（导入/垫片/辅助）+ `entry_class.js`（ZipEntry 本体与导出），
/// concat 字节恒等（切分脚本断言 + `node --check`）。
pub const SOURCE: &str = concat!(
    include_str!("entry_head.js"),
    include_str!("entry_class.js"),
);
