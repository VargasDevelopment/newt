// SPDX-License-Identifier: MIT
// SOURCE PROPOSAL only. Same language primitive as pinned book_open_control.h;
// no claim this links on RV32 or that the target's runtime closure is proved.
#include "atomic_bridge.h"
static constexpr auto order = std::memory_order_seq_cst;
uint32_t np_atomic_load(const np_atomic_u32 *p) { return p->value.load(order); }
uint32_t np_atomic_or(np_atomic_u32 *p,uint32_t v) { return p->value.fetch_or(v,order); }
uint32_t np_atomic_and(np_atomic_u32 *p,uint32_t v) { return p->value.fetch_and(v,order); }
uint32_t np_atomic_add(np_atomic_u32 *p,uint32_t v) { return p->value.fetch_add(v,order); }
uint32_t np_atomic_sub(np_atomic_u32 *p,uint32_t v) { return p->value.fetch_sub(v,order); }
uint32_t np_atomic_exchange(np_atomic_u32 *p,uint32_t v) { return p->value.exchange(v,order); }
uint8_t np_atomic_cas_weak(np_atomic_u32 *p,uint32_t *expected,uint32_t desired) {
    return p->value.compare_exchange_weak(*expected,desired,order,order);
}
