/* Transitional host backend only. The session, ZIP reader and filter are Rust.
 * Build against frozen Expat, never a system substitute. */
#include <expat.h>
#include <stddef.h>
#include <stdint.h>

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
void *newt_xml_create(void) { return XML_ParserCreate(NULL); }
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

/* Per-call allocator suite: no global allocator mode or inter-test state. */
static void *fail_malloc(size_t n) { (void)n; return NULL; }
static void *fail_realloc(void *p, size_t n) { (void)p; (void)n; return NULL; }
static void fail_free(void *p) { (void)p; }
void *newt_xml_create_fail(void) {
  XML_Memory_Handling_Suite suite = {fail_malloc, fail_realloc, fail_free};
  return XML_ParserCreate_MM(NULL, &suite, NULL);
}
