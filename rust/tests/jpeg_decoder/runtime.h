/* Caller retains ownership. The Runtime descriptor and every span must remain
 * valid, aligned and mutually disjoint for the complete call. No span may
 * overlap the descriptor, including its width/height fields. Encoded bytes must
 * also be disjoint from every writable span. Nonnull pointers are required even
 * for empty spans. No allocations occur. */
#ifndef NEWT_JPEG_BORROWED_RUNTIME_H
#define NEWT_JPEG_BORROWED_RUNTIME_H
#include <stdint.h>
#include <stddef.h>
typedef struct { uint8_t *ptr; size_t len; } JpegBytes;
typedef struct { int32_t *ptr; size_t len; } JpegI32;
typedef struct { int16_t *ptr; size_t len; } JpegI16;
typedef struct { uint32_t *ptr; size_t len; } JpegU32;
typedef struct { uint16_t *ptr; size_t len; } JpegU16;
struct Runtime {
 JpegBytes encoded,input,segment;
 JpegI32 quant;
 JpegBytes huffman;
 JpegI32 block;
 JpegI16 mcu;
 JpegBytes pixels,band,row;
 JpegU32 sums;
 JpegU16 counts;
 JpegBytes output;
 size_t stride;
 uint16_t width,height;
};
uint8_t newt_jpeg_baseline(struct Runtime *);
uint8_t newt_jpeg_progressive(struct Runtime *);
/* Baseline: original Workspace/Rows capacities. Progressive: input>=512,
 * band>=padded DC grid width * Y vertical sampling; segment,row,pixels>=width.
 * Output needs stride>=width and stride*(height-1)+width bytes. The safe core
 * checks capacities. Progressive tables consume a bounded wrapper stack frame.
 * Returns BookStatus byte or 250 for an invalid descriptor/row span. */
#endif
