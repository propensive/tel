#include "tree_sitter/parser.h"
#include <napi.h>

extern "C" TSLanguage *tree_sitter_tel();

Napi::Object Init(Napi::Env env, Napi::Object exports) {
  exports["name"] = Napi::String::New(env, "tel");
  auto language = Napi::External<TSLanguage>::New(env, tree_sitter_tel());
  language.TypeTag(reinterpret_cast<const napi_type_tag *>("TSLanguage"));
  exports["language"] = language;
  return exports;
}

NODE_API_MODULE(tree_sitter_tel_binding, Init)
