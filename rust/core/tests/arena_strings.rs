use newt_core::zip_window::arena::{Arena, CStringRef};
#[test]
fn compact_strings_keep_nul_semantics_and_do_not_allocate_on_conversion() {
    let mut memory = [0; 32];
    let mut arena = Arena::new(&mut memory);
    assert_eq!(
        core::mem::size_of::<Option<CStringRef>>(),
        core::mem::size_of::<usize>()
    );
    let first = arena.copy_nul_ref(Some(b"a\0suffix")).unwrap();
    assert_eq!(first.offset(), 0);
    assert_eq!(arena.c_string(first), Some(b"a".as_slice()));
    let empty = arena.copy_nul_ref(Some(b"")).unwrap();
    assert_eq!(arena.c_string(empty), Some(b"".as_slice()));
    let range = arena.copy_nul(Some(b"second")).unwrap();
    let used = arena.used();
    let second = arena.string_ref(range).unwrap();
    assert_eq!(arena.used(), used);
    assert_eq!(arena.c_string(second), Some(b"second".as_slice()));
    assert_eq!(arena.copy_nul_ref(None), None);
    arena.release(range.offset());
    assert_eq!(arena.c_string(second), None);
    arena.reset();
    assert_eq!(arena.c_string(first), None);
}
#[test]
fn range_validation_and_oom_preserve_the_existing_arena_contract() {
    let mut memory = [0; 4];
    let mut arena = Arena::new(&mut memory);
    let raw = arena.alloc(2, 1).unwrap();
    arena.view_mut(raw).unwrap().copy_from_slice(b"ab");
    assert_eq!(arena.string_ref(raw), None);
    let value = arena.copy_nul_ref(Some(b"x")).unwrap();
    assert_eq!(arena.c_string(value), Some(b"x".as_slice()));
    assert_eq!(arena.copy_nul_ref(Some(b"")), None);
    assert_eq!(arena.used(), 4);
    assert_eq!(arena.failed_alloc_size(), 1);
}

#[test]
fn streaming_equality_requires_a_real_terminator_and_respects_embedded_nuls() {
    let mut memory = [0; 64];
    let mut arena = Arena::new(&mut memory);
    let good = arena.copy_nul_ref(Some(b"abc\0suffix")).unwrap();
    assert!(arena.c_string_eq(good, b"abc"));
    assert!(!arena.c_string_eq(good, b"abc\0ignored"));
    assert!(!arena.c_string_eq(good, b"abcd"));
    assert!(!arena.c_string_eq(good, b"ab"));
    assert!(!arena.c_string_eq(good, b""));
    let empty = arena.copy_nul_ref(Some(b"")).unwrap();
    assert!(arena.c_string_eq(empty, b""));
    let range = arena.copy_nul(Some(b"xy")).unwrap();
    let broken = arena.string_ref(range).unwrap();
    arena.view_mut(range).unwrap()[2] = b'z';
    assert_eq!(arena.c_string(broken), None);
    assert!(!arena.c_string_eq(broken, b"xyz"));
    assert!(!arena.c_string_eq(broken, b"xy"));
    arena.release(range.offset() + 1);
    assert!(!arena.c_string_eq(broken, b"x"));
    arena.release(range.offset());
    assert!(!arena.c_string_eq(broken, b""));
    arena.reset();
    assert!(!arena.c_string_eq(good, b"abc"));
}
