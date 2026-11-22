use newt_core::zip_window::{arena::Arena, source::BookSource, window::ZipDirectoryWindow};
#[test]
fn arena_relative_alignment_diagnostics_and_nul() {
    let mut backing = [0xff; 65];
    let mut arena = Arena::new(&mut backing[1..]);
    let a = arena.alloc(3, 1).unwrap();
    assert_eq!(a.offset(), 0);
    let b = arena.alloc(2, 8).unwrap();
    assert_eq!(b.offset(), 8);
    assert_eq!(arena.used(), 10);
    assert_eq!(arena.high_water(), 10);
    assert!(arena.alloc(99, 3).is_none());
    assert_eq!(arena.failed_alloc_size(), 0);
    assert!(arena.alloc(99, 0).is_none());
    assert_eq!(arena.failed_alloc_size(), 0);
    assert!(arena.alloc(99, 1).is_none());
    assert_eq!(arena.failed_alloc_size(), 99);
    assert!(arena.alloc(70, 1).is_none());
    assert_eq!(arena.failed_alloc_size(), 99);
    assert!(arena.alloc_array_bytes(usize::MAX, 2, 1).is_none());
    assert_eq!(arena.failed_alloc_size(), 99);
    arena.release(11);
    assert_eq!(arena.used(), 10);
    arena.release(3);
    assert_eq!(arena.used(), 3);
    assert!(arena.view(b).is_none());
    let c = arena.copy_nul(Some(b"a\0b")).unwrap();
    assert_eq!(arena.view(c).unwrap(), b"a\0b\0");
    let d = arena.copy_c_string(Some(b"xy\0unused")).unwrap();
    assert_eq!(arena.view(d).unwrap(), b"xy\0");
    assert!(arena.copy_nul(None).is_none());
    assert!(arena.copy_c_string(Some(b"missing")).is_none());
    let mark = arena.mark();
    let e = arena.copy_nul(Some(b"")).unwrap();
    assert_eq!(arena.view(e).unwrap(), b"\0");
    arena.release(mark);
    arena.reset();
    assert_eq!(arena.used(), 0);
    assert_eq!(arena.high_water(), mark + 1);
    assert_eq!(arena.failed_alloc_size(), 99);
    let z = arena.alloc(64, 1).unwrap();
    assert_eq!(z.len(), 64);
    let zero = arena.alloc(0, 1).unwrap();
    assert_eq!(zero.offset(), 64);
    assert_eq!(arena.view(zero).unwrap(), b"");
    assert!(arena.alloc(usize::MAX, 1).is_none());
    assert_eq!(arena.failed_alloc_size(), usize::MAX);
    let mut replacement = [0; 4];
    arena.init(&mut replacement);
    assert_eq!(
        (arena.used(), arena.high_water(), arena.failed_alloc_size()),
        (0, 0, 0)
    );
    assert_eq!(arena.capacity(), 4);
}
#[test]
fn uninitialized_is_distinct_from_initialized_empty() {
    let mut uninit = Arena::default();
    assert!(uninit.alloc(1, 1).is_none());
    assert!(uninit.alloc(0, 1).is_none());
    assert_eq!(uninit.failed_alloc_size(), 0);
    let mut backing = [];
    uninit.init(&mut backing);
    assert!(uninit.alloc(0, 1).is_some());
    assert!(uninit.alloc(1, 1).is_none());
    assert_eq!(uninit.failed_alloc_size(), 1);
}
struct Memory<'a>(&'a [u8]);
impl BookSource for Memory<'_> {
    fn size(&self) -> u64 {
        self.0.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        let Some(tail) = usize::try_from(at).ok().and_then(|at| self.0.get(at..)) else {
            return -1;
        };
        let got = out.len().min(tail.len());
        out[..got].copy_from_slice(&tail[..got]);
        got as i32
    }
}
#[test]
fn runtime_borrowed_window_and_failed_constructor_restore_nested_mark() {
    let data = [23; 5000];
    let mut source = Memory(&data);
    let mut backing = [0; 4200];
    let mut arena = Arena::new(&mut backing);
    arena.alloc(73, 1).unwrap();
    arena.alloc(5000, 1);
    let mut out = [0; 12];
    {
        let mut w = ZipDirectoryWindow::new(&mut source, &mut arena, 20, 4900, None);
        assert!(w.cached());
        assert_eq!(w.arena_used(), 4169);
        assert_eq!(w.arena_failed_alloc_size(), 5000);
        assert!(!w.read(19, Some(&mut out), 12));
        assert!(!w.failed());
        assert!(!w.read(20, Some(&mut out[..1]), 12));
        assert!(!w.failed());
        assert!(w.read(20, Some(&mut out), 12));
        assert_eq!(out, [23; 12]);
    }
    assert_eq!(
        (arena.used(), arena.high_water(), arena.failed_alloc_size()),
        (73, 4169, 5000)
    );
    {
        let mut w = ZipDirectoryWindow::new(&mut source, &mut arena, 30, 20, None);
        assert!(w.failed());
        assert!(!w.read(30, None, 0));
    }
    assert_eq!(arena.used(), 73);
}
