/* Runtime descriptor remains caller-owned; calls cannot be constant-folded. */
#include "../runtime.h"
__attribute__((used,noinline)) uint8_t jpeg_retained_probe(struct Runtime *r, uint32_t mode) {
  return mode ? newt_jpeg_progressive(r) : newt_jpeg_baseline(r);
}
void *memcpy(void *d, const void *s, size_t n) {unsigned char *a=d;const unsigned char *b=s;for(size_t i=0;i<n;i++)a[i]=b[i];return d;}
void *memset(void *d, int v, size_t n) {unsigned char *a=d;for(size_t i=0;i<n;i++)a[i]=(unsigned char)v;return d;}
int memcmp(const void *a, const void *b, size_t n) {const unsigned char *x=a,*y=b;for(size_t i=0;i<n;i++)if(x[i]!=y[i])return x[i]-y[i];return 0;}
void *memmove(void *d, const void *s, size_t n) {unsigned char *a=d;const unsigned char *b=s;if(a<b){for(size_t i=0;i<n;i++)a[i]=b[i];}else{while(n){--n;a[n]=b[n];}}return d;}
