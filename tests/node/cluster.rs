//! tests/node/cluster.rs — 对齐 src/builtins/node/cluster.rs（node:cluster，10e）。

use crate::helpers::*;

#[test]
fn phase10e_cluster_fork_message_exit() {
    // 主/子同文件：子端回消息→主端回信→子端断开→双边 exit，进程自然退出
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "p.mjs",
        r#"
import cluster from "node:cluster";
if (cluster.isPrimary) {
  console.log("role primary", cluster.isWorker === false, cluster.isMaster === true);
  cluster.on("fork", (w) => console.log("ev-fork", w.id, w.isDead() === false));
  cluster.on("online", (w) => console.log("ev-online", w.id));
  cluster.on("message", (w, m) => console.log("ev-message", w.id, m.n));
  cluster.on("disconnect", (w) => console.log("ev-disconnect", w.id, w.isConnected() === false));
  cluster.on("exit", (w, code) => console.log("ev-exit", w.id, code, Object.keys(cluster.workers).length));
  const w = cluster.fork({ WJS_CLUSTER_T10E: "t10e" });
  console.log("forked", w.id, cluster.workers[w.id] === w);
  w.on("message", (m) => {
    console.log("w-message", m.n, m.env);
    w.send({ pong: m.n + 1 });
  });
  w.on("exit", (code) => console.log("w-exit", code, w.isDead()));
} else {
  console.log("role worker", cluster.isWorker, cluster.worker.id, process.env.WJS_CLUSTER_T10E);
  process.send({ n: 41, env: process.env.WJS_CLUSTER_T10E });
  process.on("message", (m) => {
    console.log("got", m.pong);
    process.disconnect();
  });
}
"#,
    );
    assert!(out.contains("role primary true true"), "out: {out}");
    assert!(out.contains("forked 1 true"), "out: {out}");
    assert!(out.contains("ev-fork 1 true"), "out: {out}");
    assert!(out.contains("ev-online 1"), "out: {out}");
    assert!(out.contains("role worker true 1 t10e"), "out: {out}");
    assert!(out.contains("w-message 41 t10e"), "out: {out}");
    assert!(out.contains("ev-message 1 41"), "out: {out}");
    assert!(out.contains("got 42"), "out: {out}");
    assert!(out.contains("ev-disconnect 1 true"), "out: {out}");
    assert!(out.contains("w-exit 0 true"), "out: {out}");
    assert!(out.contains("ev-exit 1 0 0"), "out: {out}");
    dir.close().unwrap();
}

#[test]
fn phase10e_cluster_surface_errors() {
    // 不 fork 的纯面：常量/策略/设置/Worker 类/错误形状（hermetic，子端不参与）
    let dir = assert_fs::TempDir::new().unwrap();
    let out = run_fs_file(
        &dir,
        "q.mjs",
        r#"
import cluster, { SCHED_NONE, SCHED_RR, isPrimary, isWorker, Worker } from "node:cluster";
console.log("consts", SCHED_NONE, SCHED_RR, cluster.SCHED_NONE, cluster.SCHED_RR);
console.log("flags", isPrimary, isWorker, cluster.isPrimary, cluster.isWorker, cluster.isMaster);
console.log("policy", cluster.schedulingPolicy);
cluster.schedulingPolicy = SCHED_NONE;
console.log("policy2", cluster.schedulingPolicy);
try { cluster.schedulingPolicy = 99; } catch (e) { console.log("policy-bad", e.code); }
console.log("settings0", JSON.stringify(cluster.settings));
cluster.setupPrimary({ exec: "x.js", args: ["a"], silent: true });
console.log("settings1", cluster.settings.exec, cluster.settings.args[0], cluster.settings.silent);
cluster.setupMaster({});
console.log("settings2", JSON.stringify(cluster.settings));
try { cluster.setupPrimary(42); } catch (e) { console.log("setup-bad", e.code); }
console.log("worker-undef", cluster.worker === undefined, typeof Worker);
if (isWorker) process.exit(0);
cluster.setupPrimary({ exec: process.argv[1] });
const wenv = cluster.fork(42);
console.log("fork-env-ok", typeof wenv.id);
await new Promise((r) => wenv.on("exit", r));
console.log("fork-env-exit");
console.log("end-ok");
"#,
    );
    assert!(out.contains("consts 1 2 1 2"), "out: {out}");
    assert!(out.contains("flags true false true false true"), "out: {out}");
    assert!(out.contains("policy 2"), "out: {out}");
    assert!(out.contains("policy2 1"), "out: {out}");
    assert!(out.contains("policy-bad ERR_INVALID_ARG_VALUE"), "out: {out}");
    assert!(out.contains("settings0 {}"), "out: {out}");
    assert!(out.contains("settings1 x.js a true"), "out: {out}");
    assert!(out.contains("settings2 {}"), "out: {out}");
    assert!(out.contains("setup-bad ERR_INVALID_ARG_TYPE"), "out: {out}");
    assert!(out.contains("worker-undef true function"), "out: {out}");
    assert!(out.contains("fork-env-ok number"), "out: {out}");
    assert!(out.contains("fork-env-exit"), "out: {out}");
    assert!(out.contains("end-ok"), "out: {out}");
    dir.close().unwrap();
}
