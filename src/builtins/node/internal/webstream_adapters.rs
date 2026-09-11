//! `node:internal/webstream_adapters`——`internal/webstreams/adapters` 薄适配
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"// （Node 版 1172 行重在逐字节泵与错误桥；本实现用可读泵回路，覆盖 toWeb/fromWeb
// 主路径；背压经 drain 等待，偏差记档。）
import * as __readable_ns from 'node:internal/streams/readable';
import * as __writable_ns from 'node:internal/streams/writable';
// 循环依赖：readable/writable →(懒)→ 本模块 →(懒)→ readable/writable；
// 类在调用期取（eval 顺序已就绪），静态边不再指向 node:stream（否则把
// node:stream 的求值拖进 pipeline/readable 中间，node:stream body 急切
// require pipeline 即 TDZ——2026-09-12 记坑）。
const Readable = () => __readable_ns.default;
const Writable = () => __writable_ns.default;
function newReadableStreamFromStreamReadable(streamReadable, options = {}) {
  const strategy = options.strategy ??
    { highWaterMark: streamReadable.readableHighWaterMark };
  const highWaterMark = typeof strategy === 'number' ? strategy : strategy.highWaterMark;
  return new ReadableStream({
    type: 'bytes',
    start(controller) {
      streamReadable.on('data', (chunk) => {
        // from()/objectMode 源的 chunk 可能是 string（Node 版经 Buffer 转换）
        if (typeof chunk === 'string') chunk = Buffer.from(chunk);
        if (typeof chunk === 'number' || chunk?.constructor === Object) chunk = Buffer.from(String(chunk));
        controller.enqueue(new Uint8Array(
          chunk.buffer, chunk.byteOffset, chunk.byteLength));
      });
      streamReadable.on('end', () => controller.close());
      streamReadable.on('error', (e) => controller.error(e));
      streamReadable.on('close', () => {
        if (!streamReadable.readableEnded) controller.close();
      });
    },
    pull(controller) {
      // pump: readable 流为推模式，无需主动拉
    },
    cancel(reason) {
      streamReadable.destroy(reason);
    },
  }, { highWaterMark });
}

function newStreamReadableFromReadableStream(readableStream, options = {}) {
  const reader = readableStream.getReader();
  return new (Readable())({
    encoding: options.encoding,
    highWaterMark: options.highWaterMark,
    async read() {
      try {
        const { done, value } = await reader.read();
        if (!done) this.push(value);
        else this.push(null);
      } catch (err) {
        this.destroy(err);
      }
    },
  });
}

function newWritableStreamFromStreamWritable(streamWritable, options = {}) {
  const highWaterMark = options?.highWaterMark ?? streamWritable.writableHighWaterMark;
  return new WritableStream({
    async start(controller) {},
    async write(chunk) {
      // chunk 为 Uint8Array / string
      await new Promise((resolve, reject) => {
        const cb = (err) => (err ? reject(err) : resolve());
        if (!streamWritable.write(chunk, cb)) {
          streamWritable.once('drain', cb); // 背压等待
        }
      });
    },
    async close() {
      // Node 原文：end() 无参 + finish 事件 resolve（end({}, cb) 的对象 chunk
      // 在非 objectMode 下 ERR_INVALID_ARG_TYPE——2026-09-12 记坑）
      await new Promise((resolve, reject) => {
        if (streamWritable.writableEnded) return resolve();
        streamWritable.once('finish', () => resolve());
        streamWritable.once('error', reject);
        streamWritable.end();
      });
    },
    async abort(reason) {
      streamWritable.destroy(reason);
    },
  }, { highWaterMark });
}

function newStreamWritableFromWritableStream(writableStream, options = {}) {
  const writer = writableStream.getWriter();
  return new (Writable())({
    highWaterMark: options.highWaterMark,
    write(chunk, encoding, callback) {
      writer.write(chunk).then(callback, (e) => callback(e));
    },
    final(callback) {
      writer.close().then(() => callback(), (e) => callback(e));
    },
  });
}

export {
  newReadableStreamFromStreamReadable,
  newStreamReadableFromReadableStream,
  newWritableStreamFromStreamWritable,
  newStreamWritableFromWritableStream,
};
export default {
  newReadableStreamFromStreamReadable,
  newStreamReadableFromReadableStream,
  newWritableStreamFromStreamWritable,
  newStreamWritableFromWritableStream,
};

"#;
