/* MIT. Primitive FreeType backend; wrapper decisions live in ft_font.rs. */
#include "native.h"
#include <ft2build.h>
#include FT_FREETYPE_H
#include FT_MODULE_H
#include FT_MULTIPLE_MASTERS_H
#include FT_OUTLINE_H
#include FT_TRUETYPE_TABLES_H
#include <stdlib.h>
#include <string.h>
#include <limits.h>
#include <stddef.h>
/* Selected layout facts, not calling-convention or final-link acceptance. */
_Static_assert(sizeof(nf_memory)==4*sizeof(void*)&&_Alignof(nf_memory)==_Alignof(void*),"nf memory layout");
_Static_assert(offsetof(nf_memory,context)==0&&offsetof(nf_memory,allocate)==sizeof(void*)&&offsetof(nf_memory,deallocate)==2*sizeof(void*)&&offsetof(nf_memory,reallocate)==3*sizeof(void*),"nf memory offsets");
_Static_assert(_Alignof(nf_axis)==4&&_Alignof(nf_lines)==4&&_Alignof(nf_slot)==8,"nf record alignment");
#if defined(__riscv) && __riscv_xlen==32
_Static_assert(sizeof(void*)==4&&sizeof(unsigned long)==4,"RV32 widths");
#endif
_Static_assert(sizeof(nf_axis)==16&&sizeof(nf_lines)==12&&sizeof(nf_slot)==88,"nf ABI");
_Static_assert(offsetof(nf_slot,linear_advance)==8&&offsetof(nf_slot,width)==40&&offsetof(nf_slot,xmin)==56,"nf ABI");
#if defined(ESP_PLATFORM)
#include <sdkconfig.h>
#include "FontAlloc.h"
#if defined(CONFIG_HEAP_ABORT_WHEN_ALLOCATION_FAILS) && CONFIG_HEAP_ABORT_WHEN_ALLOCATION_FAILS
#error "FreeType reports allocation failure; keep the non-aborting heap policy"
#endif
/* FtFont's own default (pinned FontAlloc.c). Static and context-free: no
 * buffer, no state. Null callbacks are rejected on target, never libc. */
static void *font_alloc(void*c,size_t n){(void)c;return fiFontMalloc(n);}
static void font_free(void*c,void*p){(void)c;fiFontFree(p);}
static void *font_realloc(void*c,void*p,size_t old,size_t n){(void)c;(void)old;return fiFontRealloc(p,n);}
static const nf_memory font_heap={NULL,font_alloc,font_free,font_realloc};
const nf_memory *nf_font_heap(void){return &font_heap;}
#define NF_DEFAULT_MEMORY {NULL,NULL,NULL,NULL}
#elif defined(__riscv) || defined(__XTENSA__)
#error "Target builds define ESP_PLATFORM; the libc default is host-only"
#else
#define NF_DEFAULT_MEMORY {NULL,host_alloc,host_free,host_realloc}
#endif
struct nf_library {nf_memory memory;struct FT_MemoryRec_ ft_memory;FT_Library library;};
struct nf_face {nf_library *owner;FT_Face face;FT_StreamRec stream;nf_read read;void *context;int failed;};
#if !defined(ESP_PLATFORM)
static void *host_alloc(void*c,size_t n){(void)c;return malloc(n);}
static void host_free(void*c,void*p){(void)c;free(p);}
static void *host_realloc(void*c,void*p,size_t old,size_t n){(void)c;(void)old;return realloc(p,n);}
#endif
static void *allocate(FT_Memory m,long n){nf_memory*a=&((nf_library*)m->user)->memory;return n>0?a->allocate(a->context,(size_t)n):NULL;}
static void deallocate(FT_Memory m,void*p){nf_memory*a=&((nf_library*)m->user)->memory;a->deallocate(a->context,p);}
static void *reallocate(FT_Memory m,long old,long n,void*p){nf_memory*a=&((nf_library*)m->user)->memory;return old<0||n<0?NULL:a->reallocate(a->context,p,(size_t)old,(size_t)n);}
nf_library *nf_library_new(const nf_memory *callbacks,int *error){
 nf_memory m=NF_DEFAULT_MEMORY;*error=0;
 if(callbacks){if(!callbacks->allocate||!callbacks->deallocate||!callbacks->reallocate){*error=6;return NULL;}m=*callbacks;}
 if(!m.allocate){*error=6;return NULL;}
 nf_library*l=m.allocate(m.context,sizeof(*l));if(!l){*error=FT_Err_Out_Of_Memory;return NULL;}
 memset(l,0,sizeof(*l));l->memory=m;l->ft_memory.user=l;l->ft_memory.alloc=allocate;l->ft_memory.free=deallocate;l->ft_memory.realloc=reallocate;
 *error=FT_New_Library(&l->ft_memory,&l->library);if(*error){m.deallocate(m.context,l);return NULL;}FT_Add_Default_Modules(l->library);return l;
}
void nf_library_done(nf_library*l){if(l){nf_memory m=l->memory;FT_Done_Library(l->library);m.deallocate(m.context,l);}}
uint32_t nf_capabilities(void){uint32_t c=0;
#if FREEINK_FONT_ENABLE_AUTOHINT
 c|=1;
#endif
#if FREEINK_FONT_ENABLE_NATIVE_HINTING
 c|=2;
#endif
#if FREEINK_FONT_ENABLE_MONOCHROME
 c|=4;
#endif
 return c;}
static unsigned long stream_io(FT_Stream s,unsigned long o,unsigned char*b,unsigned long n){nf_face*f=s->descriptor.pointer;if(!n)return 0;unsigned long got=f->read(f->context,o,b,n);if(got!=n)f->failed=1;return got;}
static void stream_close(FT_Stream s){(void)s;}
nf_face *nf_open(nf_library*l,const uint8_t*b,size_t n,nf_read read,void*ctx,unsigned long size,int*stage,int*error){
 *stage=0;*error=0;
 if((!b&&!read)||(b&&(!n||n>LONG_MAX))||(!b&&!size)){*stage=2;return NULL;}
 nf_face*f=l->memory.allocate(l->memory.context,sizeof(*f));if(!f){*stage=3;return NULL;}memset(f,0,sizeof(*f));f->owner=l;f->read=read;f->context=ctx;
 if(b){*error=FT_New_Memory_Face(l->library,b,(FT_Long)n,0,&f->face);}else{
 f->stream.size=size;f->stream.descriptor.pointer=f;f->stream.read=stream_io;f->stream.close=stream_close;FT_Open_Args args;memset(&args,0,sizeof(args));args.flags=FT_OPEN_STREAM;args.stream=&f->stream;*error=FT_Open_Face(l->library,&args,0,&f->face);}
 if(*error){*stage=f->failed?6:4;l->memory.deallocate(l->memory.context,f);return NULL;}return f;
}
void nf_close(nf_face*f){if(f){FT_Done_Face(f->face);f->owner->memory.deallocate(f->owner->memory.context,f);}}
size_t nf_axes(nf_face*f,nf_axis*out){if(!f)return 0;FT_MM_Var*mm=NULL;if(FT_Get_MM_Var(f->face,&mm)||!mm)return 0;size_t n=mm->num_axis<16?mm->num_axis:16;for(size_t i=0;i<n;i++){out[i]=(nf_axis){(uint32_t)mm->axis[i].tag,(int32_t)mm->axis[i].minimum,(int32_t)mm->axis[i].def,(int32_t)mm->axis[i].maximum};}FT_Done_MM_Var(f->owner->library,mm);return n;}
void nf_coordinates(nf_face*f,const int32_t*c,size_t n){if(!f)return;FT_Fixed v[16];if(n>16)n=16;for(size_t i=0;i<n;i++)v[i]=c[i];(void)FT_Set_Var_Design_Coordinates(f->face,(FT_UInt)n,v);}
int nf_size(nf_face*f,uint32_t n){if(!f)return 6;return FT_Set_Char_Size(f->face,n,n,72,72);}
uint32_t nf_glyph(nf_face*f,uint32_t cp){return f?FT_Get_Char_Index(f->face,cp):0;}
void nf_properties(nf_face*f,int dark,uint8_t version){if(!f)return;FT_Bool no_dark=!dark;(void)FT_Property_Set(f->owner->library,"autofitter","no-stem-darkening",&no_dark);
#if FREEINK_FONT_ENABLE_NATIVE_HINTING
 FT_UInt v=version;(void)FT_Property_Set(f->owner->library,"truetype","interpreter-version",&v);
#else
 (void)version;
#endif
}
void nf_transform(nf_face*f,int32_t shear){if(!f)return;FT_Matrix m={65536,shear,0,65536};FT_Set_Transform(f->face,shear?&m:NULL,NULL);}
int nf_load(nf_face*f,uint32_t g,int32_t flags){if(!f)return 6;return FT_Load_Glyph(f->face,g,flags);}
int nf_embolden(nf_face*f,int32_t s){if(!f)return 6;return FT_Outline_Embolden(&f->face->glyph->outline,s);}
int nf_render(nf_face*f,int mono){if(!f)return 6;return FT_Render_Glyph(f->face->glyph,mono?FT_RENDER_MODE_MONO:FT_RENDER_MODE_NORMAL);}
nf_slot nf_get_slot(nf_face*f){if(!f){nf_slot z;memset(&z,0,sizeof(z));return z;}FT_GlyphSlot s=f->face->glyph;nf_slot o;memset(&o,0,sizeof(o));o.outline=s->format==FT_GLYPH_FORMAT_OUTLINE;o.bitmap=s->format==FT_GLYPH_FORMAT_BITMAP;o.linear_advance=s->linearHoriAdvance;o.advance=s->advance.x;o.left=s->bitmap_left;o.top=s->bitmap_top;o.width=s->bitmap.width;o.rows=s->bitmap.rows;o.pitch=s->bitmap.pitch;o.mono=s->bitmap.pixel_mode==FT_PIXEL_MODE_MONO;if(o.outline){FT_BBox b;FT_Outline_Get_CBox(&s->outline,&b);o.xmin=b.xMin;o.xmax=b.xMax;o.ymin=b.yMin;o.ymax=b.yMax;}return o;}
const uint8_t *nf_bitmap(nf_face*f,size_t*n){if(!f){*n=0;return NULL;}FT_Bitmap*b=&f->face->glyph->bitmap;size_t p=b->pitch<0?(size_t)(-(int64_t)b->pitch):(size_t)b->pitch;*n=0;if(b->rows&&p>SIZE_MAX/b->rows)return NULL;*n=p*b->rows;if(!b->buffer)*n=0;return b->buffer;}
static int32_t clamp(FT_Pos n){int64_t wide=n;return wide>INT32_MAX?INT32_MAX:wide<INT32_MIN?INT32_MIN:(int32_t)wide;}
nf_lines nf_line_metrics(nf_face*f){if(!f)return(nf_lines){0,0,0};FT_Size_Metrics*m=&f->face->size->metrics;return(nf_lines){clamp(m->ascender),clamp(m->descender),clamp(m->height)};}
int32_t nf_kern(nf_face*f,uint32_t l,uint32_t r){if(!f)return 0;FT_Vector v={0,0};return FT_HAS_KERNING(f->face)&&!FT_Get_Kerning(f->face,l,r,FT_KERNING_UNFITTED,&v)?clamp(v.x):0;}
int32_t nf_scale(nf_face*f,int32_t u){if(!f)return 0;return clamp(FT_MulFix(u,f->face->size->metrics.x_scale));}
int nf_table(nf_face*f,uint32_t tag,uint8_t*b,size_t*n){if(!f)return 6;FT_ULong len=*n;int e=FT_Load_Sfnt_Table(f->face,tag,0,b,&len);*n=len;return e;}
int nf_describe(nf_face*f,uint8_t*family,size_t capacity,size_t*length,uint16_t*weight,int*italic){if(!f)return -1;FT_Face face=f->face;if(!FT_IS_SCALABLE(face)||FT_Select_Charmap(face,FT_ENCODING_UNICODE))return -1;const char*name=face->family_name?face->family_name:"";*length=strlen(name);if(capacity){size_t n=*length<capacity-1?*length:capacity-1;memcpy(family,name,n);family[n]=0;}TT_OS2*os2=FT_Get_Sfnt_Table(face,FT_SFNT_OS2);*weight=os2?os2->usWeightClass:(face->style_flags&FT_STYLE_FLAG_BOLD?700:400);*italic=!!(face->style_flags&FT_STYLE_FLAG_ITALIC);return 0;}
int nf_stream_failed(nf_face*f){return f&&f->failed;}
