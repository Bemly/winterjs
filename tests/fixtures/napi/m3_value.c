#include <node_api.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// promise: create deferred → JS then() 挂钩 → resolve → JS 收值
static napi_deferred g_deferred = NULL;
static napi_value MakeDeferred(napi_env env, napi_callback_info info) {
  napi_value promise;
  if (napi_create_promise(env, &g_deferred, &promise) != napi_ok) return NULL;
  return promise;
}
static napi_value ResolveIt(napi_env env, napi_callback_info info) {
  napi_value v;
  napi_create_int32(env, 77, &v);
  if (napi_resolve_deferred(env, g_deferred, v) != napi_ok) return NULL;
  g_deferred = NULL; // 一次性
  napi_value out; napi_create_int32(env, 1, &out);
  return out;
}
static napi_value RejectIt(napi_env env, napi_callback_info info) {
  napi_value promise, err;
  napi_create_promise(env, &g_deferred, &promise);
  napi_create_string_utf8(env, "nope", NAPI_AUTO_LENGTH, &err);
  napi_reject_deferred(env, g_deferred, err);
  g_deferred = NULL;
  return promise;
}
static napi_value IsPromise(napi_env env, napi_callback_info info) {
  size_t argc = 1; napi_value argv[1], out;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  bool is = false;
  napi_is_promise(env, argv[0], &is);
  napi_create_int32(env, is, &out);
  return out;
}
// refs: create → ref/unref 计数 → get_reference_value 回读 → delete
static napi_value CheckRef(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value v, out, got;
  napi_create_int32(env, 42, &v);
  napi_ref r;
  uint32_t count = 0;
  if (napi_create_reference(env, v, 1, &r) != napi_ok) return NULL;
  napi_reference_ref(env, r, &count); if (count != 2) ok = 0;
  napi_reference_unref(env, r, &count); if (count != 1) ok = 0;
  if (napi_get_reference_value(env, r, &got) != napi_ok) ok = 0;
  double d = 0; napi_get_value_double(env, got, &d); if (d != 42) ok = 0;
  napi_delete_reference(env, r);
  if (napi_get_reference_value(env, r, &got) == napi_ok) ok = 0; // 已删
  napi_create_int32(env, ok, &out);
  return out;
}
// typedarray: 建 AB → 建 Float64Array 视图 → info 回读 → 元素写读
static napi_value CheckTyped(napi_env env, napi_callback_info info) {
  int ok = 1;
  void *ab_data = NULL;
  napi_value ab, ta, out;
  if (napi_create_arraybuffer(env, 8 * sizeof(double), &ab_data, &ab) != napi_ok) return NULL;
  if (napi_create_typedarray(env, napi_float64_array, 8, ab, 0, &ta) != napi_ok) return NULL;
  napi_typedarray_type t; size_t len, off; void *ta_data; napi_value ab_back;
  if (napi_get_typedarray_info(env, ta, &t, &len, &ta_data, &ab_back, &off) != napi_ok) return NULL;
  if (t != napi_float64_array || len != 8 || off != 0 || ta_data != ab_data) ok = 0;
  // addon 侧直写内存 → JS 读
  double *d = (double *)ta_data;
  for (int i = 0; i < 8; i++) d[i] = i * 1.5;
  // DataView 同 AB
  napi_value dv;
  if (napi_create_dataview(env, 16, ab, 0, &dv) != napi_ok) return NULL;
  size_t dlen, doff; void *ddata; napi_value dab;
  napi_get_dataview_info(env, dv, &dlen, &ddata, &dab, &doff);
  if (dlen != 16 || doff != 0 || ddata != ab_data) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value MakeTA(napi_env env, napi_callback_info info) {
  // BigInt64Array 面探针：建 4 元素视图
  void *d = NULL;
  napi_value ab, ta;
  napi_create_arraybuffer(env, 4 * 8, &d, &ab);
  napi_create_typedarray(env, napi_bigint64_array, 4, ab, 0, &ta);
  return ta;
}
// buffer: create → addon 写 → JS 读（Buffer.toString）
static napi_value MakeBuf(napi_env env, napi_callback_info info) {
  void *d = NULL;
  napi_value buf;
  if (napi_create_buffer(env, 5, &d, &buf) != napi_ok) return NULL;
  memcpy(d, "hello", 5);
  return buf;
}
static napi_value CheckBuf(napi_env env, napi_callback_info info) {
  int ok = 1;
  size_t argc = 1; napi_value argv[1], out;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  bool is = false;
  napi_is_buffer(env, argv[0], &is); if (!is) ok = 0;
  void *d = NULL; size_t len = 0;
  napi_get_buffer_info(env, argv[0], &d, &len);
  if (len != 5 || d == NULL || memcmp(d, "hello", 5) != 0) ok = 0;
  // external buffer + finalize 计数
  napi_value eb;
  char *ext = malloc(8);
  memcpy(ext, "extbuf!!", 8);
  if (napi_create_external_buffer(env, 8, ext, NULL, NULL, &eb) != napi_ok) ok = 0;
  bool is2 = false;
  napi_is_buffer(env, eb, &is2); if (!is2) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value Init(napi_env env, napi_value exports) {
  struct { const char *name; napi_callback cb; } table[] = {
    {"makeDeferred", MakeDeferred}, {"resolveIt", ResolveIt},
    {"rejectIt", RejectIt}, {"isPromise", IsPromise}, {"checkRef", CheckRef},
    {"checkTyped", CheckTyped}, {"makeTA", MakeTA}, {"makeBuf", MakeBuf},
    {"checkBuf", CheckBuf},
  };
  for (unsigned long i = 0; i < sizeof(table)/sizeof(table[0]); i++) {
    napi_value fn;
    if (napi_create_function(env, table[i].name, NAPI_AUTO_LENGTH, table[i].cb, NULL, &fn) != napi_ok) return NULL;
    if (napi_set_named_property(env, exports, table[i].name, fn) != napi_ok) return NULL;
  }
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
