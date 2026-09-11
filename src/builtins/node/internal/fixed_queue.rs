//! `node:internal/fixed_queue`（Node `lib/internal/fixed_queue.js` 近逐字移植，MIT）。
//! FixedSizeQueue 固定环队列链表；events 的 `on()` 异步迭代器用水位队列。
//! 引擎无关，唯一改动：primordials → 直接调用；`static #pool` 保留。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/fixed_queue (verbatim except primordials).
// Currently optimal queue size, tested on V8 6.0 - 6.6. Must be power of two.
const kSize = 2048;
const kMask = kSize - 1;

class FixedCircularBuffer {
  constructor() {
    this.bottom = 0;
    this.top = 0;
    this.list = new Array(kSize).fill(undefined);
    this.next = null;
  }

  isEmpty() {
    return this.top === this.bottom;
  }

  isFull() {
    return ((this.top + 1) & kMask) === this.bottom;
  }

  push(data) {
    this.list[this.top] = data;
    this.top = (this.top + 1) & kMask;
  }

  shift() {
    const nextItem = this.list[this.bottom];
    if (nextItem === undefined) return null;
    this.list[this.bottom] = undefined;
    this.bottom = (this.bottom + 1) & kMask;
    return nextItem;
  }
}

export default class FixedQueue {
  static #pool = [];

  constructor() {
    this.head = this.tail = FixedQueue.#pool.pop() ?? new FixedCircularBuffer();
  }

  isEmpty() {
    return this.head.isEmpty();
  }

  push(data) {
    if (this.head.isFull()) {
      // Head is full: Creates a new queue, sets the old queue's `.next` to it,
      // and sets it as the new main queue.
      this.head = this.head.next = FixedQueue.#pool.pop() ?? new FixedCircularBuffer();
    }
    this.head.push(data);
  }

  shift() {
    const tail = this.tail;
    const next = tail.shift();
    if (tail.isEmpty() && tail.next !== null) {
      // If there is another queue, it forms the new tail.
      this.tail = tail.next;
      tail.next = null;
      tail.bottom = 0;
      tail.top = 0;
      if (FixedQueue.#pool.length < 64) {
        FixedQueue.#pool.push(tail); // Recycle old tail
      }
    }
    return next;
  }
}
"#;
