/* SPDX-License-Identifier: MIT
 * Existing Board host transport plus an explicit component cold-reset seam.
 * Never linked into target firmware. No SDK or native bridge is modified. */
#include "../board/host_fake.c"
void checkpoint_cold_reset(void) {
    assert(!lent);
    memset(pixels, 0, sizeof pixels);
    fault = updates = last_mode = last_off = lends = returns = mounts = input_begins = 0;
    millis_value = UINT32_MAX;
}
