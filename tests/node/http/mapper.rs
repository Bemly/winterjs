//! tests/node/http/mapper.rs — §4.202-① 对拍 mapper 三件（纯搬移自 parity.rs，
//! §0.9 单文件 ≤1000 行；调用面 crate::helpers::* 不变）。

use crate::helpers::*;
use assert_fs::prelude::*;

/// 正常件：async 回调内断言失败，mapper 定位到套件侧真实调用点
/// （无壳时宿主上报 assert SOURCE 包装位置，调用点不可见）。
/// 栈帧行号系 CJS 包装后行号（恒比物理行 +1，§4.202-① 实测三处一致）——
/// 帧号断言按包装行；±2 行节选窗口吸收位移，物理 assert 行必在节选内。
#[test]
fn phase_mapper_locates_async_callsite() {
    let dir = assert_fs::TempDir::new().unwrap();
    // 物理行钉住：r#" 首换行使 1 行为空，assert 在物理第 6 行（包装帧号 7）。
    let suite = dir.child("suite-async-fail.js");
    suite
        .write_str(
            r#"
'use strict';
const assert = require('assert');
setTimeout(() => {
  // mapper-marker: assert on next line
  assert.strictEqual('a', 'b');
}, 10);
"#,
        )
        .unwrap();
    let (ok, out) = run_suite_mapped(&dir, suite.path().to_str().unwrap());
    assert!(!ok);
    assert!(out.contains("[mapper-actual] \"a\""), "out:\n{out}");
    assert!(out.contains("[mapper-expected] \"b\""), "out:\n{out}");
    assert!(
        out.contains("suite-async-fail.js:7:10 (physical 6)"),
        "callsite must be the suite frame (not assert SOURCE); out:\n{out}"
    );
    // 物理行折算：帧 7 - CJS 前奏 1 = 物理 6，`>>` 标注在 assert 行。
    assert!(out.contains(">>    6|   assert.strictEqual"), "out:\n{out}");
    assert!(out.contains("mapper-marker"), "excerpt ±2 lines; out:\n{out}");
    assert!(out.contains("assert.strictEqual('a', 'b')"), "out:\n{out}");
    assert!(
        out.contains("ERR_ASSERTION") && out.contains("operator=strictEqual"),
        "out:\n{out}"
    );
    dir.close().unwrap();
}

/// 边界件：套件通过 → mapper 零标签、输出透传。
#[test]
fn phase_mapper_passthrough_on_success() {
    let dir = assert_fs::TempDir::new().unwrap();
    let suite = dir.child("suite-pass.js");
    suite.write_str("console.log('suite-ok-line');\n").unwrap();
    let (ok, out) = run_suite_mapped(&dir, suite.path().to_str().unwrap());
    assert!(ok);
    assert!(out.contains("suite-ok-line"), "out:\n{out}");
    assert!(!out.contains("[mapper-actual]"), "out:\n{out}");
    dir.close().unwrap();
}

/// §4.202-① 验收工具（定位器非闸门，红绿不进门）：
/// `WJS_MAP_SUITE=test-http-raw-headers.js cargo test --test node \
///   phase_mapper_locate_suite -- --ignored --nocapture`
/// 套件名按 /tmp/wjs-node-test/test/parallel 解析，也可给绝对路径。
#[test]
#[ignore]
fn phase_mapper_locate_suite() {
    let Some(p) = std::env::var("WJS_MAP_SUITE").ok() else {
        eprintln!("WJS_MAP_SUITE 未设：套件文件名（vendor parallel 树）或绝对路径");
        return;
    };
    let path = if std::path::Path::new(&p).exists() {
        p
    } else {
        format!("/tmp/wjs-node-test/test/parallel/{p}")
    };
    let dir = assert_fs::TempDir::new().unwrap();
    let (ok, out) = run_suite_mapped(&dir, &path);
    println!("[mapper] {} rc={}", path, if ok { 0 } else { 1 });
    println!("{out}");
}
