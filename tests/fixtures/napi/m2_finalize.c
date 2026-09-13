// napi M2 finalize 释放链 fixture：external + wrap 实例全走 GC finalizer 释放
// malloc 内存。计数器对（alloc/free）经 counts() 回读——free 追上 alloc 即
// 链路闭合（宿主 trampoline 槽位截断 → 对象可达死态 → minor GC → finalize op）。
#include <node_api.h>
#include <stdio.h>
#include <stdlib.h>

static napi_value g_box_ctor = NULL;
static long g_ext_allocs = 0;
static long g_ext_frees = 0;
static long g_wrap_allocs = 0;
static long g_wrap_frees = 0;

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

static napi_value BoxCtor(napi_env env, napi_callback_info info) {
  napi_value this_v;
  napi_get_cb_info(env, info, NULL, NULL, &this_v, NULL);
  (void)this_v;
  return NULL;
}

static napi_value Init(napi_env env, napi_value exports) {
  if (napi_define_class(env, "Box", NAPI_AUTO_LENGTH, BoxCtor, NULL, 0, NULL, &g_box_ctor) != napi_ok) return NULL;
  napi_value mk, counts;
  napi_create_function(env, "mk", NAPI_AUTO_LENGTH, Mk, NULL, &mk);
  napi_create_function(env, "counts", NAPI_AUTO_LENGTH, Counts, NULL, &counts);
  napi_set_named_property(env, exports, "mk", mk);
  napi_set_named_property(env, exports, "counts", counts);
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
