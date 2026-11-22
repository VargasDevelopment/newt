// MIT; see rust/THIRD_PARTY.md.
//! Resolve a projected identity using the existing bounded choice policy.
use super::{Choice, Identity, copy_label, generic_label};

pub fn resolve_identity(
    entries: &[Identity<'_>],
    spine: u32,
    character: u32,
    output: &mut [u8],
) -> bool {
    let mut choice = Choice::default();
    let mut safe = true;
    for (i, entry) in entries.iter().enumerate() {
        if entry.projection_spine != spine {
            continue;
        }
        if !choice.consider(
            i,
            entry.char_start,
            entry.title_key,
            entry.depth,
            entry.inherited,
            entry.target_known,
            entry.anchor_known,
            character,
        ) {
            safe = false;
            break;
        }
    }
    if safe
        && choice
            .index
            .is_some_and(|i| copy_label(Some(entries[i].label), output))
    {
        return true;
    }
    generic_label(spine, output)
}
