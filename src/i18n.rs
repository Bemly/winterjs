//! CLI 双语 locale 解析：`-l/--lang` > `WINTERJS_LANG` > 系统语言 > `en`。
//!
//! 解析全是纯函数（argv/env/系统语言三源注入），可单测；唯一的副作用
//! （`rust_i18n::set_locale`）只发生在 `init_from_argv`，由 `main` 在解析
//! CLI 之前调一次。翻译文本查表（`t!`）在 `cli::localized_command`。

/// `locales/*.yml` 里实际有的 locale（文件名即名）。
pub const SUPPORTED: &[&str] = &["en", "zh"];
/// 缺省/未知一律回英文（英文 yml 与 `cli.rs` 原文逐字节一致，默认输出不变）。
pub const FALLBACK: &str = "en";
/// 环境变量覆盖（优先级低于显式 flag，高于系统语言）。
pub const ENV_VAR: &str = "WINTERJS_LANG";

/// `zh-CN`/`zh_HK` → `zh`，`en-US` → `en`，其余未知 → `en`（只做中英双语）。
pub fn normalize(raw: &str) -> &'static str {
    let lower = raw.trim().to_ascii_lowercase().replace('_', "-");
    let hit = match lower.split('-').next().unwrap_or("") {
        "zh" => "zh",
        "en" => "en",
        _ => FALLBACK,
    };
    // 返回永远落在支持集内：加 locale 时同步改 SUPPORTED + 加 yml 文件
    debug_assert!(SUPPORTED.contains(&hit));
    hit
}

/// 预扫 argv 找显式语言：`--lang=zh` / `--lang zh` / `-l zh` / `-lzh`。
/// 遇 `--` 停扫（后面是脚本参数，如 `run -- foo -l zh` 的 `-l` 不能误认）；
/// 扫不到回 `None`（交后续优先级）。值本身不校验（`zh-CN` 等归一化，
/// 非法值由 clap 的 `PossibleValuesParser` 报错）。
pub fn prescan(argv: &[String]) -> Option<String> {
    let mut it = argv.iter().skip(1);
    while let Some(a) = it.next() {
        if a == "--" {
            break;
        }
        if let Some(v) = a.strip_prefix("--lang=") {
            return Some(v.to_string());
        }
        if a == "--lang" || a == "-l" {
            return it.next().cloned();
        }
        // `-lzh` 连写（`-l` 裸形已在上分支处理；`--` 开头的已排除误伤，
        // `-vl` 这类组合短 flag 扫不到是已知缺口，只影响 help 渲染语言，不影响解析）
        if let Some(v) = a.strip_prefix("-l")
            && !a.starts_with("--")
        {
            return Some(v.to_string());
        }
    }
    None
}

/// 三源归一（纯函数，单测入口）。
pub fn resolve(argv: &[String], env: Option<&str>, system: Option<&str>) -> &'static str {
    if let Some(v) = prescan(argv) {
        return normalize(&v);
    }
    if let Some(v) = env
        && !v.trim().is_empty()
    {
        return normalize(v);
    }
    if let Some(v) = system {
        return normalize(v);
    }
    FALLBACK
}

/// 进程启动时调一次：定 locale 并装进全局（`t!` 后续查表用）。
/// tracing 此时还没初始化，只做事不打日志。
pub fn init_from_argv() -> &'static str {
    let argv: Vec<String> = std::env::args().collect();
    let env = std::env::var(ENV_VAR).ok();
    let system = sys_locale::get_locale();
    let locale = resolve(&argv, env.as_deref(), system.as_deref());
    rust_i18n::set_locale(locale);
    locale
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn normalize_table() {
        assert_eq!(normalize("zh"), "zh");
        assert_eq!(normalize("zh-CN"), "zh");
        assert_eq!(normalize("zh_HK"), "zh");
        assert_eq!(normalize("ZH-tw"), "zh");
        assert_eq!(normalize("en"), "en");
        assert_eq!(normalize("en-US"), "en");
        assert_eq!(normalize("fr"), "en");
        assert_eq!(normalize(""), "en");
        assert_eq!(normalize("  "), "en");
    }

    #[test]
    fn prescan_forms() {
        assert_eq!(prescan(&argv(&["w", "--lang=zh"])).as_deref(), Some("zh"));
        assert_eq!(prescan(&argv(&["w", "--lang", "zh"])).as_deref(), Some("zh"));
        assert_eq!(prescan(&argv(&["w", "-l", "zh"])).as_deref(), Some("zh"));
        assert_eq!(prescan(&argv(&["w", "-lzh"])).as_deref(), Some("zh"));
        // 位置无关（子命令之后也认）
        assert_eq!(prescan(&argv(&["w", "run", "-l", "en", "a.js"])).as_deref(), Some("en"));
        // `--` 之后的不认
        assert_eq!(prescan(&argv(&["w", "run", "--", "-l", "zh"])), None);
        assert_eq!(prescan(&argv(&["w", "eval", "1"])), None);
        // `--lang` 落单（无值）交 clap 报错，这里只回 None
        assert_eq!(prescan(&argv(&["w", "--lang"])), None);
    }

    #[test]
    fn resolve_precedence() {
        // flag > env > 系统 > 回退
        assert_eq!(resolve(&argv(&["w", "-l", "en"]), Some("zh"), Some("zh-CN")), "en");
        assert_eq!(resolve(&argv(&["w"]), Some("zh"), Some("en-US")), "zh");
        assert_eq!(resolve(&argv(&["w"]), Some(""), Some("zh-TW")), "zh");
        assert_eq!(resolve(&argv(&["w"]), None, Some("zh-CN")), "zh");
        assert_eq!(resolve(&argv(&["w"]), None, Some("fr-FR")), "en");
        assert_eq!(resolve(&argv(&["w"]), None, None), "en");
        // 非法 flag 值归一化到回退（clap 随后会报 invalid value）
        assert_eq!(resolve(&argv(&["w", "-l", "fr"]), None, None), "en");
    }

    #[test]
    fn supported_matches_locales() {
        // 新增 locale 必须同步加 yml 文件；此断言钉住双语集合
        assert_eq!(SUPPORTED, &["en", "zh"]);
    }
}
