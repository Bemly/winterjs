// napi M1 值系统 fixture：建/读 roundtrip + coerce + typeof + instanceof +
// strict_equals + is_array/is_error + 错误对象族 + pending 异常面。
// 每个 check_* 返回 1/0，JS 侧分参打印断言（AGENTS §4.42）。
#include <node_api.h>
#include <stdio.h>
#include <string.h>

static napi_value check_num_roundtrip(napi_env env, napi_callback_info info) {
  napi_value v, out; double d = 0;
  if (napi_create_double(env, 3.5, &v) != napi_ok) return NULL;
  if (napi_get_value_double(env, v, &d) != napi_ok || d != 3.5) return NULL;
  if (napi_create_int32(env, 1, &out) != napi_ok) return NULL;
  return out;
}
static napi_value check_int32(napi_env env, napi_callback_info info) {
  napi_value v, out; int32_t i = 0;
  if (napi_create_int32(env, -7, &v) != napi_ok) return NULL;
  if (napi_get_value_int32(env, v, &i) != napi_ok || i != -7) return NULL;
  napi_create_int32(env, 1, &out);
  return out;
}
static napi_value check_uint32(napi_env env, napi_callback_info info) {
  napi_value v, out; uint32_t u = 0;
  if (napi_create_uint32(env, 4294967295u, &v) != napi_ok) return NULL;
  if (napi_get_value_uint32(env, v, &u) != napi_ok || u != 4294967295u) return NULL;
  napi_create_int32(env, 1, &out);
  return out;
}
static napi_value check_int64(napi_env env, napi_callback_info info) {
  napi_value v, out; int64_t i = 0;
  if (napi_create_int64(env, 4503599627370496LL, &v) != napi_ok) return NULL;
  if (napi_get_value_int64(env, v, &i) != napi_ok || i != 4503599627370496LL) return NULL;
  napi_create_int32(env, 1, &out);
  return out;
}
static napi_value check_bool_null_undef(napi_env env, napi_callback_info info) {
  napi_value b, n, u, out; int ok = 1; bool bb = false;
  napi_get_boolean(env, true, &b);
  if (napi_get_value_bool(env, b, &bb) != napi_ok || !bb) ok = 0;
  napi_get_null(env, &n); napi_get_undefined(env, &u);
  napi_valuetype t = napi_object;
  napi_typeof(env, n, &t); if (t != napi_null) ok = 0;
  napi_typeof(env, u, &t); if (t != napi_undefined) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_string_roundtrip(napi_env env, napi_callback_info info) {
  napi_value s, out; char buf[32] = {0}; size_t len = 0;
  if (napi_create_string_utf8(env, "hello", NAPI_AUTO_LENGTH, &s) != napi_ok) return NULL;
  if (napi_get_value_string_utf8(env, s, buf, sizeof(buf), &len) != napi_ok) return NULL;
  int ok = strcmp(buf, "hello") == 0 && len == 5;
  // 查询长度（buf=NULL）
  if (napi_get_value_string_utf8(env, s, NULL, 0, &len) != napi_ok || len != 5) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_utf16_surrogate(napi_env env, napi_callback_info info) {
  // U+1F600 = D83D DE00（孤立代理也按原样走 WTF-16）
  const uint16_t units[2] = {0xD83D, 0xDE00};
  napi_value s, out; uint16_t buf[4] = {0}; size_t len = 0;
  if (napi_create_string_utf16(env, units, 2, &s) != napi_ok) return NULL;
  if (napi_get_value_string_utf16(env, s, buf, 4, &len) != napi_ok) return NULL;
  int ok = len == 2 && buf[0] == 0xD83D && buf[1] == 0xDE00;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_latin1(napi_env env, napi_callback_info info) {
  const char bytes[2] = {(char)0xE9, 0};
  napi_value s, out; char buf[4] = {0}; size_t len = 0;
  if (napi_create_string_latin1(env, bytes, 1, &s) != napi_ok) return NULL;
  if (napi_get_value_string_latin1(env, s, buf, sizeof(buf), &len) != napi_ok) return NULL;
  int ok = len == 1 && (unsigned char)buf[0] == 0xE9;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_symbol(napi_env env, napi_callback_info info) {
  napi_value desc, sym, out; napi_valuetype t = napi_object;
  napi_create_string_utf8(env, "tag", NAPI_AUTO_LENGTH, &desc);
  if (napi_create_symbol(env, desc, &sym) != napi_ok) return NULL;
  napi_typeof(env, sym, &t);
  napi_create_int32(env, t == napi_symbol, &out);
  return out;
}
static napi_value check_array(napi_env env, napi_callback_info info) {
  (void)info;
  napi_value arr, e0, out; int mask = 0; bool is = false;
  napi_create_array_with_length(env, 3, &arr);
  napi_create_int32(env, 9, &e0);
  if (napi_set_element(env, arr, 1, e0) == napi_ok) mask |= 1;
  napi_value got;
  napi_get_element(env, arr, 1, &got);
  double d = 0; napi_get_value_double(env, got, &d);
  if (d == 9) mask |= 2;
  bool has = false; napi_has_element(env, arr, 1, &has);
  if (has) mask |= 4;
  uint32_t alen = 0; napi_get_array_length(env, arr, &alen);
  if (alen == 3) mask |= 8;
  napi_is_array(env, arr, &is);
  if (is) mask |= 16;
  napi_delete_element(env, arr, 1, &has);
  if (!has) mask |= 32;
  napi_create_int32(env, mask, &out);
  return out;
}
static napi_value check_typeof_and_equals(napi_env env, napi_callback_info info) {
  int ok = 1; napi_valuetype t = napi_object;
  napi_value one, one2, str1, obj, out;
  napi_create_int32(env, 1, &one); napi_create_int32(env, 1, &one2);
  napi_create_string_utf8(env, "1", NAPI_AUTO_LENGTH, &str1);
  napi_create_object(env, &obj);
  napi_typeof(env, one, &t); if (t != napi_number) ok = 0;
  napi_typeof(env, str1, &t); if (t != napi_string) ok = 0;
  napi_typeof(env, obj, &t); if (t != napi_object) ok = 0;
  bool eq = false;
  napi_strict_equals(env, one, one2, &eq); if (!eq) ok = 0;
  napi_strict_equals(env, one, str1, &eq); if (eq) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_error_family(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value msg, err, out; bool is = false;
  napi_create_string_utf8(env, "boom", NAPI_AUTO_LENGTH, &msg);
  if (napi_create_error(env, NULL, msg, &err) != napi_ok) return NULL;
  napi_is_error(env, err, &is); if (!is) ok = 0;
  napi_value plain; napi_create_object(env, &plain);
  napi_is_error(env, plain, &is); if (is) ok = 0;
  // instanceof：JS 侧传 Error 实例 + Error 构造器（第二个实参）
  size_t argc = 2; napi_value argv[2];
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  napi_instanceof(env, argv[0], argv[1], &is);
  if (!is) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_is_error_js_instance(napi_env env, napi_callback_info info) {
  size_t argc = 1; napi_value argv[1]; bool is = false;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  napi_is_error(env, argv[0], &is);
  napi_value out;
  napi_create_int32(env, is, &out);
  return out;
}
static napi_value check_coerce(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value s42, empty, obj, out;
  napi_create_string_utf8(env, "42", NAPI_AUTO_LENGTH, &s42);
  napi_create_string_utf8(env, "", NAPI_AUTO_LENGTH, &empty);
  napi_create_object(env, &obj);
  napi_value num; double d = 0;
  napi_coerce_to_number(env, s42, &num);
  napi_get_value_double(env, num, &d); if (d != 42) ok = 0;
  napi_value sb; bool b = true;
  napi_coerce_to_bool(env, empty, &sb);
  napi_get_value_bool(env, sb, &b);
  if (b) ok = 0;
  napi_value so; char buf[8] = {0}; size_t len = 0;
  napi_value n42; napi_create_double(env, 42, &n42);
  napi_coerce_to_string(env, n42, &so);
  napi_get_value_string_utf8(env, so, buf, sizeof(buf), &len);
  if (strcmp(buf, "42") != 0) ok = 0;
  napi_value co; napi_coerce_to_object(env, obj, &co);
  if (co == NULL) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}
static napi_value check_pending_exception(napi_env env, napi_callback_info info) {
  int mask = 0;
  napi_value out; bool pending = false;
  napi_throw_error(env, NULL, "clear me");
  napi_is_exception_pending(env, &pending); if (pending) mask |= 1;
  napi_value exc;
  napi_get_and_clear_last_exception(env, &exc);
  napi_is_exception_pending(env, &pending); if (!pending) mask |= 2;
  // exc 是 Error 对象：读 .message 验证（get_and_clear 回的是抛出的值本身）
  napi_value m;
  napi_get_named_property(env, exc, "message", &m);
  char buf[16] = {0}; size_t len = 0;
  napi_get_value_string_utf8(env, m, buf, sizeof(buf), &len);
  if (strcmp(buf, "clear me") == 0) mask |= 4;
  napi_create_int32(env, mask, &out);
  return out;
}
static napi_value Init(napi_env env, napi_value exports) {
  struct { const char* name; napi_callback cb; } table[] = {
    {"numRoundtrip", check_num_roundtrip}, {"int32", check_int32},
    {"uint32", check_uint32}, {"int64", check_int64},
    {"boolNullUndef", check_bool_null_undef}, {"stringRoundtrip", check_string_roundtrip},
    {"utf16Surrogate", check_utf16_surrogate}, {"latin1", check_latin1},
    {"symbol", check_symbol}, {"array", check_array},
    {"typeofAndEquals", check_typeof_and_equals}, {"errorFamily", check_error_family},
    {"isErrorJsInstance", check_is_error_js_instance}, {"coerce", check_coerce},
    {"pendingException", check_pending_exception},
  };
  for (unsigned long i = 0; i < sizeof(table)/sizeof(table[0]); i++) {
    napi_value fn;
    if (napi_create_function(env, table[i].name, NAPI_AUTO_LENGTH, table[i].cb, NULL, &fn) != napi_ok) return NULL;
    if (napi_set_named_property(env, exports, table[i].name, fn) != napi_ok) return NULL;
  }
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
