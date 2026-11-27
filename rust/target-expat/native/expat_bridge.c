/* Transitional host backend only. The session, ZIP reader and filter are Rust.
 * Build against frozen Expat, never a system substitute. */
#include <expat_config.h>
#include <expat.h>
#include <stddef.h>
#include <stdint.h>
#if !defined(XML_GE) || XML_GE != 1
#error "Pinned Expat requires XML_GE == 1 for entity amplification limits"
#endif

typedef void (*start_fn)(void *, const char *, const char **);
typedef void (*end_fn)(void *, const char *);
typedef void (*text_fn)(void *, const char *, int);
struct callbacks { void *rust; start_fn start; end_fn end; text_fn text; };
static void XMLCALL bridge_start(void *p, const char *n, const char **a) {
  struct callbacks *c = XML_GetUserData((XML_Parser)p); c->start(c->rust,n,a);
}
static void XMLCALL bridge_end(void *p, const char *n) {
  struct callbacks *c = XML_GetUserData((XML_Parser)p); c->end(c->rust,n);
}
static void XMLCALL bridge_text(void *p, const char *t, int n) {
  struct callbacks *c = XML_GetUserData((XML_Parser)p); c->text(c->rust,t,n);
}
void *newt_xml_create(void) {
  XML_Parser parser = XML_ParserCreate(NULL);
  if (!parser) return NULL;
  /* Small OPFs must not spend the open budget expanding megabytes of entities.
   * Activation is not a document-size cap: ordinary large XML stays valid. */
  if (!XML_SetBillionLaughsAttackProtectionActivationThreshold(parser, 16384)
      || !XML_SetBillionLaughsAttackProtectionMaximumAmplification(parser, 8.0f)) {
    XML_ParserFree(parser);
    return NULL;
  }
  return parser;
}
void newt_xml_close(void *p) { XML_ParserFree((XML_Parser)p); }
int newt_xml_feed(void *p, const char *bytes, int n, int final_chunk,
                  void *context, start_fn start, end_fn end, text_fn text) {
  XML_Parser parser = (XML_Parser)p;
  struct callbacks callbacks = {context,start,end,text};
  XML_UseParserAsHandlerArg(parser);
  XML_SetUserData(parser, &callbacks);
  XML_SetElementHandler(parser, bridge_start, bridge_end);
  XML_SetCharacterDataHandler(parser, bridge_text);
  int ok = XML_Parse(parser, bytes, n, final_chunk) != XML_STATUS_ERROR;
  /* No pointer into a Rust stack frame remains installed after return. */
  XML_SetUserData(parser, NULL);
  XML_SetElementHandler(parser, NULL, NULL);
  XML_SetCharacterDataHandler(parser, NULL);
  return ok;
}

/* Feed installs the Rust context as userdata. DTD callback needs the parser,
 * not that context; recover it via parser-as-handler only for this callback
 * using an independent external-entity handler and doctype trampoline below. */
static void XMLCALL producer_doctype(void *parser, const XML_Char *name,
    const XML_Char *sys, const XML_Char *pub, int subset) {
  (void)name; (void)sys; (void)pub;
  if (subset) XML_StopParser((XML_Parser)parser, XML_FALSE);
}
static int XMLCALL producer_external(XML_Parser p, const XML_Char *a,
    const XML_Char *b, const XML_Char *c, const XML_Char *d) {
  (void)p; (void)a; (void)b; (void)c; (void)d;
  return XML_STATUS_ERROR;
}
void newt_xml_producer(void *p) {
  XML_UseParserAsHandlerArg((XML_Parser)p);
  XML_SetStartDoctypeDeclHandler((XML_Parser)p, producer_doctype);
  XML_SetExternalEntityRefHandler((XML_Parser)p, producer_external);
}
int64_t newt_xml_index(void *p) { return (int64_t)XML_GetCurrentByteIndex((XML_Parser)p); }

/* Per-parser capture feed: no static globals; all pointers live on this stack
 * frame and every handler is cleared before returning. `base` is first so
 * bridge_end / bridge_text can read it cleanly under C11 6.7.2.1p15.
 * Deliberately does NOT install XML_SetXmlDeclHandler: Expat grows its
 * otherwise-unused 1 KiB m_temp2Pool whenever a declaration handler is set,
 * lowering heap-min-free on every UTF-8 chapter; Rust checks the bounded prolog
 * directly without heap allocation. */
struct obs_callbacks {
  struct callbacks base;
  int64_t *root_index;
  uint32_t *cdata_depth;
};
static void XMLCALL obs_start(void *p, const char *n, const char **a) {
  struct obs_callbacks *c = XML_GetUserData((XML_Parser)p);
  if (*c->root_index < 0) *c->root_index = (int64_t)XML_GetCurrentByteIndex((XML_Parser)p);
  c->base.start(c->base.rust, n, a);
}
static void XMLCALL obs_cdata_start(void *p) {
  struct obs_callbacks *c = XML_GetUserData((XML_Parser)p);
  if (*c->cdata_depth < UINT32_MAX) (*c->cdata_depth)++;
}
static void XMLCALL obs_cdata_end(void *p) {
  struct obs_callbacks *c = XML_GetUserData((XML_Parser)p);
  if (*c->cdata_depth > 0) (*c->cdata_depth)--;
}
int newt_xml_feed_observed(void *p, const char *bytes, int n, int final_chunk,
                           void *context, start_fn start, end_fn end, text_fn text,
                           int64_t *root_index, uint32_t *cdata_depth) {
  XML_Parser parser = (XML_Parser)p;
  struct obs_callbacks callbacks = {{context, start, end, text}, root_index, cdata_depth};
  XML_UseParserAsHandlerArg(parser);
  XML_SetUserData(parser, &callbacks);
  XML_SetElementHandler(parser, obs_start, bridge_end);
  XML_SetCharacterDataHandler(parser, bridge_text);
  XML_SetCdataSectionHandler(parser, obs_cdata_start, obs_cdata_end);
  int ok = XML_Parse(parser, bytes, n, final_chunk) != XML_STATUS_ERROR;
  /* No pointer into a Rust stack frame remains installed after return. */
  XML_SetUserData(parser, NULL);
  XML_SetElementHandler(parser, NULL, NULL);
  XML_SetCharacterDataHandler(parser, NULL);
  XML_SetCdataSectionHandler(parser, NULL, NULL);
  return ok;
}

/* Per-call allocator suite: no global allocator mode or inter-test state. */
static void *fail_malloc(size_t n) { (void)n; return NULL; }
static void *fail_realloc(void *p, size_t n) { (void)p; (void)n; return NULL; }
static void fail_free(void *p) { (void)p; }
void *newt_xml_create_fail(void) {
  XML_Memory_Handling_Suite suite = {fail_malloc, fail_realloc, fail_free};
  return XML_ParserCreate_MM(NULL, &suite, NULL);
}
