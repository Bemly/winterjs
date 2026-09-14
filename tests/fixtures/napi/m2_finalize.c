// napi M2 finalize 释放链 fixture：external + wrap 实例全走 GC finalizer 释放
// malloc 内存。计数器对（alloc/free）经 counts() 回读。
// §4.77/§4.78 语义：宿主槽位不截断 + finalizer 延迟收敛（GC sweep 内只入队，
// dispatch 安全点排空）——中途 free 不再追上 alloc；`drain(cb)` 排一个
// async_work，其 complete（安全点，宿主已排空 pending finalizer）回吐计数，
// 供测试断言"finalizer 真会跑 + free ≤ alloc 恒成立"。
#include <node_api.h>
#include <stdio.h>
#include <stdlib.h>

static napi_value g_box_ctor = NULL;
static long g_ext_allocs = 0;
static long g_ext_frees = 0;
static long g_wrap_allocs = 0;
static long g_wrap_frees = 0;
static napi_ref g_drain_cb_ref = NULL; // 跨回调持 JS 回调必须走 ref（Node 契约）

static void ext_finalizer(napi_env env, void *data, void *hint) {
  (void)env; (void)hint;
  free(data);
  g_ext_frees++;
}

static void wrap_finalizer(napi_env env, void *data, void *hint) {
  (void)env; (void)hint;
  free(data);
  g_wrap_frees++;
}

typedef struct { long id; char pad[56]; } box_t; // ~64B，放大 nursery 占比

// mk(): 建 external + wrap 实例各一，不保留 JS 引用（返回 undefined）
static napi_value Mk(napi_env env, napi_callback_info info) {
  char *blob = malloc(64);
  g_ext_allocs++;
  *(long *)blob = g_ext_allocs;
  napi_value ext;
  if (napi_create_external(env, blob, ext_finalizer, NULL, &ext) != napi_ok) {
    free(blob); g_ext_allocs--;
    return NULL;
  }
  (void)ext; // 无引用即死

  box_t *b = malloc(sizeof(box_t));
  g_wrap_allocs++;
  b->id = g_wrap_allocs;
  napi_value inst;
  if (napi_new_instance(env, g_box_ctor, 0, NULL, &inst) != napi_ok) {
    free(b); g_wrap_allocs--;
    return NULL;
  }
  if (napi_wrap(env, inst, b, wrap_finalizer, NULL, NULL) != napi_ok) {
    free(b); g_wrap_allocs--; // 实例本体照常存活（无 wrap）
    return NULL;
  }
  return NULL;
}

// counts(): [ext_allocs, ext_frees, wrap_allocs, wrap_frees]
static napi_value Counts(napi_env env, napi_callback_info info) {
  napi_value arr, out;
  napi_create_array_with_length(env, 4, &arr);
  long vals[4] = { g_ext_allocs, g_ext_frees, g_wrap_allocs, g_wrap_frees };
  for (int i = 0; i < 4; i++) {
    napi_create_int64(env, vals[i], &out);
    napi_set_element(env, arr, (uint32_t)i, out);
  }
  return arr;
}

// drain(cb): 排一个 async_work；complete 在安全点（宿主 dispatch 入口已排空
// pending finalizer）调 cb(counts)——延迟收敛后的 free 计数在此可见。
static void drain_execute(napi_env env, void *data) { (void)env; (void)data; }
static void drain_complete(napi_env env, napi_status status, void *data) {
  (void)status; (void)data;
  napi_value arr, out, undef, cb;
  if (napi_get_reference_value(env, g_drain_cb_ref, &cb) != napi_ok) return;
  napi_create_array_with_length(env, 4, &arr);
  long vals[4] = { g_ext_allocs, g_ext_frees, g_wrap_allocs, g_wrap_frees };
  for (int i = 0; i < 4; i++) {
    napi_create_int64(env, vals[i], &out);
    napi_set_element(env, arr, (uint32_t)i, out);
  }
  napi_get_undefined(env, &undef);
  napi_call_function(env, undef, cb, 1, &arr, NULL);
}
static napi_value Drain(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok) return NULL;
  if (g_drain_cb_ref == NULL &&
      napi_create_reference(env, argv[0], 1, &g_drain_cb_ref) != napi_ok) return NULL;
  napi_value name;
  napi_async_work work;
  if (napi_create_string_utf8(env, "drain", NAPI_AUTO_LENGTH, &name) != napi_ok) return NULL;
  if (napi_create_async_work(env, NULL, name, drain_execute, drain_complete, NULL, &work) != napi_ok) return NULL;
  if (napi_queue_async_work(env, work) != napi_ok) return NULL;
  return NULL;
}

static napi_value BoxCtor(napi_env env, napi_callback_info info) {
  napi_value this_v;
  napi_get_cb_info(env, info, NULL, NULL, &this_v, NULL);
  (void)this_v;
  return NULL;
}

static napi_value Init(napi_env env, napi_value exports) {
  if (napi_define_class(env, "Box", NAPI_AUTO_LENGTH, BoxCtor, NULL, 0, NULL, &g_box_ctor) != napi_ok) return NULL;
  napi_value mk, counts, drain;
  napi_create_function(env, "mk", NAPI_AUTO_LENGTH, Mk, NULL, &mk);
  napi_create_function(env, "counts", NAPI_AUTO_LENGTH, Counts, NULL, &counts);
  napi_create_function(env, "drain", NAPI_AUTO_LENGTH, Drain, NULL, &drain);
  napi_set_named_property(env, exports, "mk", mk);
  napi_set_named_property(env, exports, "counts", counts);
  napi_set_named_property(env, exports, "drain", drain);
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
