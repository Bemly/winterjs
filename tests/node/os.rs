//! tests/node/os.rs — 对齐 src/builtins/node/os.rs（node:os）。

use crate::common::*;
use assert_fs::prelude::*;

#[test]
fn phase4_node_os_basic() {
    let out = stdout_of(&mut winterjs().args(["--eval",
        r#"const os = await import("node:os"); console.log([os.platform(), os.arch()].join(",")); console.log(os.EOL.length, os.hostname().length > 0, os.tmpdir().length > 0, os.totalmem() > 0, os.freemem() >= 0, os.cpus().length > 0, typeof os.cpus()[0].model, Object.keys(os.networkInterfaces()).length > 0, os.userInfo().username.length >= 0, os.uptime() >= 0, os.loadavg().length, os.release().length >= 0);"#]));
    let mut lines = out.lines();
    let pa = lines.next().unwrap_or("");
    assert!(
        ["darwin", "linux", "win32", "android"].contains(&pa.split(',').next().unwrap_or("")),
        "platform: {pa}"
    );
    assert!(
        ["arm64", "x64", "arm"].contains(&pa.split(',').nth(1).unwrap_or("")),
        "arch: {pa}"
    );
    assert_eq!(
        lines.next().unwrap_or(""),
        "1 true true true true true string true true true 3 true",
        "os: {out}"
    );
}

#[test]
fn phase10f_os_surface() {
    // 台面：EOL/devNull 描述符 + 常量表 + 动态 tmpdir/homedir + 新面 + buffer + 原语转换 + cidr。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import os from "node:os";
const isWin = os.platform() === "win32";
const d = Object.getOwnPropertyDescriptor(os, "EOL");
console.log("eol-desc", d.writable === false && d.configurable === true);
let threw = false;
try { os.EOL = 123; } catch { threw = true; }
console.log("eol-frozen", threw);
Object.defineProperties(os, { EOL: { configurable: true, enumerable: true, writable: false, value: "foo" } });
console.log("eol-redef", os.EOL);
console.log("devnull", isWin ? os.devNull === "\\\\.\\nul" : os.devNull === "/dev/null");
console.log("prio", os.constants.priority.PRIORITY_LOW === 19 && os.constants.priority.PRIORITY_HIGHEST === -20);
let sigThrow = false;
try { os.constants.signals.FOOBAR = 1337; } catch { sigThrow = true; }
console.log("sig-frozen", sigThrow);
if (!isWin) {
  process.env.TMPDIR = "/tmpdir"; process.env.TMP = "/tmp"; process.env.TEMP = "/temp";
  const r = [os.tmpdir()];
  process.env.TMPDIR = ""; r.push(os.tmpdir());
  process.env.TMP = ""; r.push(os.tmpdir());
  process.env.TEMP = ""; r.push(os.tmpdir());
  process.env.TMPDIR = "/tmpdir/"; r.push(os.tmpdir());
  process.env.TMPDIR = "/"; r.push(os.tmpdir());
  console.log("tmpdir", JSON.stringify(r));
  const h0 = process.env.HOME;
  console.log("home-eq", os.homedir() === h0);
  delete process.env.HOME;
  console.log("home-fallback", os.homedir().includes("/"));
  if (h0 === undefined) delete process.env.HOME; else process.env.HOME = h0;
} else {
  console.log("tmpdir", "win-skip");
  console.log("home-eq", true);
  console.log("home-fallback", true);
}
console.log("newface", ["LE", "BE"].includes(os.endianness()), typeof os.machine(), !!os.version());
const u = os.userInfo(), ub = os.userInfo({ encoding: "buffer" });
console.log("userbuf", u.username === ub.username.toString("utf8") && u.homedir === ub.homedir.toString("utf8") && typeof ub.uid === "number");
console.log("prim", `${os.hostname}` === os.hostname() && `${os.tmpdir}` === os.tmpdir() && `${os.endianness}` === os.endianness());
console.log("cidr", Object.values(os.networkInterfaces()).flat().every((e) => typeof e.cidr === "string" && typeof e.netmask === "string"));
"#,
    )
    .unwrap();
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
    // win 上 env 语义未经真机 CI，只断平台无关行；unix 逐行精确。
    if cfg!(windows) {
        let stdout = String::from_utf8(out.stdout).unwrap();
        for line in [
            "eol-desc true",
            "eol-frozen true",
            "sig-frozen true",
            "newface true string true",
            "userbuf true",
            "cidr true",
        ] {
            assert!(
                stdout.lines().any(|l| l == line),
                "missing: {line}\n{stdout}"
            );
        }
    } else {
        assert_eq!(
            String::from_utf8(out.stdout).unwrap(),
            "eol-desc true\neol-frozen true\neol-redef foo\ndevnull true\nprio true\nsig-frozen true\ntmpdir [\"/tmpdir\",\"/tmp\",\"/temp\",\"/tmp\",\"/tmpdir\",\"/\"]\nhome-eq true\nhome-fallback true\nnewface true string true\nuserbuf true\nprim true\ncidr true\n",
            "os surface: {}",
            String::from_utf8_lossy(&out.stderr),
        );
    }
    dir.close().unwrap();
}

#[test]
fn phase10f_os_priority() {
    // 校验三件 + SystemError 形态 + 活体只降不升（回滚容忍 EACCES/EPERM）。
    let dir = assert_fs::TempDir::new().unwrap();
    let file = dir.child("p.mjs");
    file.write_str(
        r#"
import os from "node:os";
const C = os.constants.priority;
console.log("vals", C.PRIORITY_LOW === 19 && C.PRIORITY_HIGHEST === -20);
for (const pid of [null, "foo"]) {
  try { os.setPriority(pid, 0); console.log("NO-THROW"); }
  catch (e) { console.log(e.code === "ERR_INVALID_ARG_TYPE" ? "ok" : "FAIL " + e.code); }
  try { os.getPriority(pid); console.log("NO-THROW"); }
  catch (e) { console.log(e.code === "ERR_INVALID_ARG_TYPE" ? "ok" : "FAIL " + e.code); }
}
for (const pid of [NaN, 3.14, 2 ** 32]) {
  try { os.setPriority(pid, 0); console.log("NO-THROW"); }
  catch (e) { console.log(e.code === "ERR_OUT_OF_RANGE" ? "ok" : "FAIL " + e.code); }
  try { os.getPriority(pid); console.log("NO-THROW"); }
  catch (e) { console.log(e.code === "ERR_OUT_OF_RANGE" ? "ok" : "FAIL " + e.code); }
}
for (const pr of ["foo", NaN, -21, 20]) {
  try { os.setPriority(0, pr); console.log("NO-THROW"); }
  catch (e) {
    const want = typeof pr === "number" ? "ERR_OUT_OF_RANGE" : "ERR_INVALID_ARG_TYPE";
    console.log(e.code === want ? "ok" : "FAIL " + e.code);
  }
}
try { os.getPriority(-1); console.log("NO-THROW"); }
catch (e) {
  console.log(e.name === "SystemError" && e.code === "ERR_SYSTEM_ERROR" &&
    /uv_os_getpriority returned /.test(e.message) &&
    typeof e.errno === "number" && e.info && typeof e.info.code === "string" ? "ok" : "FAIL " + e.name);
}
const cur = os.getPriority();
console.log("live-num", typeof cur === "number");
const down = cur >= 19 ? 19 : cur + 1;
os.setPriority(down);
console.log("live-set", os.getPriority() === down);
try { os.setPriority(cur); console.log("restored", os.getPriority() === cur); }
catch (e) { console.log(e.code === "ERR_SYSTEM_ERROR" ? "restore-denied" : "FAIL " + e.code); }
"#,
    )
    .unwrap();
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
    let stdout = String::from_utf8(out.stdout).unwrap();
    let mut lines = stdout.lines();
    assert_eq!(lines.next().unwrap_or(""), "vals true");
    for _ in 0..14 {
        assert_eq!(lines.next().unwrap_or(""), "ok", "priority validation: {stdout}");
    }
    assert_eq!(lines.next().unwrap_or(""), "ok", "getPriority(-1): {stdout}");
    assert_eq!(lines.next().unwrap_or(""), "live-num true");
    assert_eq!(lines.next().unwrap_or(""), "live-set true");
    let last = lines.next().unwrap_or("");
    assert!(
        last == "restored true" || last == "restore-denied",
        "restore: {stdout}"
    );
    dir.close().unwrap();
}
