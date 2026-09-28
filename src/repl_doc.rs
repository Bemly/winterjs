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
    }
    None
}

/// 取原文（存在性校验：缺页即 `None`，调用方报未知条目）。
pub fn lookup(topic: &str) -> Option<&'static str> {
    let path = format!("{}/index.md", slug(topic.trim())?);
    MDN.get_file(&path)?.contents_utf8()
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

/// 未知条目提示（精确键空间见 `explicit` + 两条派生规则）。
pub fn unknown_hint(topic: &str) -> String {
    format!(
        "no documentation for '{topic}' (try: console.log, fetch, URL, Array.from, TextEncoder.encode)"
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
        assert!(slug("").is_none());
        assert!(slug("o.assign").is_none());
        assert!(slug("setImmediate").is_none());
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
