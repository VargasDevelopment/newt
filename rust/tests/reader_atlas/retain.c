/* Freestanding retained caller. Caller owns three disjoint valid allocations:
 * aligned const Input[count], writable frame[len], aligned writable output[2].
 * None is retained; external runtime values reach both font probes and output.
 * This is a link/code-retention proof, not an executed RV32 program. */
typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned char u8;
typedef int i32;
typedef long long i64;
struct Input { u32 cp, right; u16 px; u8 style, reserved; i32 x, baseline; };
extern i32 atlas_borrowed_runtime(const struct Input *, u32, u8 *, u32, i64 *, u32);
i32 atlas_retained_probe(const struct Input *inputs, u32 count, u8 *frame, u32 len, i64 *output, u32 landscape) {
    return atlas_borrowed_runtime(inputs, count, frame, len, output, landscape);
}
