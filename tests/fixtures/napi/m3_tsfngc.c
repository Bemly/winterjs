// napi M5 回归 fixture：TSFN 回调的 GC 根（env trace 漏标 `tsfns.js_cb`
// 即 M5 dev 真变更 139 根因，见 AGENTS §4.68）。线程延迟 600ms 投递 3 条，
// JS 侧先制造 GC 压力（5 万小对象 + 一轮 timer）再 await 落定；回调只被
// TSFN 记录持有——漏标则 GC 回收回调，投递即 SEGV。
#include <node_api.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <pthread.h>

typedef struct {
  napi_threadsafe_function tsfn;
  napi_deferred deferred;
} tsfn_gc_arg_t;

static void TsfnGcCallJs(napi_env env, napi_value js_cb, void *context, void *data) {
  (void)context;
  napi_value s, undef, r;
  napi_create_string_utf8(env, (const char *)data, NAPI_AUTO_LENGTH, &s);
  napi_get_undefined(env, &undef);
  napi_call_function(env, undef, js_cb, 1, &s, &r);
  free(data);
}

static void TsfnGcFinalize(napi_env env, void *data, void *hint) {
  (void)hint;
  tsfn_gc_arg_t *a = (tsfn_gc_arg_t *)data;
  napi_value v;
  napi_create_string_utf8(env, "done", NAPI_AUTO_LENGTH, &v);
  napi_resolve_deferred(env, a->deferred, v);
  free(a);
}

static void *TsfnGcWorker(void *p) {
  tsfn_gc_arg_t *a = (tsfn_gc_arg_t *)p;
  // 延迟投递：给 JS 侧留出 GC 窗口（回调只被 TSFN 记录持有）
  usleep(600 * 1000);
  const char *msgs[3] = { "g1", "g2", "g3" };
  for (int i = 0; i < 3; i++) {
    char *d = malloc(8);
    snprintf(d, 8, "%s", msgs[i]);
    napi_call_threadsafe_function(a->tsfn, d, napi_tsfn_blocking);
  }
  napi_release_threadsafe_function(a->tsfn, napi_tsfn_release);
  return NULL;
}

static napi_value StartGcTsfn(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], promise;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  if (argc < 1) return NULL;
  tsfn_gc_arg_t *a = malloc(sizeof(tsfn_gc_arg_t));
  napi_create_promise(env, &a->deferred, &promise);
  if (napi_create_threadsafe_function(env, argv[0], NULL, NULL, 0, 1,
                                      a, TsfnGcFinalize, NULL, TsfnGcCallJs,
                                      &a->tsfn) != napi_ok) {
    free(a);
    return NULL;
  }
  pthread_t th;
  pthread_create(&th, NULL, TsfnGcWorker, a);
  pthread_detach(th);
  return promise;
}

static napi_value Init(napi_env env, napi_value exports) {
  napi_value fn;
  if (napi_create_function(env, "startGcTsfn", NAPI_AUTO_LENGTH, StartGcTsfn, NULL, &fn) != napi_ok) return NULL;
  if (napi_set_named_property(env, exports, "startGcTsfn", fn) != napi_ok) return NULL;
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
