    use super::*;
    #[repr(align(16))]
    struct UnalignedEnd([u8; 66]);
    #[test]
    fn empty_prefix_arrays_leave_exhausted_unaligned_cursor_untouched() {
        let mut bytes = UnalignedEnd([0; 66]);
        let mut parts = Parts(&mut bytes.0[66..]);
        let address = parts.0.as_ptr();
        assert_eq!(parts.array::<u32>(0).unwrap().len(), 0);
        assert_eq!(parts.array::<ManifestItem>(0).unwrap().len(), 0);
        assert_eq!(parts.0.as_ptr(), address);
        assert_eq!(parts.0.len(), 0);
        assert!(parts.array::<u32>(1).is_err());
    }
    #[test]
    fn empty_retained_arrays_accept_exhausted_unaligned_arena() {
        let mut bytes = UnalignedEnd([0; 66]);
        let mut arena = Arena::new(&mut bytes.0);
        assert!(arena.alloc(66, 1).is_some());
        assert_eq!(tail_array::<Entry>(&mut arena, 0).unwrap().len(), 0);
        assert_eq!(tail_array::<ManifestItem>(&mut arena, 0).unwrap().len(), 0);
        assert_eq!(tail_array::<u16>(&mut arena, 0).unwrap().len(), 0);
        assert_eq!((arena.capacity(), arena.used()), (66, 66));
        assert!(tail_array::<Entry>(&mut arena, 1).is_err());
    }
