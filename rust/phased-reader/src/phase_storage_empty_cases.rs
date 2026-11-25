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

#[test]
fn contents_batch_bound_leaves_query_and_margin_and_falls_back_above_it() {
    use super::{BANK_BYTES, CONTENTS_BATCH_ROWS, CONTENTS_QUERY_BYTES, Phase};
    let mut shared = [0; BANK_BYTES];
    let mut parse = [0; BANK_BYTES];
    let mut held = false;
    let mut phase = Phase { policy: None, shared: &mut shared, parse: &mut parse, held: &mut held };
    for count in [0, CONTENTS_BATCH_ROWS, CONTENTS_BATCH_ROWS + 1] {
        phase.with_contents_mapping(count, |batch, query| {
            assert_eq!(query.len(), CONTENTS_QUERY_BYTES);
            assert_eq!(batch.is_some(), count <= CONTENTS_BATCH_ROWS);
            if let Some((mapping, rows)) = batch {
                assert_eq!((mapping.len(), rows.len()), (count, count));
                let used = core::mem::size_of_val(mapping) + core::mem::size_of_val(rows) + query.len();
                assert!(used + 8192 <= BANK_BYTES);
            }
        }).unwrap();
    }
}
