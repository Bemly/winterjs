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
// ---- Phase 9c：同步面增补（link 系/时间戳/权限/access/fd 系/cp/opendir）----
function __fsTimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  if (typeof t === "number") return t;
  if (typeof t === "string") { const n = Number(t); if (Number.isFinite(n)) return n; }
  throw new TypeError(`${what}: time must be a number, string or Date`);
}
// truncate/ftruncate 的 len（truncate 套件逐字）：undefined → 0；非 number →
// ARG_TYPE 'len'；非整数（±1.5）→ OUT_OF_RANGE 'It must be an integer'。
function __fsLenArg(len) {
  if (len === undefined) return 0;
  if (typeof len !== "number") __vErrType("len", "number", len);
  if (!Number.isInteger(len)) {
    const e = new RangeError(`The value of "len" is out of range. It must be an integer. Received ${len}`);
    e.code = "ERR_OUT_OF_RANGE"; throw e;
  }
  return len;
}
// node lib/internal/fs/utils.js toUnixTimestamp——真机 26.8.2 实测口径：
// 数字串（含 '-1'）→ 数值原样；number 非有限 → ARG_TYPE；number 负 → 当前秒
//（实测 -1 → Date.now()/1000，非 throw）；Date → 秒浮点；日期串可 parse → 秒；
// 其余 ARG_TYPE（timestamp-parsing/utimes 套件）。
function __toUnixTimestamp(time, name = "time") {
  if (typeof time === "number") {
    if (!Number.isFinite(time)) __vErrType(name, "number or Date", time);
    return time < 0 ? Date.now() / 1000 : time;
  }
  if (typeof time === "string" && +time === time) return +time;
  if (time instanceof Date) return time.getTime() / 1000;
  if (typeof time === "string") {
    const d = new Date(time);
    if (!Number.isNaN(d.getTime())) return d.getTime() / 1000;
  }
  __vErrType(name, "number or Date", time);
}
// utimes 族的时间实参（秒口径，y2K38 套件）：Date → ms 直传（精度全保）；
// 其余经 toUnixTimestamp（秒）× 1000。
function __fsUtimeMs(t, what) {
  if (t instanceof Date) return t.getTime();
  return __toUnixTimestamp(t, what) * 1000;
}
function __fsModeNum(mode) {
  // node chmod 族：parseFileMode(mode, 'mode')（无 def；undefined → ARG_TYPE）。
  return __fsParseMode(mode);
}
export function accessSync(p, mode = 0) {
  if (mode !== undefined && typeof mode !== "number") {
    const e = new TypeError(`The "mode" argument must be of type number. Received ${typeof mode} (${JSON.stringify(String(mode))})`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  p = __fsPath(p, "access");
  __fsCall("access", p, () => __wjs_fs_access(p, mode));
}
export function truncateSync(p, len) {
  p = __fsPath(p, "truncate");
  const n = __fsLenArg(len);
  __fsCall("truncate", p, () => __wjs_fs_truncate(p, n));
}
export function utimesSync(p, atime, mtime) {
  p = __fsPath(p, "utimes");
  __fsCall("utimes", p, () => __wjs_fs_utimes(p, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
export function lutimesSync(p, atime, mtime) {
  p = __fsPath(p, "lutimes");
  __fsCall("lutimes", p, () => __wjs_fs_lutimes(p, __fsUtimeMs(atime, "atime"), __fsUtimeMs(mtime, "mtime")));
}
export function chmodSync(p, mode) {
  p = __fsPath(p, "chmod");
  __vModeArg(mode);
  __fsCall("chmod", p, () => __wjs_fs_chmod(p, __fsModeNum(mode)));
}
export function chownSync(p, uid, gid) {
  p = __fsPath(p, "chown");
  // node validateInt32（fchown 套件）：非 number → ARG_TYPE /uid|gid/；
  // 非整数（Infinity/NaN）→ OUT_OF_RANGE 'It must be an integer'；-1 = 不变更。
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("chown", p, () => __wjs_fs_chown(p, uid, gid));
}
export function fchownSync(fd, uid, gid) {
  __vFd(fd);
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("fchown", "", () => __wjs_fs_fchown(fd, uid, gid));
}
export function lchownSync(p, uid, gid) {
  p = __fsPath(p, "lchown");
  __vIntRange(uid, "uid", -1, 4294967295);
  __vIntRange(gid, "gid", -1, 4294967295);
  __fsCall("lchown", p, () => __wjs_fs_lchown(p, uid, gid));
}
// node：fs.lchmod 仅 macOS 存在（native 仅 macOS 注册，非 macOS 导出 undefined）；
// mode 走 parseFileMode（lchmod 套件校验矩阵）。
function lchmodSyncImpl(p, mode) {
  p = __fsPath(p, "lchmod");
  const n = __fsModeNum(mode);
  __fsCall("lchmod", p, () => __wjs_fs_lchmod(p, n));
}
export const lchmodSync = typeof __wjs_fs_lchmod === "function" ? lchmodSyncImpl : undefined;

export function linkSync(a, b) {
  a = __fsPath(a, "link");
  b = __fsPath(b, "link");
  __fsCall("link", a, () => __wjs_fs_link(a, b));
}
export function symlinkSync(target, p) {
  target = __fsPath(target, "symlink");
  p = __fsPath(p, "symlink");
  __fsCall("symlink", p, () => __wjs_fs_symlink(target, p));
}
export function readlinkSync(p, opts) {
  p = __fsPath(p, "readlink");
  const enc = __fsEncoding(opts);
  const link = __fsCall("readlink", p, () => __wjs_fs_read_link(p));
  if (enc === "buffer") return Buffer.from(link);
  return link;
}
// node lib/internal/fs/utils.js validateCpOptions 逐字（cp 校验族套件点名）：
// undefined → 缺省；非对象（含函数/数组/null）→ ARG_TYPE 'options'；
// 六布尔逐项校验（property 文案）；mode 走 copyFile 档 [0,7]；
// dereference+verbatimSymlinks 互斥 → INCOMPATIBLE_PAIR；filter 须函数。
function __cpValidateOptions(opts) {
  const def = { dereference: false, errorOnExist: false, filter: undefined, force: true, preserveTimestamps: false, recursive: false, verbatimSymlinks: false };
  if (opts === undefined) return { ...def };
  if (opts === null || typeof opts !== "object" || Array.isArray(opts)) {
    const e = new TypeError(`The "options" argument must be of type object. Received ${__vReceived(opts)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  const o = { ...def, ...opts };
  for (const k of ["dereference", "errorOnExist", "force", "preserveTimestamps", "recursive", "verbatimSymlinks"]) {
    if (typeof o[k] !== "boolean") {
      const e = new TypeError(`The "options.${k}" property must be of type boolean. Received ${__vReceived(o[k])}`);
      e.code = "ERR_INVALID_ARG_TYPE"; throw e;
    }
  }
  let mode = o.mode;
  if (mode === undefined || mode === null) mode = 0;
  else {
    if (typeof mode !== "number") __vErrType("mode", "number", mode);
    if (!Number.isInteger(mode)) {
      const e = new RangeError(`The value of "mode" is out of range. It must be an integer. Received ${mode}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
    if (mode < 0 || mode > 7) {
      const e = new RangeError(`The value of "mode" is out of range. It must be >= 0 && <= 7. Received ${mode}`);
      e.code = "ERR_OUT_OF_RANGE"; throw e;
    }
  }
  o.mode = mode;
  if (o.dereference === true && o.verbatimSymlinks === true) {
    const e = new TypeError('Option "dereference" cannot be used in combination with option "verbatimSymlinks"');
    e.code = "ERR_INCOMPATIBLE_OPTION_PAIR"; throw e;
  }
  if (o.filter !== undefined && typeof o.filter !== "function") {
    const e = new TypeError(`The "options.filter" property must be of type function. Received ${__vReceived(o.filter)}`);
    e.code = "ERR_INVALID_ARG_TYPE"; throw e;
  }
  return o;
}
function __cpSameOrSubdir(src, dst) {
  // node cpSyncCheckPaths 近似：同路径或 dst 落在 src 内 → EINVAL。
  // 比对走绝对路径归一（尾斜杠剥离；大小写敏感 posix 口径）。
  const norm = (p) => String(p).replace(/\/+$/, "") || "/";
  const a = norm(src), b = norm(dst);
  if (a === b) return "same";
  if (b.startsWith(a + "/")) return "subdir";
  return null;
}
function __cpEff(p) {
  // C++ checkPaths 穿透链接消解（dest-symlink-points-to-src 套件）：
  // 只消解父链（终段本身是链接时不得跟随——否则两条同目标链接被误判 identical，
  // copy-symlinks-to-existing-symlinks 套件点名）；余段回拼。
  const s = String(p).replace(/\/+$/, "");
  const j = s.lastIndexOf("/");
  const base = j < 0 ? s : s.slice(j + 1);
  let cur = j <= 0 ? (j === 0 ? "/" : ".") : s.slice(0, j);
  const tail = [];
  for (let i = 0; i < 64; i++) {
    let ok = true;
    try { lstatSync(cur); } catch { ok = false; }
    if (ok) break;
    const t = cur.replace(/\/+$/, "");
    const k = t.lastIndexOf("/");
    if (k <= 0) { tail.unshift(cur); cur = k === 0 ? "/" : "."; break; }
    tail.unshift(t.slice(k + 1));
    cur = t.slice(0, k) || "/";
  }
  let rbase;
  try { rbase = realpathSync(cur); } catch { rbase = cur; }
  return String(rbase).replace(/\/+$/, "") + "/" + [...tail, base].join("/");
}
export function cpSync(src, dst, opts) {
  const o = __cpValidateOptions(opts);
  src = __fsPath(src, "cp");
  dst = __fsPath(dst, "cp");
  if (o.filter) {
    const r = o.filter(src, dst);
    if (r && typeof r.then === "function") {
      const e = new TypeError(`The "filter" return value must be of type boolean. Received an instance of Promise`);
      e.code = "ERR_INVALID_RETURN_VALUE"; throw e;
    }
    if (!r) return;
  }
  const rel = __cpSameOrSubdir(src, dst) || __cpSameOrSubdir(__cpEff(src), __cpEff(dst));
  if (rel === "same") {
    const e = new Error(`src and dest cannot be the same ${src}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  if (rel === "subdir") {
    const e = new Error(`cannot copy ${src} to a subdirectory of self ${dst}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  // node getStats 口径：dereference 决定 stat/lstat；dest 恒 lstat（不跟随）。
  const srcStat = (o.dereference ? statSync : lstatSync)(src);
  const destStat = lstatSync(dst, { throwIfNoEntry: false });
  return __cpStats(src, dst, o, srcStat, destStat);
}
// node lib/internal/fs/cp/cp-sync.js getStats 分发（C++ checkPaths 由上层
// __cpSameOrSubdir 近似；EISDIR 非递归目录由本分发抛）。
function __cpStats(src, dst, o, srcStat, destStat) {
  if (srcStat.isDirectory()) {
    // C++ checkPaths 序：dest 存在且非目录 → DIR_TO_NON_DIR（先于递归门，
    // dir-to-file 套件无 opts 即点名此码而非 EISDIR）。
    if (destStat && !destStat.isDirectory()) {
      const e = new Error(`Cannot overwrite non-directory ${dst} with directory ${src}`);
      e.code = "ERR_FS_CP_DIR_TO_NON_DIR"; throw e;
    }
    if (!o.recursive) {
      const e = new Error(`Recursive option not enabled, cannot copy a directory: ${src}/`);
      e.code = "ERR_FS_EISDIR"; throw e;
    }
    return __cpOnDir(src, dst, o, destStat);
  }
  if (srcStat.isFile() || srcStat.isCharacterDevice() || srcStat.isBlockDevice()) {
    return __cpOnFile(src, dst, o, destStat);
  }
  if (srcStat.isSymbolicLink()) {
    return __cpOnLink(src, dst, o, destStat);
  }
  if (srcStat.isSocket()) {
    const e = new Error(`Cannot copy a socket file: ${dst}`);
    e.code = "ERR_FS_CP_SOCKET"; throw e;
  }
  if (srcStat.isFIFO()) {
    const e = new Error(`Cannot copy a FIFO pipe: ${dst}`);
    e.code = "ERR_FS_CP_FIFO_PIPE"; throw e;
  }
  const e = new Error(`Cannot copy an unknown file type: ${dst}`);
  e.code = "ERR_FS_CP_UNKNOWN"; throw e;
}
function __cpEexist(dst) {
  const e = new Error(`Target already exists: cp returned EEXIST (${dst} already exists) ${dst}`);
  e.code = "ERR_FS_CP_EEXIST"; e.syscall = "cp"; e.path = dst; e.errno = 17; throw e;
}
function __cpOnDir(src, dst, o, destStat) {
  if (destStat && !o.force) {
    // 存在即错（内容无冲突也抛，dir-exists-error-on-exist 套件点名）；
    // 无 errorOnExist 则合并（逐项 force 门复用）。
    if (o.errorOnExist) __cpEexist(dst);
  }
  __fsCall("cp", dst, () => __wjs_fs_mkdir(dst, true));
  for (const e of readdirSync(src, { withFileTypes: true })) {
    cpSync(__cpJoin(src, e.name), __cpJoin(dst, e.name), o);
  }
}
// 异步孪生（async-filter 套件：filter 可为 async 函数，逐项 await；
// 文件操作仍同步直调——本地 syscall，无等待点，语义等价）。
async function __cpAsync(src, dst, opts) {
  const o = __cpValidateOptions(opts);
  src = __fsPath(src, "cp");
  dst = __fsPath(dst, "cp");
  if (o.filter) {
    const r = await o.filter(src, dst);
    if (!r) return;
  }
  const rel = __cpSameOrSubdir(src, dst) || __cpSameOrSubdir(__cpEff(src), __cpEff(dst));
  if (rel === "same") {
    const e = new Error(`src and dest cannot be the same ${src}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  if (rel === "subdir") {
    const e = new Error(`cannot copy ${src} to a subdirectory of self ${dst}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  const srcStat = (o.dereference ? statSync : lstatSync)(src);
  const destStat = lstatSync(dst, { throwIfNoEntry: false });
  return __cpStatsA(src, dst, o, srcStat, destStat);
}
async function __cpStatsA(src, dst, o, srcStat, destStat) {
  if (srcStat.isDirectory()) {
    if (destStat && !destStat.isDirectory()) {
      const e = new Error(`Cannot overwrite non-directory ${dst} with directory ${src}`);
      e.code = "ERR_FS_CP_DIR_TO_NON_DIR"; throw e;
    }
    if (!o.recursive) {
      const e = new Error(`Recursive option not enabled, cannot copy a directory: ${src}/`);
      e.code = "ERR_FS_EISDIR"; throw e;
    }
    return __cpOnDirA(src, dst, o, destStat);
  }
  // 非目录分发与 __cpStats 同形（改一处改两处：文件/链接/socket/fifo/未知）。
  if (srcStat.isFile() || srcStat.isCharacterDevice() || srcStat.isBlockDevice()) {
    return __cpOnFile(src, dst, o, destStat);
  }
  if (srcStat.isSymbolicLink()) {
    return __cpOnLink(src, dst, o, destStat);
  }
  if (srcStat.isSocket()) {
    const e = new Error(`Cannot copy a socket file: ${dst}`);
    e.code = "ERR_FS_CP_SOCKET"; throw e;
  }
  if (srcStat.isFIFO()) {
    const e = new Error(`Cannot copy a FIFO pipe: ${dst}`);
    e.code = "ERR_FS_CP_FIFO_PIPE"; throw e;
  }
  const e = new Error(`Cannot copy an unknown file type: ${dst}`);
  e.code = "ERR_FS_CP_UNKNOWN"; throw e;
}
async function __cpOnDirA(src, dst, o, destStat) {
  if (destStat && !o.force) {
    if (o.errorOnExist) __cpEexist(dst);
  }
  __fsCall("cp", dst, () => __wjs_fs_mkdir(dst, true));
  for (const e of readdirSync(src, { withFileTypes: true })) {
    await __cpAsync(__cpJoin(src, e.name), __cpJoin(dst, e.name), o);
  }
}
function __cpOnFile(src, dst, o, destStat) {
  if (!destStat) {
    // Node cp 建缺失父目录（file-to-file 套件：dest 父级不存在仍成功）。
    __fsCall("cp", dst, () => __wjs_fs_mkdir(__cpDirname(dst), true));
    __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
    return;
  }
  // 文件拷向目录 → NON_DIR_TO_DIR（file-to-dir 套件；直拷报 EISDIR 即错码）。
  if (destStat.isDirectory()) {
    const e = new Error(`Cannot overwrite directory ${dst} with non-directory ${src}`);
    e.code = "ERR_FS_CP_NON_DIR_TO_DIR"; throw e;
  }
  if (o.force) {
    // Node C++ override 语义：dest 为 symlink 时先摘除再拷（dereference 套件：
    // file-over-symlinked-dir 后 dest 为文件非链接；直拷会穿透写进目标目录）。
    let dl = null;
    try { dl = lstatSync(dst); } catch { dl = null; }
    if (dl && dl.isSymbolicLink()) {
      __fsCall("unlink", dst, () => __wjs_fs_unlink(dst));
    }
    __fsCall("copyfile", src, () => __wjs_fs_copy_file(src, dst));
    return;
  }
  if (o.errorOnExist) __cpEexist(dst);
  // !force && !errorOnExist → 静默跳过。
}
// node onLink（cp-sync.js 逐字）：verbatim 关时相对链接消解为绝对；
// 不存在直建；存在分三路（非链接穿透建→EEXIST 门；双向 subdir 检查；否则换链）。
function __cpOnLink(src, dst, o, destStat) {
  let resolvedSrc = readlinkSync(src);
  if (!o.verbatimSymlinks && !__cpIsAbs(resolvedSrc)) {
    resolvedSrc = __cpResolve(__cpDirname(src), resolvedSrc);
  }
  if (!destStat) {
    __fsCall("cp", dst, () => __wjs_fs_mkdir(__cpDirname(dst), true));
    __fsCall("symlink", dst, () => __wjs_fs_symlink(resolvedSrc, dst));
    return;
  }
  let resolvedDest;
  try {
    resolvedDest = readlinkSync(dst);
  } catch (err) {
    if (err && (err.code === "EINVAL" || err.code === "UNKNOWN")) {
      // dest 存在但非链接：Node 原文直调 symlinkSync（不摘除）——
      // 恒 EEXIST（copy-symlink-over-file 套件 force 缺省仍 EEXIST）。
      __fsCall("symlink", dst, () => __wjs_fs_symlink(resolvedSrc, dst));
      return;
    }
    throw err;
  }
  if (!__cpIsAbs(resolvedDest)) {
    resolvedDest = __cpResolve(__cpDirname(dst), resolvedDest);
  }
  // Node 原文门：仅 src 链接指向目录时同址即 EINVAL（文件链接复拷是
  // unlink+重建无操作；copy-symlinks-to-existing-symlinks 套件点名）。
  let __srcIsDir = false;
  try { __srcIsDir = statSync(src).isDirectory(); } catch { __srcIsDir = false; }
  if (__srcIsDir && __cpIsSubdir(resolvedSrc, resolvedDest)) {
    const e = new Error(`cannot copy ${resolvedSrc} to a subdirectory of self ${resolvedDest}`);
    e.code = "ERR_FS_CP_EINVAL"; throw e;
  }
  // dest 链接指向 src 内部且 src 为目录 → 覆盖即删源（先拦）。
  let dstStat = null;
  try { dstStat = statSync(dst); } catch { dstStat = null; }
  if (dstStat && dstStat.isDirectory() && __cpIsSubdir(resolvedDest, resolvedSrc)) {
    const e = new Error(`cannot overwrite ${resolvedDest} with ${resolvedSrc}`);
    e.code = "ERR_FS_CP_SYMLINK_TO_SUBDIRECTORY"; throw e;
  }
  __fsCall("unlink", dst, () => __wjs_fs_unlink(dst));
  __fsCall("symlink", dst, () => __wjs_fs_symlink(resolvedSrc, dst));
}
// posix 路径小件（cp 链接消解专用；.. 不出根）。
function __cpIsAbs(p) { return String(p).startsWith("/"); }
function __cpDirname(p) {
  const s = String(p).replace(/\/+$/, "");
  const i = s.lastIndexOf("/");
  if (i < 0) return ".";
  if (i === 0) return "/";
  return s.slice(0, i);
}
function __cpJoin(a, b) {
  return String(a).replace(/\/+$/, "") + "/" + String(b).replace(/^\/+/, "");
}
function __cpResolve(base, rel) {
  const out = [];
  for (const q of String(base + "/" + rel).split("/")) {
    if (q === "" || q === ".") continue;
    if (q === "..") { out.pop(); continue; }
    out.push(q);
  }
  return "/" + out.join("/");
}
function __cpIsSubdir(parent, child) {
  const norm = (p) => String(p).replace(/\/+$/, "") || "/";
  const a = norm(parent), b = norm(child);
  return b === a || b.startsWith(a + "/");
}
// fd 系（openSync 合成 fd，自 3 起单调，不复用最小号，记档）
export function openSync(p, flags, mode) {
  // node 顺序：stringToFlags → parseFileMode(mode, 'mode', 0o666) → path 校验。
  const f = __fsFlags(flags, "open");
  const m = __fsParseMode(mode, 0o666);
  p = __fsPath(p, "open");
  return __fsCall("open", p, () => Number(__wjs_fs_open(p, f, m)));
}
export function closeSync(fd) {
  __vFd(fd);
  __fsCall("close", "", () => __wjs_fs_close(fd));
}
