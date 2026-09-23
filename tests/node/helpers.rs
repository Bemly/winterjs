//! tests/node/helpers.rs — node 域共享脚手架（run_*_file + 自签证书 + 对拍 mapper）。

use crate::common::*;
use assert_fs::prelude::*;

/// 对拍 mapper 包装壳模板（§4.202-①）：套件路径内嵌 `__SUITE__`，装
/// `uncaughtException` 监听——栈里首个套件文件帧即真实调用点
/// （无壳时宿主上报 assert SOURCE 内部行号 `node:assert:9:5` / prelude
/// `424:53`，却安着套件文件名——包装位置是骗的）。AssertionError 自带
/// `actual/expected/operator/code`（JSON 可序列化），无需解析消息文本。
const SUITE_MAPPER_JS: &str = r##"'use strict';
// [mapper] §4.202-① 对拍定位壳（tests/node/helpers.rs 生成；勿手改套件）。
const __suite = "__SUITE__";
const __base = __suite.split("/").pop();
const __cut = (v) => {
  const t = JSON.stringify(v);
  if (typeof t !== "string") return String(t);
  return t.length > 200 ? t.slice(0, 200) + "...<cut," + t.length + ">" : t;
};
const __frames = (e) => { try { return String(e && e.stack || "").split("\n"); } catch (x) { return []; } };
const __site = (e) => {
  const fr = __frames(e).map((f) => f.trim()).filter(Boolean);
  const hit = fr.find((f) => f.indexOf(__base) !== -1);
  return hit || fr[0] || "no-frame";
};
// 栈帧行号 = 物理行 + CJS 包装前奏行数（.js 恒 1，实测 6/6；.mjs 无包装记 0）。
const __shift = __suite.endsWith(".mjs") ? 0 : 1;
const __src = (line) => {
  try {
    const fs = require("fs");
    const lines = fs.readFileSync(__suite, "utf8").split("\n");
    const n = (Number(line) || 0) - __shift;
    if (n < 1 || n > lines.length) return "";
    const out = [];
    for (let i = Math.max(1, n - 2); i <= Math.min(lines.length, n + 2); i++) {
      out.push((i === n ? ">> " : "   ") + ("    " + i).slice(-4) + "| " + lines[i - 1]);
    }
    return out.join("\n");
  } catch (x) { return ""; }
};
const __report = (tag, e) => {
  console.log("[mapper] suite " + __base + " tag=" + tag + " code=" + (e && e.code) + " operator=" + (e && e.operator));
  console.log("[mapper-actual] " + __cut(e && e.actual));
  console.log("[mapper-expected] " + __cut(e && e.expected) + " msg=" + __cut(e && e.message));
  const site = __site(e);
  const m = site.match(/:(\d+):(\d+)/);
  const phys = m ? Number(m[1]) - __shift : 0;
  console.log("[mapper-callsite] " + site + (m ? " (physical " + phys + ")\n" + __src(m[1]) : ""));
  process.exit(1);
};
process.on("uncaughtException", (e) => __report("uncaught", e));
process.on("unhandledRejection", (r) => __report("rejection", r instanceof Error ? r : { message: String(r) }));
require(__suite);
"##;

fn suite_mapper_src(suite_path: &str) -> String {
    SUITE_MAPPER_JS.replace("__SUITE__", suite_path)
}

/// 跑一个 node 套件文件（vendor 树或任意路径），失败时一次输出定位三行：
/// `[mapper-actual]`（JSON.stringify 截断 200 字）/ `[mapper-callsite]`
/// （套件侧真实调用点 行:列 + 源行）/ `[mapper-expected]`（调用点上下文
/// ±2 行节选）。返回 (是否通过, 合并输出)。
///
/// 实测边界（2026-09-23，探针钉住）：`unhandledRejection` 监听拦不到
/// （引擎自有未处理 rejection 收割先走，§4.137 路径——rejection 形失败
/// 回落引擎默认输出，位置仍骗）；`process.on('exit')` 在 fatal 路径不触发
/// （宿主 process::exit 跳过，无兜底臂）；栈帧行号 = 物理行 + CJS 包装
/// 前奏行数（`.js` 恒 +1，6 处实验一致——含 vendor 套件，`raw-headers`
/// 帧 111 = 物理 110 的 rawHeaders 断言；`.mjs` 无包装记 0），mapper 按
/// 此折算物理行标注 `>>`。TIMEOUT 件自带 20s 看门，超时杀进程报
/// `[mapper] TIMEOUT`，不挂 cargo test。
pub(crate) fn run_suite_mapped(dir: &assert_fs::TempDir, suite_path: &str) -> (bool, String) {
    let wrapper = dir.child("mapper-wrapper.js");
    wrapper.write_str(&suite_mapper_src(suite_path)).unwrap();
    use std::io::Read as _;
    // std Command（非 assert_cmd）：mapper 需要 spawn + 管道 + try_wait 看门。
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_winterjs"))
        .arg("--run")
        .arg(wrapper.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("binary spawns");
    let so = child.stdout.take().expect("stdout pipe");
    let se = child.stderr.take().expect("stderr pipe");
    let t1 = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut so = so;
        let _ = so.read_to_end(&mut buf);
        buf
    });
    let t2 = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut se = se;
        let _ = se.read_to_end(&mut buf);
        buf
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(s) => break Some(s),
            None => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    };
    let out = t1.join().unwrap();
    let err = t2.join().unwrap();
    let mut text = String::from_utf8_lossy(&out).into_owned();
    text.push_str(&String::from_utf8_lossy(&err));
    let ok = match status {
        Some(s) => s.success(),
        None => {
            text.push_str("\n[mapper] TIMEOUT 20s（套件挂死；根因定位走 instrument，不经 mapper）");
            false
        }
    };
    (ok, text)
}

pub(crate) fn run_node_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> std::process::Output {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    winterjs().arg("--run").arg(file.path()).output().unwrap()
}

/// node:fs 脚手架（workdir 内跑模块；返回 stdout）。
pub(crate) fn run_fs_file(dir: &assert_fs::TempDir, name: &str, source: &str) -> String {
    let file = dir.child(name);
    file.write_str(source).unwrap();
    let out = winterjs()
        .arg("--run")
        .arg(file.path())
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

/// rcgen 自签 end-entity 证书（SAN 127.0.0.1；落盘供 JS 侧 readFileSync）。


pub(crate) fn write_self_signed(dir: &assert_fs::TempDir) -> (String, String) {
    let key = rcgen::generate_simple_self_signed(vec!["127.0.0.1".to_string()]).unwrap();
    let cert_pem = key.cert.pem();
    let key_pem = key.signing_key.serialize_pem();
    let cert = dir.child("t-cert.pem");
    cert.write_str(&cert_pem).unwrap();
    let k = dir.child("t-key.pem");
    k.write_str(&key_pem).unwrap();
    (
        cert.path().to_string_lossy().into_owned(),
        k.path().to_string_lossy().into_owned(),
    )
}
