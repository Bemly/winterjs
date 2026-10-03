//! `node:internal/webstream_adapters`——`internal/webstreams/adapters` 薄适配
/// 源：nodejs/node（MIT）对应件最小实现；偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"// （Node 版 1172 行重在逐字节泵与错误桥；本实现用可读泵回路，覆盖 toWeb/fromWeb
// 主路径；背压经 drain 等待，偏差记档。）
import * as __readable_ns from 'node:internal/streams/readable';
import * as __writable_ns from 'node:internal/streams/writable';
import * as __duplex_ns from 'node:internal/streams/duplex';
// 循环依赖：readable/writable →(懒)→ 本模块 →(懒)→ readable/writable；
// 类在调用期取（eval 顺序已就绪），静态边不再指向 node:stream（否则把
// node:stream 的求值拖进 pipeline/readable 中间，node:stream body 急切
// require pipeline 即 TDZ——2026-09-12 记坑）。
const Readable = () => __readable_ns.default;
const Writable = () => __writable_ns.default;
function newReadableStreamFromStreamReadable(streamReadable, options = {}) {
  // R3b：node 原文校验（adapters.js：_readableState 形态门 + options 对象门 +
  // type 仅收 'bytes'/undefined，其余 ERR_INVALID_ARG_VALUE；旧实现无校验全收）。
  if (typeof streamReadable !== 'object' || streamReadable === null ||
      typeof streamReadable._readableState !== 'object') {
    const e = new TypeError(`The "streamReadable" argument must be of type stream.Readable. Received ${streamReadable === null ? 'null' : typeof streamReadable}`);
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  if (options !== undefined && (typeof options !== 'object' || options === null)) {
    const e = new TypeError(`The "options" argument must be of type object. Received ${options === null ? 'null' : typeof options}`);
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  if (options.type !== undefined && options.type !== 'bytes') {
    const e = new TypeError(`The "options.type" property must be one of 'bytes' or undefined. Received ${String(options.type)}`);
    e.code = 'ERR_INVALID_ARG_VALUE';
    throw e;
  }
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

// Duplex 双桥（Node internal/webstreams/adapters.js 原文结构移植；
// primordials 用内建等价：Promise.then 直调、process.nextTick 透传）。
// 来源：nodejs/node（MIT）。
function newReadableWritablePairFromDuplex(duplex, options = {}) {
  if (typeof duplex?._writableState !== "object" ||
      typeof duplex?._readableState !== "object") {
    const err = new TypeError(`The "duplex" argument must be of type stream.Duplex. Received ${duplex === null ? "null" : typeof duplex}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options !== undefined && options !== null && typeof options !== "object") {
    const err = new TypeError(`The "options" argument must be of type object. Received type ${typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const readableType = options.readableType ?? options.type;
  // R3b：`options.type` 别名告警（node 原文 getDeprecationWarningEmitter DEP0201；
  // 进程内一次；duplex 套件 expectWarning 点名文案 + 码）。
  if (options.readableType == null && options.type != null) {
    if (!globalThis.__wjs2_dep0201_warned) {
      globalThis.__wjs2_dep0201_warned = true;
      process.emitWarning(
        "Passing 'options.type' to Duplex.toWeb() is deprecated. To specify the ReadableStream type, use 'options.readableType'.",
        'DeprecationWarning', 'DEP0201');
    }
  }
  const isRd = typeof duplex.read === "function" && duplex.readable !== false;
  const isWr = typeof duplex.write === "function" && duplex.writable !== false;
  if (duplex.destroyed) {
    const writable = new WritableStream();
    const readable = new ReadableStream(readableType === undefined ? {} : { type: readableType });
    writable.close();
    readable.cancel();
    return { readable, writable };
  }
  const writable = isWr
    ? newWritableStreamFromStreamWritable(duplex, {})
    : new WritableStream();
  if (!isWr) writable.close();
  const readable = isRd
    ? newReadableStreamFromStreamReadable(duplex, readableType === undefined ? {} : { type: readableType })
    : new ReadableStream(readableType === undefined ? {} : { type: readableType });
  if (!isRd) readable.cancel();
  return { writable, readable };
}

function newStreamDuplexFromReadableWritablePair(pair = {}, options = {}) {
  if (pair === null || typeof pair !== "object") {
    const err = new TypeError(`The "pair" argument must be of type object. Received ${pair === null ? "null" : typeof pair}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const { readable: readableStream, writable: writableStream } = pair;
  if (!(readableStream instanceof ReadableStream)) {
    const err = new TypeError("The \"pair.readable\" argument must be of type ReadableStream.");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (!(writableStream instanceof WritableStream)) {
    const err = new TypeError("The \"pair.writable\" argument must be of type WritableStream.");
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (options !== undefined && options !== null && typeof options !== "object") {
    const err = new TypeError(`The "options" argument must be of type object. Received type ${typeof options}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  const {
    allowHalfOpen = false,
    objectMode = false,
    encoding,
    decodeStrings = true,
    highWaterMark,
    signal,
  } = options ?? {};
  if (typeof objectMode !== "boolean") {
    const err = new TypeError(`The "options.objectMode" property must be of type boolean. Received type ${typeof objectMode}`);
    err.code = "ERR_INVALID_ARG_TYPE";
    throw err;
  }
  if (encoding !== undefined && !Buffer.isEncoding(encoding)) {
    const err = new TypeError(`The "options.encoding" property must be a valid encoding. Received '${encoding}'`);
    err.code = "ERR_INVALID_ARG_VALUE";
    throw err;
  }
  const writer = writableStream.getWriter();
  const reader = readableStream.getReader();
  let writableClosed = false;
  let readableClosed = false;
  const duplex = new (__duplex_ns.default)({
    allowHalfOpen,
    highWaterMark,
    objectMode,
    encoding,
    decodeStrings,
    signal,
    writev(chunks, callback) {
      function done(error) {
        try {
          callback(error);
        } catch (error) {
          process.nextTick(() => duplex.destroy(error));
        }
      }
      Promise.resolve(writer.ready).then(
        () => Promise.all(chunks.map((data) => writer.write(data.chunk))).then(() => undefined),
        done).then(done, done);
    },
    write(chunk, encoding, callback) {
      if (typeof chunk === "string" && decodeStrings && !objectMode) {
        chunk = Buffer.from(chunk, encoding);
        chunk = new Uint8Array(chunk.buffer, chunk.byteOffset, chunk.byteLength);
      }
      function done(error) {
        try {
          callback(error);
        } catch (error) {
          duplex.destroy(error);
        }
      }
      Promise.resolve(writer.ready).then(() => writer.write(chunk)).then(done, done);
    },
    final(callback) {
      function done(error) {
        try {
          callback(error);
        } catch (error) {
          process.nextTick(() => duplex.destroy(error));
        }
      }
      if (!writableClosed) {
        Promise.resolve(writer.close()).then(done, done);
      }
    },
    read() {
      Promise.resolve(reader.read()).then(
        (chunk) => {
          if (chunk.done) duplex.push(null);
          else duplex.push(chunk.value);
        },
        (error) => duplex.destroy(error));
    },
    destroy(error, callback) {
      function done() {
        try {
          callback(error);
        } catch (error) {
          process.nextTick(() => { throw error; });
        }
      }
      async function closeWriter() {
        if (!writableClosed) await writer.abort(error);
      }
      async function closeReader() {
        if (!readableClosed) await reader.cancel(error);
      }
      if (!writableClosed || !readableClosed) {
        Promise.all([closeWriter(), closeReader()]).then(done, done);
        return;
      }
      done();
    },
  });
  Promise.resolve(writer.closed).then(
    () => {
      writableClosed = true;
      if (!duplex.writableEnded) duplex.destroy(new Error("ERR_STREAM_PREMATURE_CLOSE: premature close"));
    },
    (error) => {
      writableClosed = true;
      readableClosed = true;
      duplex.destroy(error);
    });
  Promise.resolve(reader.closed).then(
    () => { readableClosed = true; },
    (error) => {
      writableClosed = true;
      readableClosed = true;
      duplex.destroy(error);
    });
  return duplex;
}

export {
  newReadableStreamFromStreamReadable,
  newStreamReadableFromReadableStream,
  newWritableStreamFromStreamWritable,
  newStreamWritableFromWritableStream,
  newReadableWritablePairFromDuplex,
  newStreamDuplexFromReadableWritablePair,
};
export default {
  newReadableStreamFromStreamReadable,
  newStreamReadableFromReadableStream,
  newWritableStreamFromStreamWritable,
  newStreamWritableFromWritableStream,
  newReadableWritablePairFromDuplex,
  newStreamDuplexFromReadableWritablePair,
};

"#;
