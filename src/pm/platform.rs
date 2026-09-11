//! 平台匹配（npm `os`/`cpu` 语义；`optionalDependencies` 的平台二进制过滤用）。
//!
//! - 命名按 npm（`std::env::consts` 转译）：OS `macos→darwin`/`linux→linux`/
//!   `windows→win32`；ARCH `aarch64→arm64`/`x86_64→x64`/`x86→ia32`/`arm→arm`/
//!   `riscv64→riscv64`；未知原样透传（新平台不误杀，匹配失败即跳过，由调用方定）。
//! - 语义：字段缺省表全平台；`!` 前缀表排除；同名字段内任一命中即过，
//!   `os` 与 `cpu` 两字段都过才装（npm 口径）。
//! - 纯函数，单测覆盖判定表。

/// 当前 OS 的 npm 名。
pub fn npm_os() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// 当前 ARCH 的 npm 名。
pub fn npm_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    }
}

/// 单字段匹配（`None`/空表全过；`!x` 排除优先于命中）。
fn field_matches(list: Option<&[String]>, current: &str) -> bool {
    let Some(list) = list else {
        return true;
    };
    if list.is_empty() {
        return true;
    }
    // 排除优先（npm 口径：命中排除即不过）。
    if list.iter().any(|e| e.strip_prefix('!').is_some_and(|x| x == current)) {
        return false;
    }
    // 有肯定项则至少命中其一；全是否定项（且未排除）即过。
    let positives: Vec<&String> = list.iter().filter(|e| !e.starts_with('!')).collect();
    if positives.is_empty() {
        return true;
    }
    positives.iter().any(|e| e.as_str() == current)
}

/// 包是否装到当前平台（`os`/`cpu` 双过；任一不过即跳过，不报错）。
pub fn platform_matches(os: Option<&[String]>, cpu: Option<&[String]>) -> bool {
    field_matches(os, npm_os()) && field_matches(cpu, npm_arch())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_platform_passes_empty() {
        // 缺省/空表：全平台过。
        assert!(platform_matches(None, None));
        assert!(platform_matches(Some(&[]), Some(&[])));
    }

    #[test]
    fn exact_current_matches() {
        let os = [npm_os().to_string()];
        let cpu = [npm_arch().to_string()];
        assert!(platform_matches(Some(&os), Some(&cpu)));
        // 任一错即不过。
        assert!(!platform_matches(Some(&["nonexistent-os".to_string()]), Some(&cpu)));
        assert!(!platform_matches(Some(&os), Some(&["nonexistent-cpu".to_string()])));
    }

    #[test]
    fn negation_semantics() {
        // 排除当前即不过。
        assert!(!platform_matches(Some(&[format!("!{}", npm_os())]), None));
        // 排除别人不影响。
        assert!(platform_matches(Some(&["!nonexistent-os".to_string()]), None));
        // 肯定命中 + 无关排除 → 过。
        assert!(platform_matches(
            Some(&[npm_os().to_string(), "!nonexistent-os".to_string()]),
            None,
        ));
        // 肯定未命中 + 排除当前 → 不过（排除优先）。
        assert!(!platform_matches(
            Some(&["nonexistent-os".to_string(), format!("!{}", npm_os())]),
            None,
        ));
    }

    #[test]
    fn arch_names_pinned() {
        // 转译表钉死（oxlint binding 名与此对齐：darwin-arm64 等）。
        assert_eq!(npm_os(), match std::env::consts::OS {
            "macos" => "darwin",
            "windows" => "win32",
            other => other,
        });
        assert_eq!(npm_arch(), match std::env::consts::ARCH {
            "aarch64" => "arm64",
            "x86_64" => "x64",
            "x86" => "ia32",
            other => other,
        });
    }
}
