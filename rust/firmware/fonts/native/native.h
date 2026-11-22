/* MIT; low-level FreeType ABI. No C++ FtFont is linked into Rust. */
#ifndef NRUST_FT_NATIVE_H
#define NRUST_FT_NATIVE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct nf_library nf_library;
typedef struct nf_face nf_face;
typedef struct { void *context; void *(*allocate)(void*,size_t); void (*deallocate)(void*,void*); void *(*reallocate)(void*,void*,size_t,size_t); } nf_memory;
typedef unsigned long (*nf_read)(void*,unsigned long,unsigned char*,unsigned long);
typedef struct {uint32_t tag;int32_t minimum,def,maximum;} nf_axis;
typedef struct {int32_t outline,bitmap;int64_t linear_advance,advance,left,top;uint32_t width,rows;int32_t pitch,mono;int64_t xmin,xmax,ymin,ymax;} nf_slot;
typedef struct {int32_t ascender,descender,height;} nf_lines;
/* Caller keeps the context alive through library destruction; callbacks are
 * copied. NULL selects host libc on host and is rejected (6) on ESP_PLATFORM. */
nf_library *nf_library_new(const nf_memory*,int*);
#if defined(ESP_PLATFORM)
/* Static callbacks over FreeInkFont fiFontMalloc/Realloc/Free, FtFont's default. */
const nf_memory *nf_font_heap(void);
#endif
void nf_library_done(nf_library*);
uint32_t nf_capabilities(void);
nf_face *nf_open(nf_library*,const uint8_t*,size_t,nf_read,void*,unsigned long,int*,int*);
void nf_close(nf_face*);
size_t nf_axes(nf_face*,nf_axis*);
void nf_coordinates(nf_face*,const int32_t*,size_t);
int nf_size(nf_face*,uint32_t);
uint32_t nf_glyph(nf_face*,uint32_t);
void nf_properties(nf_face*,int,uint8_t);
void nf_transform(nf_face*,int32_t);
int nf_load(nf_face*,uint32_t,int32_t);
int nf_embolden(nf_face*,int32_t);
int nf_render(nf_face*,int);
nf_slot nf_get_slot(nf_face*);
const uint8_t *nf_bitmap(nf_face*,size_t*);
nf_lines nf_line_metrics(nf_face*);
int32_t nf_kern(nf_face*,uint32_t,uint32_t);
int32_t nf_scale(nf_face*,int32_t);
int nf_table(nf_face*,uint32_t,uint8_t*,size_t*);
int nf_describe(nf_face*,uint8_t*,size_t,size_t*,uint16_t*,int*);
int nf_stream_failed(nf_face*);
#ifdef __cplusplus
}
#endif
#endif
