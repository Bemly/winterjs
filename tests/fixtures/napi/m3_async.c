// napi M3 异步面 fixture：async_work（真 OS 线程 execute → complete 回 JS）+
// TSFN（OS 线程 call_threadsafe_function → call_js_cb 回 JS → release →
// thread_finalize）+ cancel/delete 语义。keep-alive 由事件循环退出条件覆盖
// （pending 未清零则进程不退，黑盒超时即挂）。
#include <node_api.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <pthread.h>

// ── async_work：execute 在线程算 fib(n)，complete resolve deferred ────────
typedef struct { int32_t n; long result; napi_deferred deferred; } work_arg_t;

static long fib(long x) {
  if (x < 2) return x;
  return fib(x - 1) + fib(x - 2);
}

static void WorkExecute(napi_env env, void *data) {
  (void)env; // 禁 JSAPI（Node 契约）
  work_arg_t *a = (work_arg_t *)data;
  a->result = fib(a->n);
}

static void WorkComplete(napi_env env, napi_status status, void *data) {
  work_arg_t *a = (work_arg_t *)data;
  if (status == napi_ok) {
    napi_value v;
    napi_create_int64(env, a->result, &v);
    napi_resolve_deferred(env, a->deferred, v);
  } else {
    napi_value v;
    napi_create_string_utf8(env, "cancelled", NAPI_AUTO_LENGTH, &v);
    napi_resolve_deferred(env, a->deferred, v);
  }
  free(a);
}

static napi_value StartWork(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], promise;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  work_arg_t *a = malloc(sizeof(work_arg_t));
  a->n = 0;
  if (argc > 0) napi_get_value_int32(env, argv[0], &a->n);
  napi_create_promise(env, &a->deferred, &promise);
  napi_async_work w;
  if (napi_create_async_work(env, NULL, NULL, WorkExecute, WorkComplete, a, &w) != napi_ok) {
    free(a);
    return NULL;
  }
  napi_queue_async_work(env, w);
  return promise; // deferred 存续于 complete（work_arg 持句柄），promise 归 JS
}

// cancel：建而不排队 → cancel → complete(cancelled) → "cancelled"
static napi_value CancelProbe(napi_env env, napi_callback_info info) {
  work_arg_t *a = malloc(sizeof(work_arg_t));
  a->n = 0;
  napi_value promise;
  napi_create_promise(env, &a->deferred, &promise);
  napi_async_work w;
  napi_create_async_work(env, NULL, NULL, WorkExecute, WorkComplete, a, &w);
  if (napi_cancel_async_work(env, w) != napi_ok) return NULL;
  napi_delete_async_work(env, w);
  return promise;
}

// delete：建而不排队直接删（无 complete；泄漏计数探针在 counts 读回）
static int g_deleted_ok = 0;
static napi_value DeleteProbe(napi_env env, napi_callback_info info) {
  work_arg_t *a = malloc(sizeof(work_arg_t));
  a->n = 0;
  napi_value promise;
  napi_create_promise(env, &a->deferred, &promise); // 永不落定（探针不 await）
  napi_async_work w;
  napi_create_async_work(env, NULL, NULL, WorkExecute, WorkComplete, a, &w);
  g_deleted_ok = (napi_delete_async_work(env, w) == napi_ok);
  free(a); // 自管（complete 不会来）
  napi_value out;
  napi_create_int32(env, g_deleted_ok, &out);
  return out;
}

// ── TSFN：线程发 3 条消息 → call_js_cb 回 JS → release → finalize 落定 ────
typedef struct {
  napi_threadsafe_function tsfn;
  napi_deferred deferred;
} tsfn_arg_t;

static void TsfnCallJs(napi_env env, napi_value js_cb, void *context, void *data) {
  (void)context;
  // data = malloc 的字符串（所有权在此消费）
  napi_value s, undef, r;
  napi_create_string_utf8(env, (const char *)data, NAPI_AUTO_LENGTH, &s);
  napi_get_undefined(env, &undef);
  napi_call_function(env, undef, js_cb, 1, &s, &r);
  free(data);
}

static void TsfnThreadFinalize(napi_env env, void *data, void *hint) {
  (void)hint;
  tsfn_arg_t *a = (tsfn_arg_t *)data; // thread_finalize_data（dispatch 按 data 传）
  napi_value v;
  napi_create_string_utf8(env, "done", NAPI_AUTO_LENGTH, &v);
  napi_resolve_deferred(env, a->deferred, v);
  free(a);
}

static void *TsfnWorker(void *p) {
  tsfn_arg_t *a = (tsfn_arg_t *)p;
  const char *msgs[3] = { "m1", "m2", "m3" };
  for (int i = 0; i < 3; i++) {
    char *d = malloc(8);
    snprintf(d, 8, "%s", msgs[i]);
    napi_status st = napi_call_threadsafe_function(a->tsfn, d, napi_tsfn_blocking);
    usleep(1000);
  }
  // 3 条消息送达后释放（0 线程 → closing → finalize → resolve "done"）
  napi_release_threadsafe_function(a->tsfn, napi_tsfn_release);
  return NULL;
}

static napi_value StartTsfn(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], promise;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  if (argc < 1) return NULL;
  tsfn_arg_t *a = malloc(sizeof(tsfn_arg_t));
  napi_create_promise(env, &a->deferred, &promise);
  if (napi_create_threadsafe_function(env, argv[0], NULL, NULL, 0, 1,
                                      a, TsfnThreadFinalize, NULL, TsfnCallJs,
                                      &a->tsfn) != napi_ok) {
    free(a);
    return NULL;
  }
  // ref/unref 面：先 unref 再 ref（API 回环；keep-alive 计数复位）
  napi_unref_threadsafe_function(env, a->tsfn);
  napi_ref_threadsafe_function(env, a->tsfn);
  // initial_thread_count=1 即线程占用；worker release 后 0 线程 → closing
  pthread_t th;
  pthread_create(&th, NULL, TsfnWorker, a);
  pthread_detach(th);
  return promise;
}

static napi_value Init(napi_env env, napi_value exports) {
  struct { const char *name; napi_callback cb; } table[] = {
    {"startWork", StartWork}, {"cancelProbe", CancelProbe},
    {"deleteProbe", DeleteProbe}, {"startTsfn", StartTsfn},
  };
  for (unsigned long i = 0; i < sizeof(table)/sizeof(table[0]); i++) {
    napi_value fn;
    if (napi_create_function(env, table[i].name, NAPI_AUTO_LENGTH, table[i].cb, NULL, &fn) != napi_ok) return NULL;
    if (napi_set_named_property(env, exports, table[i].name, fn) != napi_ok) return NULL;
  }
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
