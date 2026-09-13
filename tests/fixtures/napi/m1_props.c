// napi M1 属性面 fixture：named/element 读写删、generic key、property_names、
// prototype、define_properties（value/method + data 指针 + attrs）。
#include <node_api.h>
#include <string.h>

static napi_value check_named(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value obj, v, key, got, out; bool has = false;
  napi_create_object(env, &obj);
  napi_create_int32(env, 7, &v);
  napi_set_named_property(env, obj, "x", v);
  napi_has_named_property(env, obj, "x", &has); if (!has) ok = 0;
  napi_get_named_property(env, obj, "x", &got);
  double d = 0; napi_get_value_double(env, got, &d); if (d != 7) ok = 0;
  napi_create_string_utf8(env, "x", NAPI_AUTO_LENGTH, &key);
  bool own = false; napi_has_own_property(env, obj, key, &own); if (!own) ok = 0;
  bool del = false; napi_delete_property(env, obj, key, &del);
  if (!del) ok = 0;
  napi_has_named_property(env, obj, "x", &has); if (has) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_generic_key(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value obj, key, val, got, out;
  napi_create_object(env, &obj);
  napi_create_string_utf8(env, "k", NAPI_AUTO_LENGTH, &key);
  napi_create_int32(env, 5, &val);
  napi_set_property(env, obj, key, val);
  napi_get_property(env, obj, key, &got);
  double d = 0; napi_get_value_double(env, got, &d); if (d != 5) ok = 0;
  bool has = false; napi_has_property(env, obj, key, &has); if (!has) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static int g_data_tag = 42;
static napi_value DataCb(napi_env env, napi_callback_info info) {
  void* data = NULL;
  napi_get_cb_info(env, info, NULL, NULL, NULL, &data);
  napi_value out;
  napi_create_int32(env, data != NULL && *(int*)data == 42, &out);
  return out;
}
static napi_value check_define_properties(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value obj, out;
  napi_create_object(env, &obj);
  napi_value plain; napi_create_int32(env, 3, &plain);
  napi_property_descriptor descs[] = {
    {"val", NULL, NULL, NULL, NULL, plain, (napi_property_attributes)(napi_writable | napi_enumerable | napi_configurable), NULL},
    {"method", NULL, DataCb, NULL, NULL, NULL, (napi_property_attributes)(napi_writable | napi_enumerable | napi_configurable), &g_data_tag},
    {"hidden", NULL, NULL, NULL, NULL, plain, (napi_property_attributes)0, NULL}, // 不可枚举
  };
  if (napi_define_properties(env, obj, 3, descs) != napi_ok) return NULL;
  napi_value got; double d = 0;
  napi_get_named_property(env, obj, "val", &got);
  napi_get_value_double(env, got, &d); if (d != 3) ok = 0;
  napi_value mr, r;
  napi_get_named_property(env, obj, "method", &mr);
  // 调 method()（data 指针经 cbinfo 回流）
  napi_value undef; napi_get_undefined(env, &undef);
  if (napi_call_function(env, obj, mr, 0, NULL, &r) != napi_ok) return NULL;
  napi_get_value_double(env, r, &d); if (d != 1) ok = 0;
  // property_names 只含可枚举（val/method，不含 hidden）
  napi_value names;
  napi_get_property_names(env, obj, &names);
  uint32_t alen = 0; napi_get_array_length(env, names, &alen);
  if (alen != 2) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_prototype_and_array_len(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value obj, arr, out, proto;
  napi_create_object(env, &obj);
  napi_get_prototype(env, obj, &proto);
  napi_valuetype t = napi_object;
  napi_typeof(env, proto, &t); if (t != napi_object) ok = 0;
  // 原型上有 Object.prototype 方法
  bool has = false;
  napi_has_named_property(env, proto, "toString", &has); if (!has) ok = 0;
  napi_create_array_with_length(env, 2, &arr);
  uint32_t alen = 0;
  napi_get_array_length(env, arr, &alen); if (alen != 2) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value Init(napi_env env, napi_value exports) {
  struct { const char* name; napi_callback cb; } table[] = {
    {"named", check_named}, {"genericKey", check_generic_key},
    {"defineProperties", check_define_properties},
    {"prototypeAndArrayLen", check_prototype_and_array_len},
  };
  for (unsigned long i = 0; i < sizeof(table)/sizeof(table[0]); i++) {
    napi_value fn;
    if (napi_create_function(env, table[i].name, NAPI_AUTO_LENGTH, table[i].cb, NULL, &fn) != napi_ok) return NULL;
    if (napi_set_named_property(env, exports, table[i].name, fn) != napi_ok) return NULL;
  }
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
