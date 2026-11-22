/* Link-only external caller: no Rust-layout construction, firmware or execution.
 * An eventual Rust owner must supply initialized Navigation and Metadata values
 * and nonempty real parsed metadata, disjoint caller-owned mapping, observations,
 * output label, and result[4], with the contracts in retention.rs. */
typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned char u8;
struct Anchor { u32 hash, start; };
extern u32 navigation_bind_runtime(void *, const void *, u32, u16);
extern u32 navigation_anchors_runtime(void *, const struct Anchor *, u32, u32, u8 *, u32);
extern u32 navigation_menu_runtime(const void *, void *, const void *, u32 *, u32, u32, u32, u8 *, u32);
extern const u8 navigation_type_size[];
void navigation_retained_probe(void *state, void *menu, const void *metadata,
    const struct Anchor *anchors, u32 anchors_len, u32 *mapping, u32 mapping_len,
    u32 selected, u16 required, u32 character, u32 focus, u8 *label, u32 label_len,
    u32 *result) {
    result[0] = navigation_bind_runtime(state, metadata, selected, required);
    result[1] = navigation_anchors_runtime(state, anchors, anchors_len, character, label, label_len);
    result[2] = navigation_menu_runtime(state, menu, metadata, mapping, mapping_len, character, focus, label, label_len);
    result[3] = navigation_type_size[0];
}

/* Memory shims reused verbatim from rust/tests/jpeg_decoder/tests/retain.c
 * (repository MIT). Same freestanding -fno-builtin retention pattern. */
typedef __SIZE_TYPE__ size_t;
void *memcpy(void *d, const void *s, size_t n) {unsigned char *a=d;const unsigned char *b=s;for(size_t i=0;i<n;i++)a[i]=b[i];return d;}
void *memset(void *d, int v, size_t n) {unsigned char *a=d;for(size_t i=0;i<n;i++)a[i]=(unsigned char)v;return d;}
int memcmp(const void *a, const void *b, size_t n) {const unsigned char *x=a,*y=b;for(size_t i=0;i<n;i++)if(x[i]!=y[i])return x[i]-y[i];return 0;}
void *memmove(void *d, const void *s, size_t n) {unsigned char *a=d;const unsigned char *b=s;if(a<b){for(size_t i=0;i<n;i++)a[i]=b[i];}else{while(n){--n;a[n]=b[n];}}return d;}
