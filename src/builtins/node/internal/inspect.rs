//! `node:internal/util/inspect`（Node `lib/internal/util/inspect.js` 子集移植，MIT）。
//!
//! 忠实移植：`format`/`formatWithOptionsInternal`（占位符逐字）、
//! `identicalSequenceRange`、`stripVTControlCharacters`+`ansi` 正则、
//! `getStringWidth`（非 Intl 路径）、字符串转义与数字格式化。
//!
//! 偏差（按 9a 验收需要裁剪，逐条记录）：
//! - `inspect` 为务实重写（非 3000 行逐字）：支持 depth/circular/customInspect/
//!   getters/maxArrayLength/maxStringLength/类型形状（数组洞/Map/Set/TypedArray/
//!   Date/RegExp/Error/函数/类名/`[Symbol]` 键），**不做 breakLength 换行**
//!   （恒单行）、Promise 恒 `<pending>`（无 JS 内态 API）、ArrayBuffer 不打
//!   `[Uint8Contents]`、numericSeparator 恒关、showProxy/showHidden 忽略。
//! - 颜色支持仅存接口（`stylizeNoColor` 恒等；colors 选项忽略），错误消息路径
//!   全部无色，与 9a 断言无交。

/// 内嵌 ESM 源。
pub const SOURCE: &str = r#"
// Copyright Joyent, Inc. and other Node contributors. MIT.
// Port of node:internal/util/inspect (subset; deviations documented in module docs).
const inspectDefaultOptions = Object.freeze({
  showHidden: false, depth: 2, colors: false, customInspect: true,
  maxArrayLength: 100, maxStringLength: 10000, breakLength: 80,
  compact: 3, sorted: false, getters: false, numericSeparator: false,
});
const stylizeNoColor = (str, _style) => str;
const ansi = new RegExp(
  '(?:\\u001B\\][\\s\\S]*?(?:\\u0007|\\u001B\\u005C|\\u009C))' +
  '|[\\u001B\\u009B][[\\]()#;?]*' +
  '(?:\\d{1,4}(?:[;:]\\d{0,4})*)?' +
  '[\\dA-PR-TZcf-nq-uy=><~]', 'g',
);
const kCustom = Symbol.for('nodejs.util.inspect.custom');

function stripVTControlCharacters(str) {
  if (typeof str !== 'string') throw new TypeError('str must be a string');
  if (str.indexOf('\u001B') === -1 && str.indexOf('\u009B') === -1) return str;
  return str.replace(ansi, '');
}

function isFullWidthCodePoint(code) {
  return code >= 0x1100 && (
    code <= 0x115f || code === 0x2329 || code === 0x232a ||
    (code >= 0x2e80 && code <= 0x3247 && code !== 0x303f) ||
    (code >= 0x3250 && code <= 0x4dbf) ||
    (code >= 0x4e00 && code <= 0xa4c6) ||
    (code >= 0xa960 && code <= 0xa97c) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe10 && code <= 0xfe19) ||
    (code >= 0xfe30 && code <= 0xfe6b) ||
    (code >= 0xff01 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1b000 && code <= 0x1b001) ||
    (code >= 0x1f200 && code <= 0x1f251) ||
    (code >= 0x1f300 && code <= 0x1f64f) ||
    (code >= 0x20000 && code <= 0x3fffd));
}

function isZeroWidthCodePoint(code) {
  return code <= 0x1f || (code >= 0x7f && code <= 0x9f) ||
    (code >= 0x300 && code <= 0x36f) ||
    (code >= 0x200b && code <= 0x200f) ||
    (code >= 0x20d0 && code <= 0x20ff) ||
    (code >= 0xfe00 && code <= 0xfe0f) ||
    (code >= 0xfe20 && code <= 0xfe2f) ||
    (code >= 0xe0100 && code <= 0xe01ef);
}

function getStringWidth(str, removeControlChars = true) {
  let width = 0;
  if (removeControlChars) str = stripVTControlCharacters(str);
  str = str.normalize('NFC');
  for (const char of str) {
    const code = char.codePointAt(0);
    if (isFullWidthCodePoint(code)) width += 2;
    else if (!isZeroWidthCodePoint(code)) width++;
  }
  return width;
}

// ── format（逐字移植；颜色路径以 NoColor 恒等收束）────────────────────────
function formatNumberNoColor(number) {
  return Object.is(number, -0) ? '-0' : `${number}`;
}
function formatBigIntNoColor(bigint) {
  return `${bigint}n`;
}
function tryStringify(obj) {
  try { return JSON.stringify(obj); } catch { return '[Circular]'; }
}

function formatWithOptionsInternal(inspectOptions, args) {
  const first = args[0];
  let a = 0;
  let str = '';
  let join = '';

  if (typeof first === 'string') {
    if (args.length === 1) return first;
    let tempStr;
    let lastPos = 0;
    for (let i = 0; i < first.length - 1; i++) {
      if (first.charCodeAt(i) === 37) { // '%'
        const nextChar = first.charCodeAt(++i);
        if (a + 1 !== args.length) {
          switch (nextChar) {
            case 115: { // 's'
              const tempArg = args[++a];
              if (typeof tempArg === 'number') tempStr = formatNumberNoColor(tempArg);
              else if (typeof tempArg === 'bigint') tempStr = formatBigIntNoColor(tempArg);
              else if (typeof tempArg !== 'object' || tempArg === null ||
                       typeof tempArg.toString !== 'function') {
                tempStr = String(tempArg);
              } else {
                tempStr = inspect(tempArg, { ...inspectOptions, compact: 3, colors: false, depth: 0 });
              }
              break;
            }
            case 106: tempStr = tryStringify(args[++a]); break; // 'j'
            case 100: { // 'd'
              const tempNum = args[++a];
              if (typeof tempNum === 'bigint') tempStr = formatBigIntNoColor(tempNum);
              else if (typeof tempNum === 'symbol') tempStr = 'NaN';
              else tempStr = formatNumberNoColor(Number(tempNum));
              break;
            }
            case 79: tempStr = inspect(args[++a], inspectOptions); break; // 'O'
            case 111: // 'o'
              tempStr = inspect(args[++a], { ...inspectOptions, showHidden: true, showProxy: true, depth: 4 });
              break;
            case 105: { // 'i'
              const tempInteger = args[++a];
              if (typeof tempInteger === 'bigint') tempStr = formatBigIntNoColor(tempInteger);
              else if (typeof tempInteger === 'symbol') tempStr = 'NaN';
              else tempStr = formatNumberNoColor(Number.parseInt(tempInteger));
              break;
            }
            case 102: { // 'f'
              const tempFloat = args[++a];
              if (typeof tempFloat === 'symbol') tempStr = 'NaN';
              else tempStr = formatNumberNoColor(Number.parseFloat(tempFloat));
              break;
            }
            case 99: a += 1; tempStr = ''; break; // 'c'
            case 37: // '%'
              str += first.slice(lastPos, i);
              lastPos = i + 1;
              continue;
            default: continue; // 非法占位符原样保留
          }
          if (lastPos !== i - 1) str += first.slice(lastPos, i - 1);
          str += tempStr;
          lastPos = i + 1;
        } else if (nextChar === 37) {
          str += first.slice(lastPos, i);
          lastPos = i + 1;
        }
      }
    }
    if (lastPos !== 0) {
      a++;
      join = ' ';
      if (lastPos < first.length) str += first.slice(lastPos);
    }
  }

  while (a < args.length) {
    const value = args[a];
    str += join;
    str += typeof value !== 'string' ? inspect(value, inspectOptions) : value;
    join = ' ';
    a++;
  }
  return str;
}

function format(...args) {
  return formatWithOptionsInternal(undefined, args);
}

function formatWithOptions(inspectOptions, ...args) {
  if (inspectOptions === null || Array.isArray(inspectOptions) || typeof inspectOptions !== 'object') {
    throw new TypeError('The "inspectOptions" argument must be of type Object.' +
      `${inspectOptions === null ? ' Received null' : Array.isArray(inspectOptions) ? ' Received an instance of Array' : ` Received type ${typeof inspectOptions}`}`);
  }
  return formatWithOptionsInternal(inspectOptions, args);
}

// ── identicalSequenceRange（逐字移植；events enhanceStackTrace 用）────────
function identicalSequenceRange(a, b) {
  for (let i = 0; i < a.length - 3; i++) {
    const pos = b.indexOf(a[i]);
    if (pos !== -1) {
      const rest = b.length - pos;
      if (rest > 3) {
        let len = 1;
        const maxLen = Math.min(a.length - i, rest);
        while (maxLen > len && a[i + len] === b[pos + len]) len++;
        if (len > 3) return [len, i];
      }
    }
  }
  return [0, 0];
}

// ── inspect（务实重写，偏差见模块头注）──────────────────────────────────
const strEscapeSequencesReplacer = /[\x00-\x1f\x27\x5c\x7f-\x9f]|[\ud800-\udbff](?![\udc00-\udfff])|(?<![\ud800-\udbff])[\udc00-\udfff]/g;
const keyStrEscape = /[\x00-\x1f\x27\x5c\x7f-\x9f]/g;
const escapeSequences = new Map([
  [0, '\\0'], [1, '\\x01'], [2, '\\x02'], [3, '\\x03'], [4, '\\x04'], [5, '\\x05'],
  [6, '\\x06'], [7, '\\a'], [8, '\\b'], [9, '\\t'], [10, '\\n'], [11, '\\v'],
  [12, '\\f'], [13, '\\r'], [14, '\\x0e'], [15, '\\x0f'], [16, '\\x10'], [17, '\\x11'],
  [18, '\\x12'], [19, '\\x13'], [20, '\\x14'], [21, '\\x15'], [22, '\\x16'], [23, '\\x17'],
  [24, '\\x18'], [25, '\\x19'], [26, '\\x1a'], [27, '\\e'], [28, '\\x1c'], [29, '\\x1d'],
  [30, '\\x1e'], [31, '\\x1f'], [127, '\\x7f'],
]);

function escapeFn(s) {
  const code = s.charCodeAt(0);
  if (escapeSequences.has(code)) return escapeSequences.get(code);
  if (code < 0x20 || (code >= 0x7f && code <= 0x9f)) {
    return '\\x' + code.toString(16).padStart(2, '0');
  }
  return s; // 孤代理：原样（Node 为 \\u 转义，接受偏差）
}

function inspectString(value, ctx) {
  let trailer = '';
  if (value.length > ctx.maxStringLength) {
    const remaining = value.length - ctx.maxStringLength;
    value = value.slice(0, ctx.maxStringLength);
    trailer = `... ${remaining} more character${remaining > 1 ? 's' : ''}`;
  }
  return `'${value.replace(strEscapeSequencesReplacer, escapeFn)}'` + trailer;
}

function formatNumber(value) {
  return Object.is(value, -0) ? '-0' : `${value}`;
}

function formatPrimitive(value, ctx) {
  switch (typeof value) {
    case 'string': return inspectString(value, ctx);
    case 'number': return formatNumber(value);
    case 'bigint': return `${value}n`;
    case 'boolean': return `${value}`;
    case 'undefined': return 'undefined';
    case 'symbol': return value.toString();
    default: return Object.is(value, null) ? 'null' : 'unknown';
  }
}

function constructorName(obj) {
  let proto = Object.getPrototypeOf(obj);
  while (proto !== null) {
    const ctor = proto.constructor;
    if (typeof ctor === 'function' && ctor.name !== '') return ctor.name;
    proto = Object.getPrototypeOf(proto);
  }
  return null;
}

function getFunctionPrefix(fn) {
  if (fn.constructor?.name === 'AsyncFunction') return 'AsyncFunction';
  if (fn.constructor?.name === 'GeneratorFunction') return 'GeneratorFunction';
  if (fn.constructor?.name === 'AsyncGeneratorFunction') return 'AsyncGeneratorFunction';
  return 'Function';
}

function prefixOf(value) {
  const name = constructorName(value);
  if (name === null || name === 'Object') return '';
  if (name === 'Array') return '';
  return name + ' ';
}

// 普通对象（含 null 原型）主体：`{ a: 1 }` / 空键 `{}`。
function formatPlainObject(value, ctx, depth, seen) {
  const keys = Reflect.ownKeys(value);
  const entries = [];
  for (const key of keys) {
    if (ctx.maxArrayLength !== Infinity && entries.length >= ctx.maxArrayLength) break;
    let desc;
    try { desc = Object.getOwnPropertyDescriptor(value, key); } catch { continue; }
    if (desc === undefined) continue;
    const name = typeof key === 'symbol' ? `[${key.toString()}]` : key;
    let valStr;
    if (desc.get !== undefined || desc.set !== undefined) {
      const hasGetter = desc.get !== undefined;
      const hasSetter = desc.set !== undefined;
      if (ctx.getters === true || ctx.getters === 'always') {
        try { valStr = inspect2(hasGetter ? desc.get.call(value) : undefined, ctx, depth + 1, seen); }
        catch (err) { valStr = `[Exception: ${err?.message ?? err}]`; }
      } else {
        valStr = hasGetter && hasSetter ? '[Getter/Setter]' : hasGetter ? '[Getter]' : '[Setter]';
      }
    } else {
      valStr = inspect2(desc.value, ctx, depth + 1, seen);
    }
    const shown = typeof key === 'symbol' ? name :
      (/^[A-Za-z_$][\w$]*$/.test(name) ? name : JSON.stringify(name));
    entries.push(`${shown}: ${valStr}`);
  }
  const body = entries.length === 0 ? '{}' : `{ ${entries.join(', ')} }`;
  const p = prefixOf(value) || (Object.getPrototypeOf(value) === null ? '[Object: null prototype] ' : '');
  return `${p}${body}`;
}

function inspect2(value, ctx, depth, seen) {
  // 循环引用：Node 形 `​[Circular * n]`（n = 1 起）
  if (typeof value === 'object' && value !== null) {
    const idx = seen.indexOf(value);
    if (idx !== -1) return `[Circular * ${idx + 1}]`;
  }
  if (typeof value !== 'object' || value === null) return formatPrimitive(value, ctx);
  if (depth < 0) return Array.isArray(value) ? '[Array]' : '[Object]';

  // custom inspect（customInspect:false 关闭；返回自身则继续普通格式化；异常传播）
  if (ctx.customInspect && typeof value[kCustom] === 'function') {
    const ret = value[kCustom](depth, { ...ctx, stylize: stylizeNoColor }, inspect);
    if (ret !== value) return ret;
  }

  seen.push(value);
  try {
    if (value instanceof Error) return value.stack ?? `${value.name}: ${value.message}`;
    if (value instanceof Date) return Number.isNaN(value.getTime()) ? 'Invalid Date' : value.toISOString();
    if (value instanceof RegExp) return value.toString();
    if (typeof value === 'function') {
      const name = value.name === '' ? ' (anonymous)' : `: ${value.name}`;
      if (value.toString().startsWith('class')) {
        const proto = Object.getPrototypeOf(value.prototype);
        const ext = proto && proto.constructor && proto.constructor.name !== 'Object' && proto.constructor.name !== ''
          ? ` extends ${proto.constructor.name}` : '';
        return `[class${name === ' (anonymous)' ? ' (anonymous)' : ` ${value.name}`}${ext}]`;
      }
      return `[${getFunctionPrefix(value)}${name}]`;
    }
    if (value instanceof Promise) return 'Promise { <pending> }'; // 偏差：无内态 API
    if (value instanceof Map) {
      const entries = [];
      let i = 0;
      for (const [k, v] of value) {
        if (ctx.maxArrayLength !== Infinity && i >= ctx.maxArrayLength) break;
        entries.push(`${inspect2(k, ctx, depth + 1, seen)} => ${inspect2(v, ctx, depth + 1, seen)}`);
        i++;
      }
      let extra = '';
      if (ctx.maxArrayLength !== Infinity && value.size > i) {
        extra = `... ${value.size - i} more item${value.size - i > 1 ? 's' : ''}`;
      }
      const body = entries.length === 0 && extra === '' ? '{}' : `{ ${entries.join(', ')}${extra ? `, ${extra}` : ''} }`;
      return `Map(${value.size}) ${body}`;
    }
    if (value instanceof Set) {
      const entries = [];
      let i = 0;
      for (const v of value) {
        if (ctx.maxArrayLength !== Infinity && i >= ctx.maxArrayLength) break;
        entries.push(inspect2(v, ctx, depth + 1, seen));
        i++;
      }
      let extra = '';
      if (ctx.maxArrayLength !== Infinity && value.size > i) {
        extra = `... ${value.size - i} more item${value.size - i > 1 ? 's' : ''}`;
      }
      const body = entries.length === 0 && extra === '' ? '{}' : `{ ${entries.join(', ')}${extra ? `, ${extra}` : ''} }`;
      return `Set(${value.size}) ${body}`;
    }
    if (value instanceof WeakMap || value instanceof WeakSet) return `${constructorName(value)} { <items unknown> }`;
    if (value instanceof ArrayBuffer || (typeof SharedArrayBuffer === 'function' && value instanceof SharedArrayBuffer)) {
      return `${constructorName(value)} { byteLength: ${value.byteLength} }`;
    }
    if (ArrayBuffer.isView(value)) {
      if (value instanceof DataView) {
        return `DataView { byteLength: ${value.byteLength}, byteOffset: ${value.byteOffset}, buffer: ArrayBuffer { byteLength: ${value.buffer.byteLength} } }`;
      }
      const entries = [];
      const limit = ctx.maxArrayLength === Infinity ? value.length : Math.min(value.length, ctx.maxArrayLength);
      for (let i = 0; i < limit; i++) entries.push(inspect2(value[i], ctx, depth + 1, seen));
      let extra = '';
      if (value.length > limit) {
        const more = value.length - limit;
        extra = `... ${more} more item${more > 1 ? 's' : ''}`;
      }
      const body = entries.length === 0 && extra === '' ? '[]' : `[ ${entries.join(', ')}${extra ? `, ${extra}` : ''} ]`;
      return `${constructorName(value)}(${value.length}) ${body}`;
    }
    if (Array.isArray(value)) {
      const name = constructorName(value);
      const p = name === 'Array' || name === null ? '' : `${name}(${value.length}) `;
      const entries = [];
      let holeCount = 0;
      const limit = ctx.maxArrayLength === Infinity ? value.length : Math.min(value.length, ctx.maxArrayLength);
      for (let i = 0; i < limit; i++) {
        if (!(i in value)) { holeCount++; continue; }
        if (holeCount > 0) {
          entries.push(holeCount === 1 ? '<1 empty item>' : `<${holeCount} empty items>`);
          holeCount = 0;
        }
        entries.push(inspect2(value[i], ctx, depth + 1, seen));
      }
      if (holeCount > 0 && limit === value.length) {
        entries.push(holeCount === 1 ? '<1 empty item>' : `<${holeCount} empty items>`);
      }
      let extra = '';
      if (value.length > limit) {
        const more = value.length - limit;
        extra = `... ${more} more item${more > 1 ? 's' : ''}`;
      }
      if (entries.length === 0 && extra === '' && p === '') return '[]';
      return `${p}[ ${entries.join(', ')}${extra ? `, ${extra}` : ''} ]`;
    }
    // 普通对象
    return formatPlainObject(value, ctx, depth, seen);
  } finally {
    seen.pop();
  }
}

function inspect(value, options) {
  const opts = typeof options === 'boolean' ? { ...inspectDefaultOptions, showHidden: options } :
    { ...inspectDefaultOptions, ...(options ?? {}) };
  return inspect2(value, opts, opts.depth, []);
}

export {
  identicalSequenceRange,
  inspect,
  inspectDefaultOptions,
  stylizeNoColor,
  format,
  formatWithOptions,
  getStringWidth,
  stripVTControlCharacters,
  isZeroWidthCodePoint,
};
export default { identicalSequenceRange, inspect, inspectDefaultOptions, stylizeNoColor, format, formatWithOptions, getStringWidth, stripVTControlCharacters, isZeroWidthCodePoint };
"#;
