//! vm 错误包络 helpers（对齐 vm.rs；纯搬移，调用点零改）。

use crate::jsapi_glue::report_error;

/// natives 抛错包络：`__wjs2_vm_error:{name}\n{message}`（`%` 由 report 转义，
/// 换行分隔——message 内换行只影响尾部显示，JS 侧按首行取 name）。
/// JS 侧 `__vmCall` 剥包络重建同名 Error（§4.31 教训：不断言原文以外的形态）。
pub(crate) fn throw_vm(cx: &mut mozjs::context::JSContext, name: &str, message: &str) {
    let clean = message.replace('\0', "");
    report_error(cx, &format!("__wjs2_vm_error:{name}\n{clean}"));
}

/// vm 求值路径的赋值类 TypeError 文案桥：SM 引擎文案 → node contextify 拦截器
/// 口径（真机该文案出自 node 的 global 属性拦截器，非引擎——套件按 node 文案
/// regex 断言；仅 vm_run 用，主域/主 global 求值不改写引擎文案）。
fn bridge_vm_assign_message(message: &str) -> String {
    // SM "assignment to undeclared variable z" → node "z is not defined"
    //（strict 隐式全局赋值的 ReferenceError，真机文案）。
    if let Some(key) = message.strip_prefix("assignment to undeclared variable ") {
        return format!("{key} is not defined");
    }
    let Some(rest) = message.strip_prefix('"') else {
        return message.to_string();
    };
    let Some((key, tail)) = rest.split_once("\" is ") else {
        return message.to_string();
    };
    match tail {
        "read-only" => {
            format!("Cannot assign to read only property '{key}' of object '[object Object]'")
        }
        "non-configurable and can't be redefined" => format!("Cannot redefine property: {key}"),
        _ => message.to_string(),
    }
}

/// vm 运行期错误的信封消息：文案桥 + 位置标记（`__wjs2_vm_stk:{json}`）。
/// node displayErrors 口径——vm 错误的 err.stack 以 `filename:line` 前缀开头
/// （checkErr 类 `startsWith(filename)` 校验点名）；JS 侧 __vmUnwrap 剥标记
/// 重建栈。栈内帧格式仍是引擎口径（SM `@` vs V8 `at`，记档偏离）。
pub(crate) fn vm_stk_envelope(filename: &str, code: &str, info: &mozjs::rust::ErrorInfo) -> String {
    let line = info.line.max(1);
    let col = info.col.max(1);
    let srcline = code
        .split('\n')
        .nth((line as usize).saturating_sub(1))
        .unwrap_or("");
    let stk = serde_json::json!({ "f": filename, "l": line, "c": col, "s": srcline }).to_string();
    format!("{}\n__wjs2_vm_stk:{stk}", bridge_vm_assign_message(&info.message))
}
