//! 黑盒测试共享 helper(各域测试文件经 `use common::*;` 引用)。
#![allow(dead_code)]

use assert_cmd::Command;
use assert_fs::prelude::*;

pub fn winterjs() -> Command {
    Command::cargo_bin("winterjs").expect("binary builds")
}

pub fn stdout_of(cmd: &mut Command) -> String {
    let out = cmd.output().expect("binary runs");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf8 stdout")
}

/// 起一个只 serving N 个请求的本机 HTTP 服务器（ephemeral 端口，hermetic）。
/// handler 收完整请求头（+ POST body），回包由闭包定。
pub fn serve_http(
    n: usize,
    handler: impl Fn(String, Vec<u8>) -> (u16, Vec<(&'static str, String)>, Vec<u8>) + Send + 'static,
) -> u16 {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut head = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let Ok(k) = s.read(&mut buf) else { break };
                if k == 0 {
                    break;
                }
                head.extend_from_slice(&buf[..k]);
                if head.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head_end = head
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .map(|i| i + 4)
                .unwrap_or(head.len());
            let head_str = String::from_utf8_lossy(&head[..head_end]).into_owned();
            let body_len = head_str
                .lines()
                .find_map(|l| {
                    l.strip_prefix("content-length:")
                        .or_else(|| l.strip_prefix("Content-Length:"))
                })
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            let mut body = head[head_end..].to_vec();
            while body.len() < body_len {
                let Ok(k) = s.read(&mut buf) else { break };
                if k == 0 {
                    break;
                }
                body.extend_from_slice(&buf[..k]);
            }
            body.truncate(body_len);
            let (status, headers, resp_body) = handler(head_str, body);
            let reason = if status == 200 { "OK" } else { "Error" };
            let mut resp = format!(
                "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n",
                resp_body.len()
            );
            for (k, v) in headers {
                resp.push_str(&format!("{k}: {v}\r\n"));
            }
            resp.push_str("\r\n");
            let _ = s.write_all(resp.as_bytes());
            let _ = s.write_all(&resp_body);
        }
    });
    port
}

#[cfg(unix)]
pub fn build_ffi_dylib(dir: &assert_fs::TempDir) -> String {
    let c = dir.child("ffitest.c");
    c.write_str(
        r#"
#include <stdint.h>
int32_t ffi_add(int32_t a, int32_t b) { return a + b; }
int64_t ffi_mul64(int64_t a, int64_t b) { return a * b; }
double ffi_mix(int32_t a, double b) { return a + b; }
double ffi_sum3(double a, double b, double c) { return a + b + c; }
uint8_t ffi_is_even(uint32_t n) { return (n % 2) == 0; }
void ffi_fill(uint8_t *buf, int32_t len, uint8_t v) { for (int32_t i = 0; i < len; i++) buf[i] = v; }
int32_t ffi_count_zeros(const uint8_t *buf, int32_t len) { int32_t n = 0; for (int32_t i = 0; i < len; i++) if (buf[i] == 0) n++; return n; }
const char *ffi_hello(void) { return "hi from c"; }
float ffi_f32ret(double x) { return (float)(x * 2.0); }
"#,
    )
    .unwrap();
    let name = if cfg!(target_os = "macos") {
        "libffitest.dylib"
    } else {
        "libffitest.so"
    };
    let out = dir.child(name);
    let mut cmd = std::process::Command::new("cc");
    if cfg!(target_os = "macos") {
        cmd.arg("-dynamiclib");
    } else {
        cmd.args(["-shared", "-fPIC"]);
    }
    let status = cmd
        .arg("-o")
        .arg(out.path())
        .arg(c.path())
        .status()
        .expect("cc must be available");
    assert!(status.success(), "cc failed");
    name.to_string()
}

pub fn wjs(args: &[&str], dir: &assert_fs::TempDir) -> (bool, String, String) {
    let out = winterjs()
        .args(args)
        .current_dir(dir.path())
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// 现场打一个最小 npm tarball（`package/` 前缀包裹；配 sha2 算 integrity 用）。
pub fn make_tgz(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar_data = Vec::new();
    {
        let mut ar = tar::Builder::new(&mut tar_data);
        for (name, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_path(format!("package/{name}")).unwrap();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            ar.append(&header, *data).unwrap();
        }
        ar.finish().unwrap();
    }
    use flate2::Compression;
    use flate2::write::GzEncoder;
    use std::io::Write as _;
    let mut enc = GzEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&tar_data).unwrap();
    enc.finish().unwrap()
}
