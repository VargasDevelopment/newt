/// Operation owner only; no source, Action, reader, or resource is retained.
/// Route is supplied by the revision owner using the exact canonical envelope;
/// this API must not substitute SDK's short filename for that identity.
pub struct CacheBuild<'n> {
    pub name: &'n [u8],
    pub generation: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildResult {
    pub status: Status,
    pub verified_empty: bool,
}
impl BuildResult {
    fn status(status: Status) -> Self {
        Self {
            status,
            verified_empty: false,
        }
    }
}
