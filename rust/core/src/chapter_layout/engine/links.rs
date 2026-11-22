// FreeInk cd6f5b8 ChapterLayout.cpp translation. MIT; same hyperlink path.
use super::EngineState;

impl EngineState<'_> {
    // Retire hyperlink-only scratch before subsequent layout callbacks.
    #[inline(never)]
    pub(super) fn start_link(&mut self, href: &[u8]) {
        if self.par_link_count < self.storage.paragraph_links.len() {
            let p = &mut self.storage.paragraph_links[self.par_link_count];
            let mut path = [0; 512];
            let mut frag = [0; 512];
            if let Some((np, nf)) = self.image_source.resolve(href, &mut path, &mut frag) {
                p.path_len = np.min(159);
                p.fragment_len = nf.min(47);
                p.path[..p.path_len].copy_from_slice(&path[..p.path_len]);
                p.fragment[..p.fragment_len].copy_from_slice(&frag[..p.fragment_len]);
                self.par_link_count += 1;
                self.current_link = self.par_link_count as u8;
                self.note()
            }
        }
    }
}
