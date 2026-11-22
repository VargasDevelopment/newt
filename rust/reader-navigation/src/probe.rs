//! Runtime borrowed no_std entry points. Retention still needs execution.
use super::*;

#[inline(never)]
pub fn estimate(input: Estimate, character: u32) -> u32 {
    input.percent(character).map_or(u32::MAX, u32::from)
}
#[inline(never)]
pub fn identity(entries: &[Identity<'_>], spine: u32, character: u32, output: &mut [u8]) -> bool {
    resolve_identity(entries, spine, character, output)
}
#[inline(never)]
pub fn anchors(
    navigation: &mut Navigation,
    observations: &[(u32, u32)],
    reset: bool,
    character: u32,
    output: &mut [u8],
) -> u32 {
    if reset {
        navigation.begin_anchors();
    }
    for &(hash, start) in observations {
        navigation.observe_anchor(hash, start);
    }
    let section = navigation.section(character);
    let copied = copy_label(Some(section.label), output);
    u32::from(navigation.issues()) | (u32::from(copied) << 16)
}
#[inline(never)]
pub fn object_bytes() -> usize {
    core::mem::size_of::<Navigation>()
}
