//! `node:internal/streams/duplexpair`（Node lib/internal/streams/duplexpair.js 逐字内嵌，MIT）。
/// 来源：nodejs/node `internal/streams/duplexpair.js`（MIT 头见源内）逐字内嵌；require → 垫片映射，
/// primordials → node:internal/primordials。偏差见 docs/plan.md Phase 9b。
pub const SOURCE: &str = r#"import { primordials } from 'node:internal/primordials';
import * as __m0 from 'node:internal/assert';
import * as __m1 from 'node:stream';

// CJS require 垫片（静态 spec → 内建模块 default 导出；循环依赖经懒访问解环）
const require = (spec) => __requireMap(spec);
function __requireMap(spec) {
  switch (spec) {
    case 'internal/assert': return __m0.default;
    case 'stream': return __m1.default;
  default: throw new Error('unmapped internal require: ' + spec);
  }
}

const module = { exports: { __proto__: null } };

'use strict';
const {
  Symbol,
} = primordials;

const assert = require('internal/assert');

const kCallback = Symbol('Callback');
const kInitOtherSide = Symbol('InitOtherSide');

// Node 原文为顶层 class（顶层 require('stream')）；此处懒初始化解环（ESM
// 静态边 duplexpair → node:stream 会把 node:stream 拖进中间求值），类体逐字。
let DuplexSide;
function initDuplexSide() {
  if (DuplexSide !== undefined) return DuplexSide;
  const { Duplex } = require('stream');
  DuplexSide = class DuplexSide extends Duplex {
  #otherSide = null;

  constructor(options) {
    super(options);
    this[kCallback] = null;
    this.#otherSide = null;
  }

  [kInitOtherSide](otherSide) {
    // Ensure this can only be set once, to enforce encapsulation.
    if (this.#otherSide === null) {
      this.#otherSide = otherSide;
    } else {
      assert(this.#otherSide === null);
    }
  }

  _read() {
    const callback = this[kCallback];
    if (callback) {
      this[kCallback] = null;
      callback();
    }
  }

  _write(chunk, encoding, callback) {
    assert(this.#otherSide !== null);
    assert(this.#otherSide[kCallback] === null);
    if (chunk.length === 0) {
      process.nextTick(callback);
    } else {
      this.#otherSide.push(chunk);
      this.#otherSide[kCallback] = callback;
    }
  }

  _final(callback) {
    this.#otherSide.on('end', callback);
    this.#otherSide.push(null);
  }


  _destroy(err, callback) {
    const otherSide = this.#otherSide;

    if (otherSide !== null && !otherSide.destroyed) {
      // Use nextTick to avoid crashing the current execution stack (like HTTP parser)
      process.nextTick(() => {
        if (otherSide.destroyed) return;

        if (err) {
          // Destroy the other side, without passing the 'err' object.
          // This closes the other side gracefully so it doesn't hang,
          // but prevents the "Unhandled error" crash.
          otherSide.destroy();
        } else {
          // Standard graceful close
          otherSide.push(null);
        }
      });
    }

    callback(err);
  }
  };
  return DuplexSide;
}

function duplexPair(options) {
  initDuplexSide();
  const side0 = new DuplexSide(options);
  const side1 = new DuplexSide(options);
  side0[kInitOtherSide](side1);
  side1[kInitOtherSide](side0);
  return [side0, side1];
}
module.exports = duplexPair;

export default module.exports;
"#;
