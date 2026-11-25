/* Generated XML and the actual shared bridge; no system Expat or book text. */
#include <expat_config.h>
#include <expat.h>
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

void *newt_xml_create(void);
void newt_xml_close(void *);
void newt_xml_producer(void *);
int newt_xml_feed(void *, const char *, int, int, void *,
                 void (*)(void *,const char *,const char **),
                 void (*)(void *,const char *),void (*)(void *,const char *,int));
XML_Bool __real_XML_SetBillionLaughsAttackProtectionActivationThreshold(XML_Parser,unsigned long long);
XML_Bool __real_XML_SetBillionLaughsAttackProtectionMaximumAmplification(XML_Parser,float);
void __real_XML_ParserFree(XML_Parser);
static int fail_setting, frees;
XML_Bool __wrap_XML_SetBillionLaughsAttackProtectionActivationThreshold(XML_Parser p,unsigned long long n) {
  return fail_setting == 1 ? XML_FALSE : __real_XML_SetBillionLaughsAttackProtectionActivationThreshold(p,n);
}
XML_Bool __wrap_XML_SetBillionLaughsAttackProtectionMaximumAmplification(XML_Parser p,float n) {
  return fail_setting == 2 ? XML_FALSE : __real_XML_SetBillionLaughsAttackProtectionMaximumAmplification(p,n);
}
void __wrap_XML_ParserFree(XML_Parser p) { ++frees; __real_XML_ParserFree(p); }

struct bytes { char *data; size_t size, capacity; };
static void append(struct bytes *b,const void *data,size_t size) {
  if (b->size+size+1>b->capacity) {
    b->capacity=2*(b->size+size+1); b->data=realloc(b->data,b->capacity); assert(b->data);
  }
  memcpy(b->data+b->size,data,size); b->size+=size; b->data[b->size]=0;
}
static void put(struct bytes *b,const char *text) { append(b,text,strlen(text)); }
struct sink { struct bytes events, captured; size_t text, attr_bytes, feeds; int trace, capture; };
static void event(struct sink *s,char tag,const void *data,size_t n) {
  if (!s->trace) return;
  append(&s->events,&tag,1); append(&s->events,&n,sizeof(n)); append(&s->events,data,n);
}
static void start(void *ctx,const char *name,const char **attrs) {
  struct sink *s=ctx; event(s,'S',name,strlen(name));
  for (; *attrs; attrs+=2) {
    size_t vlen=strlen(attrs[1]);
    s->attr_bytes+=vlen;
    event(s,'A',attrs[0],strlen(attrs[0])); event(s,'V',attrs[1],vlen);
  }
}
static void end(void *ctx,const char *name) { event(ctx,'E',name,strlen(name)); }
static void text(void *ctx,const char *data,int n) {
  struct sink *s=ctx; s->text+=(size_t)n;
  if (s->capture) append(&s->captured,data,(size_t)n);
  event(s,'T',data,(size_t)n);
}
static int parse(void *parser,const struct bytes *xml,size_t chunk,struct sink *s) {
  assert(parser); int ok=1;
  for (size_t at=0; ok && at<xml->size; at+=chunk) {
    size_t n=xml->size-at; if (n>chunk) n=chunk;
    ++s->feeds;
    ok=newt_xml_feed(parser,xml->data+at,(int)n,0,s,start,end,text);
  }
  if (ok) { ++s->feeds; ok=newt_xml_feed(parser,"",0,1,s,start,end,text); }
  newt_xml_close(parser); return ok;
}
static struct bytes expansion_n(size_t pad_bytes,int nested,int attribute,int refs) {
  struct bytes b={0};
  if (pad_bytes) { put(&b,"<!--"); for (size_t i=0;i<pad_bytes;++i) put(&b,"x"); put(&b,"-->"); }
  put(&b,"<!DOCTYPE package [<!ENTITY a '");
  for (int i=0;i<1024;++i) put(&b,"x");
  put(&b,"'>");
  if (nested) { put(&b,"<!ENTITY b '"); for (int i=0;i<32;++i) put(&b,"&a;"); put(&b,"'>"); }
  put(&b,attribute ? "]><package value='" : "]><package>");
  for (int i=0;i<refs;++i) put(&b,nested ? "&b;" : "&a;");
  put(&b,attribute ? "'/>" : "</package>"); return b;
}
static struct bytes expansion(int padding,int nested,int attribute) {
  return expansion_n(padding ? 32768 : 0,nested,attribute,nested ? 32 : 1024);
}
static struct bytes utf16le(const struct bytes *ascii) {
  struct bytes out={0}; const unsigned char bom[]={255,254}; append(&out,bom,2);
  for (size_t i=0;i<ascii->size;++i) { append(&out,ascii->data+i,1); append(&out,"",1); }
  return out;
}
static void bounded(int factor) {
  for (int nested=0;nested<2;++nested) for (int attribute=0;attribute<2;++attribute) {
    struct bytes xml=expansion(factor,nested,attribute);
    for (size_t chunk=1;chunk<=2048;chunk*=2048) {
      struct sink s={0};
      int ok=parse(newt_xml_create(),&xml,chunk,&s);
      printf("EXPAND %d %d %zu %d %zu %zu %zu %zu\n",
             nested,attribute,chunk,ok,s.text,s.attr_bytes,s.feeds,xml.size);
    }
    free(xml.data);
  }
  if (factor) {
    /* Just above 8.0x past the 16 KiB activation threshold (~9.1x, well below 100x default). */
    for (int attribute=0;attribute<2;++attribute) {
      struct bytes xml=expansion_n(16384,0,attribute,150);
      for (size_t chunk=1;chunk<=2048;chunk*=2048) {
        struct sink s={0};
        int ok=parse(newt_xml_create(),&xml,chunk,&s);
        printf("BOUNDARY %d %zu %d %zu %zu %zu %zu\n",
               attribute,chunk,ok,s.text,s.attr_bytes,s.feeds,xml.size);
      }
      free(xml.data);
    }
  }
}
static void check_identical_to_main(const struct bytes *xml) {
  for (size_t chunk=1;chunk<=2048;chunk*=2048) {
    struct sink current={.trace=1},main={.trace=1};
    assert(parse(newt_xml_create(),xml,chunk,&current));
    /* Main's constructor is exactly XML_ParserCreate(NULL); same feed bridge. */
    assert(parse(XML_ParserCreate(NULL),xml,chunk,&main));
    assert(current.events.size==main.events.size);
    assert(!memcmp(current.events.data,main.events.data,current.events.size));
    free(current.events.data); free(main.events.data);
  }
}
static void ordinary(void) {
  struct bytes xml={0}; put(&xml,"<html><body>");
  for (int i=0;i<2048;++i) put(&xml,"<p lang='en' title='A &amp; B'>Ordinary prose &lt; &gt; &quot; &apos; &#169; &#x2014;.</p>");
  put(&xml,"</body></html>"); assert(xml.size>128*1024);
  check_identical_to_main(&xml);
  printf("PASS ordinary chapter: %zu bytes, SAX trace byte-identical to main\n",xml.size);
  free(xml.data);

  /* Plain OPF larger than the 16 KiB activation threshold stays valid and unchanged. */
  struct bytes opf={0}; put(&opf,"<package><metadata><dc:title>A &amp; B &#65;</dc:title></metadata><manifest>");
  for (int i=0;i<320;++i) put(&opf,"<item id='c0' href='chapter-ordinary-name.xhtml' media-type='application/xhtml+xml'/>");
  put(&opf,"</manifest><spine><itemref idref='c0'/></spine></package>");
  assert(opf.size>20*1024);
  check_identical_to_main(&opf);
  free(opf.data);

  /* Tiny benign internal entity and external-only DOCTYPE (no external fetch) keep working. */
  const struct { const char *xml; const char *want; } accepted[]={
    {"<!DOCTYPE package [<!ENTITY a 'ok'>]><package>&a;</package>","ok"},
    {"<!DOCTYPE package SYSTEM 'urn:unfetched'><package>&amp;&#65;</package>","&A"},
  };
  for (size_t i=0;i<sizeof(accepted)/sizeof(*accepted);++i) {
    struct bytes b={0}; put(&b,accepted[i].xml);
    struct sink s={.capture=1};
    assert(parse(newt_xml_create(),&b,1,&s));
    assert(s.captured.size==strlen(accepted[i].want) && !strcmp(s.captured.data,accepted[i].want));
    free(s.captured.data); free(b.data);
  }

  /* Benign UTF-16LE (with BOM) still parses; UTF-16LE entity bomb refuses promptly. */
  struct bytes utf16_src={0};
  put(&utf16_src,"<!DOCTYPE package [<!ENTITY a 'ok'>]><package>&a;&amp;&#65;</package>");
  struct bytes utf16_ok=utf16le(&utf16_src);
  struct sink ok_sink={.capture=1};
  assert(parse(newt_xml_create(),&utf16_ok,1,&ok_sink));
  assert(ok_sink.captured.size==4 && !strcmp(ok_sink.captured.data,"ok&A"));
  free(ok_sink.captured.data); free(utf16_ok.data); free(utf16_src.data);

  struct bytes bomb=expansion(0,1,0);
  struct bytes utf16_bomb=utf16le(&bomb);
  for (size_t chunk=1;chunk<=2048;chunk*=2048) {
    struct sink bomb_sink={0};
    assert(!parse(newt_xml_create(),&utf16_bomb,chunk,&bomb_sink));
    /* UTF-16LE direct bytes are 2x ASCII while stored UTF-8 entity bytes are 1x,
     * so 8x direct stops well below 32 KiB (vs 1 MiB unblocked) in bounded feeds. */
    assert(bomb_sink.text<32768 && bomb_sink.feeds<=(utf16_bomb.size+chunk-1)/chunk+1);
  }
  free(utf16_bomb.data); free(bomb.data);

  /* Just below 8.0x past the 16 KiB activation threshold (~7.0x-7.3x, >130 KiB total) stays valid. */
  for (int attribute=0;attribute<2;++attribute) {
    struct bytes below=expansion_n(16384,0,attribute,110);
    for (size_t chunk=1;chunk<=2048;chunk*=2048) {
      struct sink s={0};
      assert(parse(newt_xml_create(),&below,chunk,&s));
      if (attribute) assert(s.attr_bytes==110*1024 && s.text==0);
      else assert(s.text==110*1024 && s.attr_bytes==0);
    }
    free(below.data);
  }

  /* Producer internal-subset stop remains unchanged. */
  struct bytes dtd={0}; put(&dtd,accepted[0].xml);
  struct sink producer={0};
  void *p=newt_xml_create(); newt_xml_producer(p);
  assert(!parse(p,&dtd,1,&producer));
  free(dtd.data);
}
static void failed_setting(int setting) {
  fail_setting=setting; int before=frees; void *p=newt_xml_create();
  printf("SETUP %d %d\n",p==NULL,frees-before); if (p) newt_xml_close(p);
  fail_setting=0;
}
int main(int argc,char **argv) {
  assert(argc==2);
  if (!strcmp(argv[1],"threshold")) bounded(0);
  else if (!strcmp(argv[1],"factor")) bounded(1);
  else if (!strcmp(argv[1],"ordinary")) ordinary();
  else if (!strcmp(argv[1],"fail-threshold")) failed_setting(1);
  else if (!strcmp(argv[1],"fail-factor")) failed_setting(2);
  else abort();
  puts("PASS entity limits"); return 0;
}
