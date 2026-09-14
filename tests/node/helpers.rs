//! tests/node/helpers.rs — node 域共享脚手架（run_*_file + 自签证书）。

use crate::common::*;
use assert_fs::prelude::*;

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
