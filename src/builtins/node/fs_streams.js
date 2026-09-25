// ---- fs 流（10f：createReadStream 换 node ReadStream——真机 26 口径
// open(fd)/ready/data(Buffer)/end/close 事件序 + path/flags/autoClose/
// start/end 选项；chunk 为 Buffer（§4.83）。sync 底座偏差记档：'open' 的
// fd 恒 null（无真异步 fd 生命周期），sync 读错误在构造期抛而非 'error' 事件。
// createWriteStream 维持 Web 流外形（口径见下注）。----
class __ReadStream extends Readable {
  constructor(p, opts) {
    opts = __fsStreamOpts(opts);
    const enc = __fsEncoding(opts);
    // node validateOffset（non-number-arguments-throw 套件）：start/end 非 number
    // （含 '4' 字符串形）即 ARG_TYPE；NaN/小数/负数/超 MAX_SAFE 即 OUT_OF_RANGE；
    // end: Infinity 显式放行；start > end（end 非 Infinity）即 RangeError。
    // node 校验点在构造器（fd 形同样适用）。
    __fsValidateOffset(opts.start, "start");
    __fsValidateOffset(opts.end, "end");
    __fsValidateStartEnd(opts.start, opts.end);
    const hwm = opts.highWaterMark !== undefined ? Number(opts.highWaterMark) : 65536;
    const size = Number.isFinite(hwm) && hwm > 0 ? Math.floor(hwm) : 65536;
    // node 口径：autoDestroy 取自 autoClose（streams.js 逐字；autoClose:false
    // 时 finish 后不自毁、closed 恒 false，autoclose-option 套件点名）。
    // encoding 透传基类（read-stream-encoding/fd 套件：base64 等经 StringDecoder
    // 吐字符串，3 字节量子暂存由基类接管，push 侧恒给 Buffer）。
    const superOpts = { highWaterMark: size, autoDestroy: opts.autoClose !== false, emitClose: true };
    if (enc !== null) superOpts.encoding = enc;
    super(superOpts);
    // node 口径：fd 形下 path 不赋值（undefined），只无 fd 时由路径确立。
    this.path = undefined;
    this.__brand = "ReadStream";
    this.flags = opts.flags ?? "r";
    this.mode = opts.mode ?? 0o666;
    this.autoClose = opts.autoClose !== false;
    this.bytesRead = 0;
    this.fd = null;
    // node 口径：start/end 原值暴露（end 缺省 Infinity，inherit/read-stream.js
    // 点名 file.start/file.end；经 Proxy 的 __proto__ 形同样直读）。
    this.start = opts.start;
    this.end = opts.end === undefined ? Infinity : opts.end;
    if (opts.fd !== undefined && opts.fd !== null) {
      // 10f：fd 形（FileHandle.createReadStream / { fd }）——不 open，增量读；
      // start 定位读（__pos，fileNext 套件）；fd 为 FileHandle 时读/关走 handle 方法
      //（node streams.js FileHandleOperations 同构；write-stream-2 以 spy 断言）。
      if (opts.fs) {
        const e = new Error("The FileHandle with fs method is not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED"; throw e;
      }
      this.__fh = typeof opts.fd === "object" ? opts.fd : null;
      this.fd = this.__fh ? this.__fh.fd : opts.fd;
      this.__fdMode = true;
      this.__opened = false;
      this.__hwm = size;
      // fd 形定位读（inherit fileNext 套件：复用已读到尾的 fd + start:0 必须从头读；
      // node pos 语义：start 缺省走当前位置，显式 start 定位且随读推进）。
      this.__pos = opts.start !== undefined ? opts.start : null;
      if (opts.signal !== undefined) {
        // node validateAbortSignal：undefined 跳过，null/非 signal 即抛。
        if (opts.signal === null || typeof opts.signal.addEventListener !== "function") {
          const e = new TypeError("The 'signal' option must be an AbortSignal-like object");
          e.code = "ERR_INVALID_ARG_TYPE"; throw e;
        }
        if (opts.signal.aborted) {
          queueMicrotask(() => { if (!this.destroyed) this.destroy(__fsAbortErr(opts.signal.reason)); });
          return;
        }
        opts.signal.addEventListener("abort", () => this.destroy(__fsAbortErr(opts.signal.reason)), { once: true });
      }
      // node 口径：FileHandle 被（用户/他人）close 即毁流——handle 关后再读
      // 是 EBADF，不是可恢复错误（read-stream-file-handle data→close 用例）。
      // 位置：signal 校验之后——无效 signal 的构造器抛错不得积累监听。
      if (this.__fh && typeof this.__fh.on === "function") {
        this.__fh.on("close", () => { if (!this.destroyed) this.destroy(); });
      }
      this.__holdStream(opts);
      return;
    }
    this.path = p;
    // 真 open + 读盘不在构造期（__doOpen，A 段微任务先开、_read 回落兜底）：
    // 未使用即不碰 fs（防 fd 泄漏）；缺失文件走异步 'error'
    // （node 口径：构造期只做类型校验，存在性错异步派发）。
    // 用户 open 补丁（本类无 open 方法，函数值即补丁，patch-open 套件点名）：
    // 调补丁计数后跳过真实打开（补丁接管语义；本体不再读盘）。
    this.__bytes = null;
    this.__off = 0;
    this.__hwm = size;
    this.__opened = false;
    this.__openErr = null;
    // live 跟随记账（read-pos 套件）：快照尾的绝对偏移 + 是否显式 end。
    // 无显式 end 时耗尽不立即落定，见 _read（patch-open 补丁接管形不跟随）。
    // __snapEnd 在 __doOpen 落定（按钳制后 end+1）。
    this.__snapEnd = 0;
    this.__endOpt = opts.end;
    this.__holdStream(opts);
    // path 形真 fd 随 close 收（autoClose 语义，见 __closePathFd）。
    this.once("close", () => this.__closePathFd());
  }
  // 续命（sync 底座无原生句柄，循环提前退出即 close 永不到）：
  // 构造持有 → close 释放；autoClose 关/emitClose 关时 end 即静默终结亦释放
  // （close 不会来；后继 close() 由 __refed 旗防重；抛错路径不持有）。
  // A 段微任务：调用户 open 补丁（本类无 open 方法，函数值即补丁，
  // patch-open 套件点名）+ 从未使用即自释（只构造不读写不关的流不续命，
  // 否则 patch-open 子进程形永不退出）。
  __holdStream(opts) {
    this.__refed = true;
    this.__used = false;
    __wjs_fs_stream_ref();
    this.once("close", () => this.__unrefStream());
    this.once("end", () => { if (!this.autoClose || opts.emitClose === false) this.__unrefStream(); });
    queueMicrotask(() => {
      if (!this.__openCalled && typeof this.open === "function") {
        this.__openCalled = true;
        try { this.open(); } catch {}
        // 补丁接管：本体不读盘（置空，与 _read 首读分支同形，防回落误真开）。
        this.__bytes = new Uint8Array(0);
        this.__snapEnd = 0;
      }
      // 真开前置（node 口径：构造后即异步 open，不等消费侧流动——仅 error 监听
      // 的缺失文件同样异步 error；_read 懒触发够不着该形，故在此先开）。
      // 开败即就地递送一次 _read（走 __openErr 分发；成功则等流动消费）。
      if (!this.__fdMode && this.__bytes === null && !this.__openCalled) {
        this.__doOpen();
        if (this.__openErr) this._read();
      }
      if (!this.__used) this.__unrefStream();
    });
  }
  __unrefStream() {
    if (this.__refed) { this.__refed = false; __wjs_fs_stream_unref(); }
  }
  close(cb) {
    this.__used = true;
    // node ReadStream.close：毁流 → 'close'；裸 fd 在此收口（destroy 不带钩）。
    // fd 失效值 null（autoclose-option 套件点名；-1 系 FileHandle 口径，流不用）。
    if (this.__fdMode && !this.__fh && this.autoClose && this.fd != null && this.fd !== -1) {
      try { __wjs_fs_close(this.fd); } catch { }
      this.fd = null;
    }
    this.destroy();
    if (typeof cb === "function") this.once("close", cb);
    return this;
  }
  _read() {
    this.__used = true;
    // 用户 open 补丁（原型/实例函数值即补丁，patch-open 套件）：通常 A 段微任务
    // 已消费（置空 + __openCalled）；此分支为 _read 先到的回落（同形幂等）。
    if (!this.__openCalled && typeof this.open === "function") {
      this.__openCalled = true;
      try { this.open(); } catch {}
      this.__bytes = new Uint8Array(0);
      this.__snapEnd = 0;
    } else if (!this.__fdMode && this.__bytes === null && !this.__openCalled) {
      this.__doOpen();
    }
    // node 口径事件序 open → ready → data …：首次 _read 前派发（sync 底座下
    // 若走 microtask，流在监听器挂载的同一同步链上已流到 close，事件被
    // destroyed 早退吞掉）。open 带真 fd（test1 套件点名 typeof number）。
    if (!this.__opened) {
      this.__opened = true;
      if (this.__openErr) {
        // 路径 open/读盘失败：异步 error（构造期不抛），无 open 事件；
        // 收口走 errorOrDestroy。
        this.__streamError(this.__openErr);
        return;
      }
      this.emit("open", this.fd);
      this.emit("ready");
    }
    if (this.__fdMode) {
      const buf = Buffer.alloc(this.__hwm);
      if (this.__fh) {
        this.__fh.read(buf, 0, this.__hwm, this.__pos).then(
          (r) => {
            if (this.destroyed) return;
            if (!r || r.bytesRead <= 0) { this.push(null); return; }
            if (this.__pos !== null) this.__pos += r.bytesRead;
            this.bytesRead += r.bytesRead;
            this.push(r.bytesRead === r.buffer.byteLength ? r.buffer : Buffer.from(r.buffer.subarray(0, r.bytesRead)));
          },
          (e) => this.__streamError(e),
        );
        return;
      }
      let n;
      try { n = readSync(this.fd, buf, 0, this.__hwm, this.__pos); } catch (e) { this.__streamError(e); return; }
      if (n <= 0) {
        if (this.autoClose) { try { __wjs_fs_close(this.fd); } catch { } this.fd = null; }
        this.push(null);
        return;
      }
      if (this.__pos !== null) this.__pos += n;
      this.bytesRead += n;
      this.push(n === this.__hwm ? buf : Buffer.from(buf.subarray(0, n)));
      return;
    }
    if (this.__off >= this.__bytes.length) {
      // live 增长跟随（read-pos 套件）：无显式 end 时耗尽不立即落定——下个
      // macrotask 重查长度（让写者 timer 交错，真机异步读同款节奏），有增长
      // 即续读尾部（短读自然出现），无增长/ stat 失败才落定。显式 end、
      // patch-open 补丁接管形、已销毁即直接落定/丢弃。
      if (this.__endOpt === undefined && !this.__openCalled && !this.destroyed) {
        const self = this;
        setImmediate(() => {
          if (self.destroyed) return;
          let size;
          try { size = statSync(self.path).size; } catch { size = -1; }
          if (size > self.__snapEnd) {
            let tail = null;
            try { tail = __wjs_fs_read_file(self.path).subarray(self.__snapEnd); } catch {}
            if (tail && tail.length > 0) {
              self.__bytes = tail;
              self.__off = 0;
              self.__snapEnd += tail.length;
              self._read();
              return;
            }
          }
          self.push(null);
        });
        return;
      }
      this.push(null);
      return;
    }
    const end = Math.min(this.__bytes.length, this.__off + this.__hwm);
    // bytesRead 累加（test1 套件逐 data 断言；旧形置绝对值，多块即错）。
    // encoding 由基类 StringDecoder 接管（super encoding 透传）——push 侧恒给
    // Buffer，base64 量子暂存/刷尾全在基类。
    const chunk = this.__bytes.subarray(this.__off, end);
    this.bytesRead += chunk.length;
    this.push(Buffer.from(chunk));
    this.__off = end;
  }
  // 首个 _read 内真开：openSync 建真 fd（供 open 事件/file.fd 复用，autoClose:false
  // 留开）+ 经该 fd 全量读盘（sync 底座；live 跟随仍按快照+重查）。失败只记 __openErr
  // （缺失文件异步 error，构造期不抛——node 口径）。
  // 数据必经已开 fd 读（fifo 等不可重开：按径二次 open 会等新写者永挂——首版
  // openSync 建 fd 后又调 read_file 重开，fifo 套件现形，sample 实锤卡 open(2)）。
  __doOpen() {
    try {
      this.fd = Number(__fsCall("open", this.path, () =>
        __wjs_fs_open(this.path, __fsFlags(this.flags, "createReadStream"), this.mode)));
    } catch (e) {
      this.__openErr = e;
      this.__bytes = new Uint8Array(0);
      return;
    }
    const parts = [];
    let total = 0;
    try {
      while (true) {
        const buf = Buffer.alloc(65536);
        const n = readSync(this.fd, buf, 0, 65536, null);
        if (n <= 0) break;
        parts.push(Buffer.from(buf.subarray(0, n)));
        total += n;
      }
    } catch (e) {
      this.__openErr = e;
      this.__bytes = new Uint8Array(0);
      return;
    }
    const bytes = Buffer.concat(parts, total);
    const start = this.start !== undefined ? this.start : 0;
    let end = this.end !== Infinity ? this.end : bytes.length - 1;
    if (end >= bytes.length) end = bytes.length - 1;
    this.__bytes = bytes.subarray(start, end + 1);
    this.__off = 0;
    this.__snapEnd = end + 1;
  }
  // node errorOrDestroy 口径：autoClose 关时只派 error（不 destroy/close，fd
  // 保留——fd:13337 套件点名 closed/destroyed 恒 false）；否则 destroy
  // （error+close 走既有收口，fd 按 autoClose 关）。
  __streamError(e) {
    if (!this.autoClose) {
      this.__unrefStream();
      queueMicrotask(() => { if (!this.destroyed) this.emit("error", e); });
      return;
    }
    this.destroy(e);
  }
  // path 形真 fd 收口：autoClose 才关（false 留给用户复用，fileNext 套件），
  // 收后 fd 落 null（read-stream-err 套件点名）。
  __closePathFd() {
    if (this.__fdMode || !this.autoClose) return;
    if (typeof this.fd === "number" && this.fd >= 0) {
      try { __wjs_fs_close(this.fd); } catch {}
    }
    this.fd = null;
  }
}

// node 口径：autoClose 为原型访问器 + 非法接收者抛 ERR_INVALID_THIS
//（write-stream-autoclose-option 套件末行点名 `WriteStream.prototype.autoClose`）。
function __fsStreamAutoClose(proto, brand) {
  const bad = () => {
    const e = new TypeError(`Value of "this" must be of type ${brand}`);
    e.code = "ERR_INVALID_THIS"; throw e;
  };
  Object.defineProperty(proto, "autoClose", {
    get() { if (!this || this.__brand !== brand) bad(); return this.__autoClose; },
    set(v) { if (!this || this.__brand !== brand) bad(); this.__autoClose = v; },
    configurable: true,
  });
}
__fsStreamAutoClose(__ReadStream.prototype, "ReadStream");
// node legacy 形：fs.ReadStream(file) 无 new 可调（自 new）+ instanceof 成立——
// Proxy apply 转 construct。
export const ReadStream = new Proxy(__ReadStream, {
  apply(_t, _this, args) { return new __ReadStream(...args); },
});

export function createReadStream(p, opts) {
  // node 口径：{ fd } 形下 path 可 null（test-fs-promises-file-handle-read 点名）。
  if (!(opts && opts.fd != null)) p = __fsPath(p, "createReadStream");
  return new ReadStream(p, opts);
}
// WriteStream（10f，node 口径镜像 ReadStream）：open(fd)/ready/finish/close 事件序
// + path/flags/autoClose/bytesWritten；增量直写经默认导出（mock 可见），
// open/ready 于首个 _write/_final 前派发。
class __WriteStream extends Writable {
  constructor(p, opts) {
    opts = __fsStreamOpts(opts);
    __fsEncoding(opts);
    // node WriteStream 只验 start（end 非 WriteStream 面，忽略不验）。
    __fsValidateOffset(opts.start, "start");
    super({ autoDestroy: opts.autoClose !== false, emitClose: true });
    // node 口径（streams.js 逐字）：encoding 选项即 writable 默认编码
    // （write-stream-encoding 套件：base64 串经 pipe 进来按此解码落盘；
    // 基类 writable 层在 _write 前已按 defaultEncoding 解为 Buffer）。
    if (typeof opts.encoding === "string") this.setDefaultEncoding(opts.encoding);
    // node 口径：fd 形下 path 为 undefined（ReadStream 同口径）。
    this.path = undefined;
    this.__brand = "WriteStream";
    if (opts.fd === undefined || opts.fd === null) this.path = p;
    this.flags = opts.flags ?? "w";
    this.mode = opts.mode ?? 0o666;
    this.autoClose = opts.autoClose !== false;
    this.flush = __fsFlushOpt(opts);
    this.bytesWritten = 0;
    this.fd = null;
    this.__chunks = [];
    this.__opened = false;
    // 写位置跟踪（autoclose-option 套件：fd 形 start:0 即覆盖写；追加系恒
    // null 走 O_APPEND 游标；余下显式位置逐次推进）。
    this.__pos = (String(this.flags).startsWith("a") || opts.start === undefined) ? null : opts.start;
    if (opts.fd !== undefined && opts.fd !== null) {
      // 10f：fd 形（FileHandle.createWriteStream）——增量直写，非 path 攒块；
      // fd 为 FileHandle 时写/关走 handle 方法（FileHandleOperations 同构）。
      if (opts.fs) {
        const e = new Error("The FileHandle with fs method is not implemented");
        e.code = "ERR_METHOD_NOT_IMPLEMENTED"; throw e;
      }
      this.__fh = typeof opts.fd === "object" ? opts.fd : null;
      this.fd = this.__fh ? this.__fh.fd : opts.fd;
      this.__fdMode = true;
      if (this.__fh && typeof this.__fh.on === "function") {
        this.__fh.on("close", () => { if (!this.destroyed) this.destroy(); });
      }
    }
    // 续命（ReadStream.__holdStream 同口径；写侧静默终结点为 finish）。
    // A 段微任务同上（open 补丁计数 + 未用自释）。
    this.__refed = true;
    this.__used = false;
    __wjs_fs_stream_ref();
    this.once("close", () => this.__unrefStream());
    this.once("finish", () => { if (!this.autoClose || opts.emitClose === false) this.__unrefStream(); });
    queueMicrotask(() => {
      if (!this.__openCalled && typeof this.open === "function") {
        this.__openCalled = true;
        try { this.open(); } catch {}
      }
      if (!this.__used) this.__unrefStream();
    });
  }
  __unrefStream() {
    if (this.__refed) { this.__refed = false; __wjs_fs_stream_unref(); }
  }
  __emitOpen() {
    if (this.__opened) return;
    // node 口径：open 事件触发时文件已真实打开（options-immutable 套件：
    // 'open' 回调里 ReadStream 必须读得到该文件——旧假 open 不建文件即 ENOENT）。
    if (!this.__fdMode && this.fd === null) {
      this.fd = Number(__fsCall("open", this.path, () =>
        __wjs_fs_open(this.path, __fsFlags(this.flags, "createWriteStream"), this.mode)));
    }
    this.__opened = true;
    // 派发递延一轮（write-stream-end 套件：end() 后挂的 on('open') 仍须收到；
    // fd 同步已建，回调读文件不受影响）。
    queueMicrotask(() => { this.emit("open", null); this.emit("ready"); });
  }
  _write(chunk, enc, cb) {
    this.__used = true;
    this.__emitOpen();
    const u8 = __fsData(chunk, "createWriteStream");
    if (this.__fdMode) {
      if (this.__fh) {
        this.__fh.write(u8).then(
          (r) => { this.bytesWritten += r ? r.bytesWritten : u8.length; cb(); },
          (e) => cb(e),
        );
        return;
      }
      try {
        const n = this.__pos === null ? writeSync(this.fd, u8)
          : writeSync(this.fd, u8, 0, u8.length, this.__pos);
        this.bytesWritten += n;
        if (this.__pos !== null) this.__pos += n;
        cb();
      }
      catch (e) { cb(e); }
      return;
    }
    // 增量直写经默认导出（write-stream-err 套件：mock fs.write 可见——
    // require 补丁落在 __api 同一对象，直调本地 write 即绕过补丁）。
    // 成功才计 bytesWritten；错即 error 事件（第二块 BAM 口径）。
    __api.write(this.fd, u8, 0, u8.length, this.__pos, (e) => {
      if (!e) {
        this.bytesWritten += u8.length;
        if (this.__pos !== null) this.__pos += u8.length;
      }
      cb(e);
    });
  }
  _final(cb) {
    this.__used = true;
    this.__emitOpen();
    // 完成递延一轮（end() 后挂的 finish/close 监听仍须收到；base 在无积压时
    // 同步调 _final，同步 cb 即同步派发终结事件，write-stream-end 套件现形）。
    const done = (e) => queueMicrotask(() => cb(e));
    if (this.__fdMode) {
      // flush:true 即落盘（FileHandle 形走 handle.sync，裸 fd 直刷；错即 error）。
      const finishFd = () => {
        if (this.autoClose) {
          if (this.__fh) { this.__fh.close().catch(() => { }); }
          else { try { __wjs_fs_close(this.fd); } catch { } this.fd = null; }
        }
        done();
      };
      if (this.flush) {
        if (this.__fh) { this.__fh.sync().then(finishFd, (e) => done(e)); return; }
        if (typeof this.fd === "number" && this.fd >= 0) {
          try { fsyncSync(this.fd); } catch (e) { done(e); return; }
        }
      }
      finishFd();
      return;
    }
    // 增量面已逐块落盘（_write 直写）；收尾经默认导出 close（补丁可见，
    // write-stream-err/change-open 套件断 fd 同一性/mustCall）+ flush +
    // autoClose，错序与旧攒块面同（fsync 错仍关后 done(e)）。
    // close 不等回调即走（change-open 补丁不调回；真机同为 fire-and-forget）。
    const closeFinal = (after) => {
      if (this.autoClose && typeof this.fd === "number" && this.fd >= 0) {
        const fd = this.fd;
        this.fd = null;
        try { __api.close(fd); } catch {}
        after();
      } else {
        if (this.autoClose) this.fd = null;
        after();
      }
    };
    try {
      if (this.flush) __fsCall("fsync", this.path, () => __wjs_fs_fsync(this.fd, false));
    } catch (e) { closeFinal(() => done(e)); return; }
    this.__chunks.length = 0;
    closeFinal(done);
  }
  _destroy(err, cb) {
    this.__used = true;
    if (!this.__fdMode && typeof this.fd === "number" && this.fd >= 0) {
      try { __wjs_fs_close(this.fd); } catch { }
    }
    this.fd = null;
    cb(err);
  }
  close(cb) {
    this.__used = true;
    // node WriteStream.close 镜像 ReadStream.close：毁流 → 'close'；
    // 裸 fd 在此收口（double-close/close-without-callback 套件点名）。
    if (this.__fdMode && !this.__fh && this.autoClose && this.fd != null && this.fd !== -1) {
      try { __wjs_fs_close(this.fd); } catch { }
      this.fd = null;
    }
    this.destroy();
    if (typeof cb === "function") this.once("close", cb);
    return this;
  }
}

__fsStreamAutoClose(__WriteStream.prototype, "WriteStream");
export const WriteStream = new Proxy(__WriteStream, {
  apply(_t, _this, args) { return new __WriteStream(...args); },
});

export function createWriteStream(p, opts) {
  if (!(opts && opts.fd != null)) p = __fsPath(p, "createWriteStream");
  return new WriteStream(p, opts);
}
