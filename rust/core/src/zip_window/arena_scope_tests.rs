// SPDX-License-Identifier: MIT
use super::Arena;

#[test]
fn temporary_tail_restores_capacity_used_and_diagnostics() {
    let mut bytes = [0; 257];
    let mut arena = Arena::new(&mut bytes[1..]);
    let kept = arena.copy_nul_ref(Some(b"retained")).unwrap();
    let permanent = arena.take_tail(16, 8).unwrap();
    permanent.fill(0x77);
    let capacity = arena.capacity();
    let mark = arena.mark();
    let temporary = arena
        .with_tail(32, 16, |prefix, tail| {
            assert_eq!(tail.as_ptr().align_offset(16), 0);
            assert!(prefix.capacity() <= capacity - 32);
            tail.fill(0x55);
            let value = prefix.copy_nul_ref(Some(b"temporary")).unwrap();
            assert_eq!(prefix.c_string(kept), Some(b"retained".as_slice()));
            assert!(prefix.alloc(999, 1).is_none());
            assert_eq!(tail, &[0x55; 32]);
            value
        })
        .unwrap();
    assert_eq!(arena.capacity(), capacity);
    assert_eq!(arena.c_string(temporary), Some(b"temporary".as_slice()));
    assert_eq!(arena.high_water(), mark + 10);
    assert_eq!(arena.failed_alloc_size(), 999);
    arena.release(mark);
    assert_eq!(arena.c_string(temporary), None);
    assert!(arena.alloc(capacity - mark, 1).is_some());
    assert_eq!(permanent, &[0x77; 16]);
}

#[test]
fn rejection_and_callback_error_leave_full_capacity_available() {
    let mut bytes = [0; 64];
    let mut arena = Arena::new(&mut bytes);
    for (size, align) in [(usize::MAX, 1), (65, 1), (1, 3)] {
        assert_eq!(
            arena.with_tail(size, align, |_, _| panic!("rejected")),
            None::<()>
        );
        assert_eq!(arena.capacity(), 64);
        assert_eq!(arena.used(), 0);
    }
    assert_eq!(
        arena.with_tail(16, 1, |prefix, _| {
            assert!(prefix.copy_nul_ref(Some(b"partial")).is_some());
            Err::<(), _>(17)
        }),
        Some(Err(17))
    );
    assert_eq!(arena.capacity(), 64);
    assert_eq!(arena.used(), 8);
    arena.release(0);
    assert!(arena.alloc(64, 1).is_some());
}
