// napi M2 类与 wrap 面 fixture：define_class（实例方法/getter/setter/static
// 成员）/ new_instance / wrap-unwrap-remove_wrap / external（typeof + 回读）/
// new.target / instanceof / escapable scope / syntax error（node_api_* 族）。
// 探针按位断言，1 = 全过；JS 黑盒只看打印。
#include <node_api.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

// ── wrap 载体：malloc 的 native 记录（finalizer 释放链同款，m2_finalize 有
//    专测；本文件只验指针语义）────────────────────────────────────────────
typedef struct { int32_t age; char name[32]; } person_t;

static int g_last_ctor_age = 0;
static int g_last_new_target_ok = 0;
static int g_fail = 0;

static napi_value PersonCtor(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1];
  napi_value this_v, nt;
  napi_get_cb_info(env, info, &argc, argv, &this_v, NULL);
  napi_get_new_target(env, info, &nt);
  napi_valuetype t; napi_typeof(env, nt, &t);
  // new.target：构造调用必为 function（探针经 newTargetOk 回读）
  g_last_new_target_ok = (t == napi_function);
  if (argc > 0) {
    napi_get_value_int32(env, argv[0], &g_last_ctor_age);
  }
  person_t *p = malloc(sizeof(person_t));
  p->age = g_last_ctor_age;
  snprintf(p->name, sizeof(p->name), "p%d", g_last_ctor_age);
  // this 上 wrap（this 由宿主 trampoline 自建并经 cbinfo 传入）
  if (napi_wrap(env, this_v, p, NULL, NULL, NULL) != napi_ok) { free(p); return NULL; }
  return NULL; // 回落自建 this
}

static napi_value PersonGetAge(napi_env env, napi_callback_info info) {
  napi_value this_v;
  napi_get_cb_info(env, info, NULL, NULL, &this_v, NULL);
  person_t *p = NULL;
  if (napi_unwrap(env, this_v, (void **)&p) != napi_ok || !p) return NULL;
  napi_value out;
  napi_create_int32(env, p->age, &out);
  return out;
}

static napi_value PersonGetName(napi_env env, napi_callback_info info) {
  napi_value this_v;
  napi_get_cb_info(env, info, NULL, NULL, &this_v, NULL);
  person_t *p = NULL;
  if (napi_unwrap(env, this_v, (void **)&p) != napi_ok || !p) return NULL;
  napi_value out;
  napi_create_string_utf8(env, p->name, NAPI_AUTO_LENGTH, &out);
  return out;
}

static napi_value PersonSetName(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], this_v;
  napi_get_cb_info(env, info, &argc, argv, &this_v, NULL);
  person_t *p = NULL;
  if (napi_unwrap(env, this_v, (void **)&p) != napi_ok || !p || argc < 1) return NULL;
  char buf[32] = {0};
  size_t n = 0;
  napi_get_value_string_utf8(env, argv[0], buf, sizeof(buf) - 1, &n);
  memcpy(p->name, buf, sizeof(p->name));
  return NULL;
}

// 非 new 调用：new.target 应为 undefined
static napi_value ProbePlainCall(napi_env env, napi_callback_info info) {
  napi_value nt;
  napi_get_new_target(env, info, &nt);
  napi_valuetype t; napi_typeof(env, nt, &t);
  napi_value out;
  napi_create_int32(env, t == napi_undefined, &out);
  return out;
}

static napi_value ProbeNewTargetOk(napi_env env, napi_callback_info info) {
  napi_value out;
  napi_create_int32(env, g_last_new_target_ok, &out);
  return out;
}

static napi_value ProbeLastAge(napi_env env, napi_callback_info info) {
  napi_value out;
  napi_create_int32(env, g_last_ctor_age, &out);
  return out;
}

// ── 自包含 C 矩阵：new_instance/wrap 回环/external/scope/syntax ─────────
static napi_value g_person_ctor = NULL;

static napi_value CheckNewInstance(napi_env env, napi_callback_info info) {
  int ok = 1;
  g_fail = 0;
  napi_value argv1[1], inst, out;
  napi_create_int32(env, 7, &argv1[0]);
  if (napi_new_instance(env, g_person_ctor, 1, argv1, &inst) != napi_ok) return NULL;
  // instanceof（M1 面）+ 经实例方法读回 native
  bool is = false;
  napi_instanceof(env, inst, g_person_ctor, &is); if (!is) { g_fail |= 1; ok = 0; }
  napi_value getage;
  napi_get_named_property(env, inst, "getAge", &getage);
  napi_value r;
  napi_call_function(env, inst, getage, 0, NULL, &r);
  double d = 0; napi_get_value_double(env, r, &d); if (d != 7) { g_fail |= 2; ok = 0; }
  // wrap/unwrap/remove_wrap 回环（C 侧构造实例）
  person_t *w = malloc(sizeof(person_t)); w->age = 99;
  if (napi_wrap(env, inst, w, NULL, NULL, NULL) == napi_ok) { g_fail |= 3; ok = 0; } // 已 wrap → 必须拒
  free(w);
  person_t *got = NULL;
  if (napi_unwrap(env, inst, (void **)&got) != napi_ok || !got || got->age != 7) { g_fail |= 4; ok = 0; }
  napi_value unw;
  napi_remove_wrap(env, inst, (void **)&got);
  if (napi_unwrap(env, inst, (void **)&got) == napi_ok) { g_fail |= 5; ok = 0; } // 摘除后不可再解
  if (got) free(got);
  // 非 define_class 实例对象 wrap 必须 fail-fast
  napi_value plain;
  napi_create_object(env, &plain);
  if (napi_wrap(env, plain, NULL, NULL, NULL, NULL) == napi_ok) { g_fail |= 6; ok = 0; }
  napi_create_int32(env, ok, &out);
  return out;
}

static char g_ext_tag[8] = "ext";
static napi_value CheckExternal(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value ext, out;
  if (napi_create_external(env, g_ext_tag, NULL, NULL, &ext) != napi_ok) return NULL;
  napi_valuetype t;
  napi_typeof(env, ext, &t); if (t != napi_external) ok = 0;
  char *got = NULL;
  if (napi_get_value_external(env, ext, (void **)&got) != napi_ok) ok = 0;
  if (!got || strcmp(got, "ext") != 0) ok = 0;
  napi_value plain;
  napi_create_object(env, &plain);
  if (napi_get_value_external(env, plain, (void **)&got) == napi_ok) ok = 0;
  napi_typeof(env, plain, &t); if (t == napi_external) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}

static napi_value CheckEscapableScope(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_handle_scope hs;
  napi_escapable_handle_scope ehs;
  napi_value v, esc, out;
  if (napi_open_handle_scope(env, &hs) != napi_ok) return NULL;
  if (napi_open_escapable_handle_scope(env, &ehs) != napi_ok) ok = 0;
  napi_create_string_utf8(env, "escaped!", NAPI_AUTO_LENGTH, &v);
  if (napi_escape_handle(env, ehs, v, &esc) != napi_ok) ok = 0;
  if (napi_escape_handle(env, ehs, v, &esc) != napi_escape_called_twice) ok = 0;
  if (napi_close_escapable_handle_scope(env, ehs) != napi_ok) ok = 0;
  // 错序/重复 close → generic_failure（宿主可读校验，Node 为 assert/UB）
  if (napi_close_escapable_handle_scope(env, ehs) != napi_generic_failure) ok = 0;
  napi_value cpy;
  napi_create_int32(env, 1, &cpy); // escape 后本 scope 内照常建值
  if (napi_close_handle_scope(env, hs) != napi_ok) ok = 0;
  // escapee 已迁独立池：scope 关闭后仍可读
  char buf[16] = {0};
  napi_get_value_string_utf8(env, esc, buf, sizeof(buf) - 1, NULL);
  if (strcmp(buf, "escaped!") != 0) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}

static napi_value MakeSyntaxError(napi_env env, napi_callback_info info) {
  napi_value code, msg;
  napi_create_string_utf8(env, "ERR_WJS_SYNTAX", NAPI_AUTO_LENGTH, &code);
  napi_create_string_utf8(env, "bad syntax", NAPI_AUTO_LENGTH, &msg);
  napi_value err;
  if (node_api_create_syntax_error(env, code, msg, &err) != napi_ok) return NULL;
  return err;
}

static napi_value ThrowSyntaxError(napi_env env, napi_callback_info info) {
  node_api_throw_syntax_error(env, "ERR_WJS_THROW", "thrown syntax");
  return NULL;
}

static napi_value CheckSyntaxErrorShape(napi_env env, napi_callback_info info) {
  int ok = 1;
  napi_value err = MakeSyntaxError(env, info), out;
  bool is = false;
  napi_value name_v;
  napi_get_named_property(env, err, "name", &name_v);
  char buf[32] = {0};
  napi_get_value_string_utf8(env, name_v, buf, sizeof(buf) - 1, NULL);
  if (strcmp(buf, "SyntaxError") != 0) ok = 0;
  napi_is_error(env, err, &is); if (!is) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}

static napi_value CheckClassShape(napi_env env, napi_callback_info info) {
  // static 成员（prop/method）在构造器函数对象上，实例无
  int ok = 1;
  napi_value out, r;
  napi_get_named_property(env, g_person_ctor, "kind", &r);
  napi_valuetype t; napi_typeof(env, r, &t); if (t != napi_string) ok = 0;
  napi_get_named_property(env, g_person_ctor, "make", &r);
  napi_typeof(env, r, &t); if (t != napi_function) ok = 0;
  napi_create_int32(env, ok, &out);
  return out;
}

static napi_value PersonStaticMake(napi_env env, napi_callback_info info) {
  size_t argc = 1;
  napi_value argv[1], inst;
  napi_get_cb_info(env, info, &argc, argv, NULL, NULL);
  if (napi_new_instance(env, g_person_ctor, argc, argv, &inst) != napi_ok) return NULL;
  return inst;
}

static napi_value Init(napi_env env, napi_value exports) {
  napi_property_descriptor person_props[] = {
    { "getAge", NULL, PersonGetAge, NULL, NULL, NULL,
      (napi_property_attributes)(napi_writable | napi_enumerable | napi_configurable), NULL },
    { "name", NULL, NULL, PersonGetName, PersonSetName, NULL,
      (napi_property_attributes)(napi_enumerable | napi_configurable), NULL },
  };
  // static 成员走 define_properties 挂构造器函数对象（napi_static 位语义）
  napi_value ctor;
  if (napi_define_class(env, "Person", NAPI_AUTO_LENGTH, PersonCtor, NULL, 2, person_props, &ctor) != napi_ok) return NULL;
  g_person_ctor = ctor;
  napi_value kind_v;
  napi_create_string_utf8(env, "human", NAPI_AUTO_LENGTH, &kind_v);
  napi_property_descriptor static_extra[] = {
    { "kind", NULL, NULL, NULL, NULL, kind_v, (napi_property_attributes)(napi_static | napi_enumerable), NULL },
    { "make", NULL, PersonStaticMake, NULL, NULL, NULL,
      (napi_property_attributes)(napi_static | napi_writable | napi_enumerable | napi_configurable), NULL },
    { "plainCall", NULL, ProbePlainCall, NULL, NULL, NULL,
      (napi_property_attributes)(napi_writable | napi_enumerable | napi_configurable), NULL },
    { "newTargetOk", NULL, ProbeNewTargetOk, NULL, NULL, NULL,
      (napi_property_attributes)(napi_writable | napi_enumerable | napi_configurable), NULL },
    { "lastAge", NULL, ProbeLastAge, NULL, NULL, NULL,
      (napi_property_attributes)(napi_writable | napi_enumerable | napi_configurable), NULL },
    { "shape", NULL, CheckClassShape, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
    { "newInstance", NULL, CheckNewInstance, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
    { "external", NULL, CheckExternal, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
    { "escapable", NULL, CheckEscapableScope, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
    { "syntaxShape", NULL, CheckSyntaxErrorShape, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
    { "makeSyntax", NULL, MakeSyntaxError, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
    { "throwSyntax", NULL, ThrowSyntaxError, NULL, NULL, NULL, (napi_property_attributes)0, NULL },
  };
  if (napi_define_properties(env, ctor, 12, static_extra) != napi_ok) return NULL;
  napi_set_named_property(env, exports, "Person", ctor);
  return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME, Init)
