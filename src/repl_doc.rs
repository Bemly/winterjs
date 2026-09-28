//! REPL `.doc` 整篇文档（irb `show_doc` 方向；CLI 本体面，`node:repl` 不动）。
//!
//! 语料：`mdn-content/files/en-us/**/*.md`（MDN Web Docs，© Mozilla contributors，
//! CC-BY-SA；见该目录 `ATTRIBUTION.md`），编译期经 `include_dir!` 打进二进制。
//! slug 三路：① WinterCG 显式表（路径不规则，如 `fetch` 住 `Window/fetch`）；
//! ② `console.X` 规则派生（`{x}_static`）；③ SM 内建规则派生
//! （`global_objects/{Head}/{method}`）。派生结果一律经语料存在性校验，
//! 缺页即未知条目（不猜、不编）。

/// 内嵌 MDN 语料（`files/en-us` 下，`index.md` 逐页）。
static MDN: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/mdn-content/files/en-us");

/// 内嵌命名空间语料（`scripts/gen-ns-docs.py` 由上游 `.d.ts` TSDoc 抽取，
/// `winterjs-content` 为手写 5 页；见各目录 `ATTRIBUTION.md`）。
static BUN: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/bun-content");
static DENO: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/deno-content");
static WJS: include_dir::Dir<'static> =
    include_dir::include_dir!("$CARGO_MANIFEST_DIR/winterjs-content");

/// WinterCG 显式 slug（MDN 路径不规则，逐条实证；`web/api/` 下，`index.md` 省略）。
fn explicit(topic: &str) -> Option<&'static str> {
    Some(match topic {
        "fetch" => "web/api/window/fetch",
        "URL" => "web/api/url",
        "URLSearchParams" => "web/api/urlsearchparams",
        "URLPattern" => "web/api/urlpattern",
        "TextEncoder" => "web/api/textencoder",
        "TextDecoder" => "web/api/textdecoder",
        "Headers" => "web/api/headers",
        "Request" => "web/api/request",
        "Response" => "web/api/response",
        "Blob" => "web/api/blob",
        "File" => "web/api/file",
        "ReadableStream" => "web/api/readablestream",
        "WritableStream" => "web/api/writablestream",
        "TransformStream" => "web/api/transformstream",
        "CompressionStream" => "web/api/compressionstream",
        "DecompressionStream" => "web/api/decompressionstream",
        "crypto" => "web/api/crypto",
        "AbortController" => "web/api/abortcontroller",
        "AbortSignal" => "web/api/abortsignal",
        "Event" => "web/api/event",
        "EventTarget" => "web/api/eventtarget",
        "CustomEvent" => "web/api/customevent",
        "MessageChannel" => "web/api/messagechannel",
        "WebSocket" => "web/api/websocket",
        "performance" => "web/api/performance",
        "localStorage" => "web/api/window/localstorage",
        "structuredClone" => "web/api/window/structuredclone",
        "queueMicrotask" => "web/api/window/queuemicrotask",
        "atob" => "web/api/window/atob",
        "btoa" => "web/api/window/btoa",
        "setTimeout" => "web/api/window/settimeout",
        "setInterval" => "web/api/window/setinterval",
        "clearTimeout" => "web/api/window/cleartimeout",
        "clearInterval" => "web/api/window/clearinterval",
        "console" => "web/api/console",
        "URLSearchParams.append" => "web/api/urlsearchparams/append",
        "URLSearchParams.get" => "web/api/urlsearchparams/get",
        "URLSearchParams.set" => "web/api/urlsearchparams/set",
        "URLSearchParams.delete" => "web/api/urlsearchparams/delete",
        "URLSearchParams.has" => "web/api/urlsearchparams/has",
        "URLSearchParams.getAll" => "web/api/urlsearchparams/getall",
        "URLSearchParams.sort" => "web/api/urlsearchparams/sort",
        "URLSearchParams.entries" => "web/api/urlsearchparams/entries",
        "Headers.get" => "web/api/headers/get",
        "Headers.set" => "web/api/headers/set",
        "Headers.append" => "web/api/headers/append",
        "Headers.delete" => "web/api/headers/delete",
        "Headers.has" => "web/api/headers/has",
        "Headers.entries" => "web/api/headers/entries",
        "TextEncoder.encode" => "web/api/textencoder/encode",
        "TextEncoder.encodeInto" => "web/api/textencoder/encodeinto",
        "TextDecoder.decode" => "web/api/textdecoder/decode",
        "AbortController.abort" => "web/api/abortcontroller/abort",
        "AbortSignal.timeout" => "web/api/abortsignal/timeout_static",
        "crypto.getRandomValues" => "web/api/crypto/getrandomvalues",
        "crypto.randomUUID" => "web/api/crypto/randomuuid",
        "performance.now" => "web/api/performance/now",
        "localStorage.getItem" => "web/api/storage/getitem",
        "localStorage.setItem" => "web/api/storage/setitem",
        "localStorage.removeItem" => "web/api/storage/removeitem",
        "localStorage.clear" => "web/api/storage/clear",
        "localStorage.key" => "web/api/storage/key",
        "ReadableStream.getReader" => "web/api/readablestream/getreader",
        "ReadableStream.cancel" => "web/api/readablestream/cancel",
        "ReadableStream.pipeTo" => "web/api/readablestream/pipeto",
        "Request.text" => "web/api/request/text",
        "Blob.text" => "web/api/blob/text",
        "Blob.slice" => "web/api/blob/slice",
        "EventTarget.addEventListener" => "web/api/eventtarget/addeventlistener",
        "EventTarget.removeEventListener" => "web/api/eventtarget/removeeventlistener",
        "EventTarget.dispatchEvent" => "web/api/eventtarget/dispatchevent",
        "URL.canParse" => "web/api/url/canparse_static",
        _ => return None,
    })
}

/// SM 内建头（规则派生只认这些；`Global_Objects/{Head}/{method}`）。
fn sm_head(head: &str) -> Option<&'static str> {
    Some(match head {
        "Object" | "object" => "Object",
        "Array" | "array" => "Array",
        "JSON" | "json" => "JSON",
        "Math" | "math" => "Math",
        "String" | "string" => "String",
        "Number" | "number" => "Number",
        "Boolean" | "boolean" => "Boolean",
        "BigInt" | "bigint" => "BigInt",
        "Symbol" | "symbol" => "Symbol",
        "Promise" | "promise" => "Promise",
        "Map" | "map" => "Map",
        "Set" | "set" => "Set",
        "WeakMap" => "WeakMap",
        "WeakSet" => "WeakSet",
        "Date" | "date" => "Date",
        "RegExp" | "regexp" => "RegExp",
        "Error" | "error" => "Error",
        "Proxy" | "proxy" => "Proxy",
        "Reflect" | "reflect" => "Reflect",
        "Function" | "function" => "Function",
        _ => return None,
    })
}

/// topic → 语料候选路径（`index.md` 后缀调用方补；存在性由 `lookup` 校验）。
fn slug(topic: &str) -> Option<String> {
    let t = topic.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(p) = explicit(t) {
        return Some(p.to_string());
    }
    if let Some(m) = t.strip_prefix("console.") {
        // `console.log` → `web/api/console/log_static`（方法页 `_static` 后缀）。
        let lower: String = m.chars().filter(|c| *c != '.').collect::<String>().to_lowercase();
        if lower.chars().all(|c| c.is_ascii_alphanumeric()) && !lower.is_empty() {
            return Some(format!("web/api/console/{lower}_static"));
        }
        return None;
    }
    if let Some((head, method)) = t.split_once('.') {
        // `Array.from` → `global_objects/array/from`（目录全小写是 MDN 惯例）。
        if let Some(h) = sm_head(head)
            && method.chars().all(|c| c.is_ascii_alphanumeric())
            && !method.is_empty()
        {
            return Some(format!(
                "web/javascript/reference/global_objects/{}/{}",
                h.to_lowercase(),
                method.to_lowercase()
            ));
        }
        return None;
    }
    // bare 全局（`encodeURI`/`eval`/`Proxy`/`parseInt`…）→ `global_objects/{lower}`。
    // 无需 allowlist：`lookup` 经语料存在性校验，缺页即 `None`（用户变量同此）。
    // 字符集限字母数字/`_`/`$`（无 `/`/`.`，无路径穿越）。
    if !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
        return Some(format!(
            "web/javascript/reference/global_objects/{}",
            t.to_lowercase()
        ));
    }
    None
}

/// 命名空间语料路由（`Bun`/`Deno`/`WinterJS` 头；`lookup` 用）。
/// `Bun.serve` → `bun-content/serve/index.md`，bare `Bun` → `index` 页；
/// 方法段限字母数字/`_`/`$`（`Bun.$` 的 `$` 在内；无 `..`，无路径穿越）。
/// 存在性由 `lookup` 校验（缺页即未知条目，不猜）。
fn ns_lookup(topic: &str) -> Option<&'static str> {
    // `global` 是 `globalThis` 的 Node 口径别名（bootstrap），补全 `global.Deno.x`
    // 形主题与 `Deno.x` 同页。
    let t = topic.trim();
    let t = t.strip_prefix("globalThis.").unwrap_or(t);
    let t = t.strip_prefix("global.").unwrap_or(t);
    let (head, method) = match t.split_once('.') {
        Some((h, m)) => (h, m),
        None => (t, "index"),
    };
    let dir: &include_dir::Dir<'static> = match head {
        "Bun" | "bun" => &BUN,
        "Deno" | "deno" => &DENO,
        "WinterJS" | "winterjs" => &WJS,
        _ => return None,
    };
    if method.is_empty()
        || !method
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
    {
        return None;
    }
    let path = format!("{}/index.md", method.to_lowercase());
    dir.get_file(&path)
        .and_then(|f| f.contents_utf8())
}

/// 取原文（存在性校验：缺页即 `None`）。
/// `slug` 主规则未命中时按形状试探（首中即返；全部只读静态语料）：
/// - `A.b` → `global_objects/a/b`（sm_head 白名单外的头）、`web/api/a/b`、
///   `web/api/a/b_static`（Web 静态方法惯例）；
/// - bare → `reference/statements/t`、`reference/operators/t`、`web/api/t`。
/// 段字符集限小写字母数字/`_`/`$`（无 `..`，无路径穿越）。
/// 命名空间主题（`Bun`/`Deno`/`WinterJS` 头）走 `ns_lookup`（MDN 主规则之
/// 后、fallback 试探之前；三语料互不串味）。
pub fn lookup(topic: &str) -> Option<&'static str> {
    let t = topic.trim();
    if let Some(path) = slug(t).map(|s| format!("{s}/index.md"))
        && let Some(f) = MDN.get_file(&path)
        && let Some(s) = f.contents_utf8()
    {
        return Some(s);
    }
    if let Some(s) = ns_lookup(t) {
        return Some(s);
    }
    for cand in fallback_paths(t) {
        if let Some(f) = MDN.get_file(&cand)
            && let Some(s) = f.contents_utf8()
        {
            return Some(s);
        }
    }
    None
}

/// 试探候选路径（`lookup` 用；调用方只取首中）。
fn fallback_paths(topic: &str) -> Vec<String> {
    let mut out = Vec::new();
    let t = topic
        .trim()
        .strip_prefix("globalThis.")
        .unwrap_or(topic.trim());
    if t.is_empty() {
        return out;
    }
    let ok = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$')
    };
    if let Some((a, b)) = t.split_once('.') {
        let (a, b) = (a.to_lowercase(), b.to_lowercase());
        if ok(&a) && ok(&b) {
            out.push(format!("web/javascript/reference/global_objects/{a}/{b}/index.md"));
            out.push(format!("web/api/{a}/{b}/index.md"));
            out.push(format!("web/api/{a}/{b}_static/index.md"));
        }
        return out;
    }
    let l = t.to_lowercase();
    if ok(&l) {
        out.push(format!("web/javascript/reference/statements/{l}/index.md"));
        out.push(format!("web/javascript/reference/operators/{l}/index.md"));
        out.push(format!("web/api/{l}/index.md"));
    }
    out
}

/// `__wjs_doc_summary(topic)`（补全桥用；缺页回 `undefined`，桥保留签名原文）。
///
/// SAFETY: 由引擎以有效调用帧调用（JSNative 约定）。
pub unsafe extern "C" fn doc_summary(
    cx_raw: *mut mozjs::jsapi::JSContext,
    argc: u32,
    vp: *mut mozjs::jsval::JSVal,
) -> bool {
    use mozjs::conversions::ToJSValConvertible as _;
    use mozjs::jsval::UndefinedValue;
    use mozjs::rooted;
    unsafe {
        let mut cx = crate::jsapi_glue::wrap_cx(cx_raw);
        let frame = crate::jsapi_glue::Frame::from_raw(vp, argc);
        let topic = if argc > 0 {
            crate::jsapi_glue::value_to_string(&mut cx, frame.arg(0))
        } else {
            String::new()
        };
        match summary(&topic) {
            Some(s) => {
                rooted!(&in(cx) let mut v = UndefinedValue());
                s.to_jsval(&mut cx, v.handle_mut());
                frame.set_rval(v.get());
            }
            None => frame.set_rval(UndefinedValue()),
        }
        true
    }
}

/// 去噪（纯函数）：frontmatter、`{{宏}}` 行与行内宏、图片行、`-nolint` 围栏后缀。
pub fn sanitize(md: &str) -> String {
    let mut lines = md.lines().peekable();
    // frontmatter：首行 `---` 起到配对 `---` 止。
    if lines.peek() == Some(&"---") {
        lines.next();
        for l in lines.by_ref() {
            if l.trim() == "---" {
                break;
            }
        }
    }
    let mut out = Vec::new();
    for l in lines {
        let t = l.trim();
        if t.is_empty() {
            out.push(String::new());
            continue;
        }
        // 整行宏（`{{Compat}}`/`{{APIRef}}` 等）与图片行直接丢。
        if t.starts_with("{{") && t.ends_with("}}") {
            continue;
        }
        if t.starts_with('!') && t.contains("](") {
            continue;
        }
        let mut s = l.to_string();
        // 行内宏：domxref/glossary 取显示文字（`{{domxref("p","d")}}`→`d`，
        // 单参取末段；大小写兼容 `DOMxRef`/`Glossary`；单双引号兼容），
        // 其余宏（APIRef/Compat/Specifications 等）删——摘要在他处。
        strip_inline_macros(&mut s);
        // `[text](url)` → `text`。
        loop {
            let (Some(a), Some(b)) = (s.find('['), s.find("](")) else {
                break;
            };
            if a > b {
                break;
            }
            if let Some(c) = s[b..].find(')') {
                let text = s[a + 1..b].to_string();
                s.replace_range(a..b + c + 1, &text);
            } else {
                break;
            }
        }
        out.push(s);
    }
    // 围栏 ` ```js-nolint ` → ` ```js `（语言检测用）。
    out.join("\n").replace("-nolint", "")
}

/// TTY 渲染（termimad 样式排版；调用方按 TTY 与否二选一）。
pub fn render_tty(md: &str) -> String {
    let skin = termimad::MadSkin::default();
    skin.term_text(&sanitize(md)).to_string()
}

/// 管道/测试渲染（确定性纯文本：不换行重排，只做标记剥离）。
pub fn render_plain(md: &str) -> String {
    let mut out = Vec::new();
    for l in sanitize(md).lines() {
        let mut s = l.trim().to_string();
        // 标题 `#`、引用 `>`、列表 `-/ *` 前缀去标记留文本。
        while let Some(rest) = s.strip_prefix('#') {
            s = rest.trim_start().to_string();
        }
        if let Some(rest) = s.strip_prefix('>') {
            s = rest.trim_start().to_string();
        }
        for mark in ["- ", "* ", "+ "] {
            if let Some(rest) = s.strip_prefix(mark) {
                s = rest.to_string();
                break;
            }
        }
        // 行内 `code`/加粗/斜体只去标记。
        s = s.replace("**", "").replace("__", "");
        while let Some(a) = s.find('`') {
            s.remove(a);
        }
        out.push(s);
    }
    out.join("\n")
}

/// 行内宏剥离（`sanitize` 用）：`domxref`/`glossary`/`jsxref` 取显示文字，其余删。
/// 大小写兼容（`DOMxRef`/`Glossary`），单双引号兼容；显示文字缺席时
/// domxref/jsxref 取路径末段、glossary 取术语本身。
fn strip_inline_macros(s: &mut String) {
    loop {
        let Some(a) = s.find("{{") else {
            break;
        };
        let Some(rel) = s[a..].find("}}") else {
            break;
        };
        let b = a + rel;
        let inner = s[a + 2..b].to_string();
        let (name, args) = match inner.find('(') {
            Some(i) => (inner[..i].trim().to_lowercase(), inner[i..].to_string()),
            None => (inner.trim().to_lowercase(), String::new()),
        };
        let replacement = if name == "domxref" || name == "glossary" || name == "jsxref" {
            let quoted: Vec<String> = {
                let mut q = Vec::new();
                let mut cur = String::new();
                let mut quote = None;
                for ch in args.chars() {
                    match quote {
                        None if ch == '"' || ch == '\'' => quote = Some(ch),
                        Some(qc) if ch == qc => {
                            quote = None;
                            q.push(std::mem::take(&mut cur));
                        }
                        _ if quote.is_some() => cur.push(ch),
                        _ => {}
                    }
                }
                q
            };
            match quoted.as_slice() {
                [_, second, ..] if !second.is_empty() => second.clone(),
                [first, ..] if name == "domxref" => first.rsplit('/').next().unwrap_or("").to_string(),
                [first, ..] => first.clone(),
                [] => String::new(),
            }
        } else {
            String::new()
        };
        // `{{glossary("URL", "URLs")}}` 显示文字常与前词连写（`encode URLs`），
        // 仅当替换为空且前后皆非空才补空格——此处调用方上下文未知，保持原样拼接。
        s.replace_range(a..b + 2, &replacement);
    }
}

/// 去 `_斜体_` 标记（内含空格才算强调；`snake_case`/`__wjs_x` 保留）。
/// 浮窗纯文本用（`.doc` 整篇走 termimad 原生斜体，不动）。
fn strip_italics(s: &str) -> String {
    let ch: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < ch.len() {
        if ch[i] == '_' && ch.get(i + 1) != Some(&'_') {
            let mut j = i + 1;
            while j < ch.len() && ch[j] != '_' {
                j += 1;
            }
            if j < ch.len() && ch.get(j + 1) != Some(&'_') {
                let inner: String = ch[i + 1..j].iter().collect();
                if inner.contains(' ') {
                    out.push_str(&inner);
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(ch[i]);
        i += 1;
    }
    out
}

/// 摘要缓存（语料静态，`summary` 纯函数；首轮 Tab 后复用——空行 148 项
/// 逐页重洗实测 ~450ms，超补全超时窗。确定性内容，单测不断言空满，无需复位）。
static SUMMARY_CACHE: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

/// 补全浮窗摘要（实时读语料，无拷贝）：topic → 前两段 + 首个代码块
/// （调用形状）+ Parameters 节（机械提取，无编撰；reedline 描述盒按空白
/// 重排，`\n` 只做语义分隔，渲染恒成一段）。
/// 缺页即 `None`。
pub fn summary(topic: &str) -> Option<String> {
    let key = topic
        .strip_prefix("globalThis.")
        .unwrap_or(topic)
        .to_string();
    if let Some(hit) = SUMMARY_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned()
    {
        return hit;
    }
    let v = summary_inner(&key);
    SUMMARY_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, v.clone());
    v
}

/// `summary` 本体（缓存穿透时计算；纯函数）。
fn summary_inner(t: &str) -> Option<String> {
    let md = lookup(t)?;
    let clean_src = sanitize(md);
    // 正文段（MDN 源码折行，段内空格连接；标题/引用/围栏行不计；取前两段）。
    let mut paras: Vec<String> = Vec::new();
    let mut cur = String::new();
    for l in clean_src.lines().map(str::trim) {
        if l.starts_with('#') || l.starts_with("```") {
            break;
        }
        if l.is_empty() || l.starts_with('>') {
            if !cur.is_empty() {
                paras.push(std::mem::take(&mut cur));
                if paras.len() >= 2 {
                    break;
                }
            }
            continue;
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(l);
    }
    if !cur.is_empty() && paras.len() < 2 {
        paras.push(cur);
    }
    if paras.is_empty() {
        return None;
    }
    // 首个围栏代码块（调用形状；至多 10 行，fence 行不要）。
    let mut code: Vec<&str> = Vec::new();
    let mut in_fence = false;
    for l in clean_src.lines().map(str::trim) {
        if l.starts_with("```") {
            if in_fence {
                break;
            }
            in_fence = true;
            continue;
        }
        if in_fence {
            if !l.is_empty() {
                code.push(l);
            }
            if code.len() >= 10 {
                break;
            }
        }
    }
    // Parameters 节（`### Parameters` 起到下个标题止；`- \`name\`` → `name:`，
    // `  - : desc` 续接；先收条目再拼，截断不断在名后，上限约 600 字）。
    let mut items: Vec<(String, String)> = Vec::new();
    let mut cur_name: Option<String> = None;
    let mut cur_desc = String::new();
    let mut in_params = false;
    for l in clean_src.lines().map(str::trim) {
        if l.starts_with('#') {
            if in_params {
                break;
            }
            if l.trim_start_matches('#').trim().eq_ignore_ascii_case("Parameters") {
                in_params = true;
            }
            continue;
        }
        if !in_params || l.is_empty() || l.starts_with('>') {
            continue;
        }
        let mut s = l.to_string();
        while let Some(rest) = s.strip_prefix("- ") {
            s = rest.to_string();
        }
        s = s.strip_prefix(':').map_or(s.clone(), |r| r.trim_start().to_string());
        if s.is_empty() {
            continue;
        }
        if s.starts_with('`') {
            if let Some(n) = cur_name.take() {
                items.push((n, std::mem::take(&mut cur_desc)));
            }
            cur_name = Some(s.replace('`', ""));
        } else {
            if !cur_desc.is_empty() {
                cur_desc.push(' ');
            }
            cur_desc.push_str(&s);
        }
    }
    if let Some(n) = cur_name.take() {
        items.push((n, cur_desc));
    }
    let mut params = String::new();
    for (n, d) in &items {
        let piece = if d.is_empty() {
            format!("{n} ")
        } else {
            format!("{n}: {d} ")
        };
        if params.len() + piece.len() > 600 {
            break;
        }
        params.push_str(&piece);
    }
    let mut parts = paras;
    if !code.is_empty() {
        parts.push(code.join("\n"));
    }
    if !params.trim().is_empty() {
        parts.push(params.trim().to_string());
    }
    Some(strip_italics(&parts.join("\n").replace("**", "").replace('`', "")))
}

/// 未知条目提示（精确键空间见 `explicit` + 两条派生规则 + `ns_lookup` 三头）。
pub fn unknown_hint(topic: &str) -> String {
    format!(
        "no documentation for '{topic}' (try: console.log, fetch, URL, Array.from, TextEncoder.encode, Deno.readFile, Bun.serve)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_frontmatter_and_macros() {
        let md = "---\ntitle: x\nslug: y\n---\n\n{{APIRef(\"Console API\")}}\n\nReal text with {{domxref(\"a\")}} inside.\n\n![alt](img.png)\n\n```js-nolint\ncode()\n```\n";
        let s = sanitize(md);
        assert!(!s.contains("title:"), "{s}");
        assert!(!s.contains("APIRef"), "{s}");
        assert!(!s.contains("domxref"), "{s}");
        assert!(!s.contains("img.png"), "{s}");
        assert!(!s.contains("-nolint"), "{s}");
        assert!(s.contains("Real text with a inside."), "{s}");
        assert!(s.contains("code()"), "{s}");
    }

    #[test]
    fn slug_rules() {
        assert_eq!(
            slug("console.log").as_deref(),
            Some("web/api/console/log_static")
        );
        assert_eq!(
            slug("Array.from").as_deref(),
            Some("web/javascript/reference/global_objects/array/from")
        );
        assert_eq!(slug("fetch").as_deref(), Some("web/api/window/fetch"));
        assert_eq!(
            slug("encodeURI").as_deref(),
            Some("web/javascript/reference/global_objects/encodeuri")
        );
        assert_eq!(
            slug("Proxy").as_deref(),
            Some("web/javascript/reference/global_objects/proxy")
        );
        assert!(slug("").is_none());
        assert!(slug("o.assign").is_none());
        // node 私货无 MDN 页：slug 有形状，lookup 必 None（存在性是唯一真相）。
        assert!(summary("setImmediate").is_none());
        // 用户变量无页（存在性由 lookup 校验）；路径穿越字符拒收。
        assert!(slug("myVar").is_some()); // 有 slug 形状，但 lookup 必 None（下测）。
        assert!(slug("../secret").is_none());
        assert!(slug("a/b").is_none());
    }

    #[test]
    fn summary_reads_corpus_live() {
        // 浮窗摘要 = 前两段 + 调用形状 + Parameters（实时读，无拷贝）；缺页 None。
        let s = summary("console.log").expect("console.log documented");
        assert!(s.starts_with("The console.log()"), "{s}");
        assert!(s.contains("console.log(val1)"), "{s}");
        assert!(s.contains("val1 … valN:"), "{s}");
        let t = summary("console.timeEnd").expect("timeEnd documented");
        assert!(t.contains("See Timers"), "{t}");
        assert!(t.contains("console.timeEnd(label)"), "{t}");
        assert!(t.contains("label:"), "{t}");
        let e = summary("Event").expect("Event documented");
        assert!(e.starts_with("The Event interface represents an event"), "{e}");
        let f = summary("fetch").expect("fetch documented");
        assert!(f.contains("fulfilled once the response is available"), "{f}");
        // bare 全局（`encodeURI` 一族先前无 slug，直通 `None`）。
        let u = summary("encodeURI").expect("encodeURI documented");
        assert!(u.contains("function encodes a URI by replacing"), "{u}");
        let v = summary("eval").expect("eval documented");
        assert!(v.contains("evaluates JavaScript code"), "{v}");
        assert!(summary("myVar").is_none());
        assert!(summary("../secret").is_none());
        // 试探路（`lookup` fallback）：白名单外的头、Web 静态方法、语句/操作符。
        let a = summary("Atomics.add").expect("Atomics.add documented");
        assert!(a.contains("adds a given value at a given position"), "{a}");
        let p = summary("URL.parse").expect("URL.parse documented");
        assert!(p.contains("returns a newly created"), "{p}");
        let fr = summary("for").expect("for documented");
        assert!(fr.contains("creates a loop"), "{fr}");
        let ty = summary("typeof").expect("typeof documented");
        assert!(ty.contains("indicating the type"), "{ty}");
        assert!(summary("document.querySelector").is_none());
        assert!(summary("foo.bar").is_none());
        assert!(summary("o.assign").is_none());
        assert!(summary("globalThis.Object.assign").is_some());
    }

    #[test]
    fn strip_italics_keeps_identifiers() {
        assert_eq!(strip_italics("a _source object_ here"), "a source object here");
        assert_eq!(strip_italics("snake_case kept"), "snake_case kept");
        assert_eq!(strip_italics("__wjs_x kept"), "__wjs_x kept");
        assert_eq!(strip_italics("a_b kept"), "a_b kept");
    }

    #[test]
    fn ns_lookup_hits_generated_corpus() {
        // 三命名空间：存在性即真相（缺页 None，不猜）；形状约束（空方法、
        // 路径穿越、大小写头宽容）。
        let d = summary("Deno.readFile").expect("Deno.readFile documented");
        assert!(d.contains("entire contents of a file"), "{d}");
        assert!(d.contains("function readFile("), "{d}");
        let b = summary("Bun.serve").expect("Bun.serve documented");
        assert!(b.contains("high-performance HTTP server"), "{b}");
        let w = summary("WinterJS.version").expect("WinterJS.version documented");
        assert!(w.contains("winterjs version"), "{w}");
        assert!(summary("Bun").is_some());
        assert!(summary("Deno").is_some());
        assert!(summary("WinterJS").is_some());
        assert!(summary("globalThis.Deno.args").is_some());
        assert!(summary("deno.readfile").is_some());
        assert!(summary("Bun.TOML").is_none());
        assert!(summary("Deno.statFs").is_none());
        assert!(summary("Bun.cwd").is_none());
        assert!(summary("Deno.").is_none());
        assert!(summary("Bun../secret").is_none());
        assert!(summary("Deno.readFile.toString").is_none());
    }

    #[test]
    fn lookup_hits_vendored_corpus() {
        // 显式表全枚举（加 slug 必须在此加主题；APFS 大小写不敏感，缺页只能靠
        // 此测试在 Linux CI 形态下暴露，本地以 git ls-tree 为准复核过）。
        for t in [
            "fetch", "URL", "URLSearchParams", "URLPattern", "TextEncoder", "TextDecoder",
            "Headers", "Request", "Response", "Blob", "File", "ReadableStream", "WritableStream",
            "TransformStream", "CompressionStream", "DecompressionStream", "crypto",
            "AbortController", "AbortSignal", "Event", "EventTarget", "CustomEvent",
            "MessageChannel", "WebSocket", "performance", "localStorage", "structuredClone",
            "queueMicrotask", "atob", "btoa", "setTimeout", "setInterval", "clearTimeout",
            "clearInterval", "console", "URLSearchParams.append", "URLSearchParams.get",
            "URLSearchParams.set", "URLSearchParams.delete", "URLSearchParams.has",
            "URLSearchParams.getAll", "URLSearchParams.sort", "URLSearchParams.entries",
            "Headers.get", "Headers.set", "Headers.append", "Headers.delete", "Headers.has",
            "Headers.entries", "TextEncoder.encode", "TextEncoder.encodeInto",
            "TextDecoder.decode", "AbortController.abort", "AbortSignal.timeout",
            "crypto.getRandomValues", "crypto.randomUUID", "performance.now",
            "localStorage.getItem", "localStorage.setItem", "localStorage.removeItem",
            "localStorage.clear", "localStorage.key", "ReadableStream.getReader",
            "ReadableStream.cancel", "ReadableStream.pipeTo", "Request.text", "Blob.text",
            "Blob.slice", "EventTarget.addEventListener", "EventTarget.removeEventListener",
            "EventTarget.dispatchEvent", "URL.canParse",
        ] {
            assert!(lookup(t).is_some(), "missing corpus page for {t}");
        }
        assert!(lookup("o.assign").is_none());
    }
}
