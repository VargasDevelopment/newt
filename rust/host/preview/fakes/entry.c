// SPDX-License-Identifier: MIT
// Host preview entry, in the role of the device's SDK entry
// (rust/firmware/native/entry.cpp): lend the firmware two native objects and
// call its own native entry. The fakes own these two disjoint tokens for the
// whole process, as the SDK owns its display and input objects until reset.
extern void newt_rust_app_run(void *display, void *input);

static unsigned char objects[2];

int main(void) {
    newt_rust_app_run(&objects[0], &objects[1]);
    return 0;
}
