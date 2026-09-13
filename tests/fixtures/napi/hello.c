// napi M0 验收 fixture：NAPI_MODULE 宏路径（符号注册 napi_register_module_v1）。
// hello() → 42；add(a,b) → a+b（cbinfo 实参面）；echo(str) → str（字符串读面 M1 补）。
#include <node_api.h>

static napi_value Hello(napi_env env, napi_callback_info info) {
  (void)env; (void)info;
  napi_value out;
  if (napi_create_int32(env, 42, &out) != napi_ok) return NULL;
  return out;
}

static napi_value Add(napi_env env, napi_callback_info info) {
  size_t argc = 2;
  napi_value argv[2];
  if (napi_get_cb_info(env, info, &argc, argv, NULL, NULL) != napi_ok) return NULL;
  double a = 0, b = 0;
  if (argc == 2 &&
      napi_get_value_double(env, argv[0], &a) == napi_ok &&
      napi_get_value_double(env, argv[1], &b) == napi_ok) {
    napi_value out;
    if (napi_create_double(env, a + b, &out) == napi_ok) return out;
  }
  napi_throw_error(env, NULL, "add needs two numbers");
  return NULL;
}

static napi_value Init(napi_env env, napi_value exports) {
  napi_value hello, add, version;
  napi_create_function(env, "hello", NAPI_AUTO_LENGTH, Hello, NULL, &hello);
  napi_create_function(env, "add", NAPI_AUTO_LENGTH, Add, NULL, &add);
  napi_create_string_utf8(env, "m0-ok", NAPI_AUTO_LENGTH, &version);
  napi_set_named_property(env, exports, "hello", hello);
  napi_set_named_property(env, exports, "add", add);
  napi_set_named_property(env, exports, "version", version);
  return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
