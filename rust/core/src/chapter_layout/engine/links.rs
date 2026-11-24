// FreeInk cd6f5b8 ChapterLayout.cpp translation. MIT; same hyperlink path.
use super::{EngineState, Link, copy_page_bytes};

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

    /// Record one visual segment's link rect, merging adjacent segments of the
    /// same link on the current line and refusing the line when slots run out.
    pub(super) fn record_link(
        &mut self,
        link_id: u8,
        active_link: &mut u8,
        x: i16,
        y: i16,
        width: u16,
        height: u16,
    ) -> bool {
        if link_id == 0 {
            *active_link = 0;
            return true;
        }
        if *active_link == link_id {
            if let Some(link) = self.storage.links[..self.link_count].last_mut() {
                extend_link(link, x, y, width, height);
                return true;
            }
        }
        if self.link_count >= self.storage.links.len() {
            return false;
        }
        let p = &self.storage.paragraph_links[usize::from(link_id - 1)];
        let Some(target) =
            copy_page_bytes(self.storage.page, &mut self.pool_len, &p.path[..p.path_len])
        else {
            return false;
        };
        let Some(fragment) = copy_page_bytes(
            self.storage.page,
            &mut self.pool_len,
            &p.fragment[..p.fragment_len],
        ) else {
            return false;
        };
        self.storage.links[self.link_count] = Link {
            target,
            fragment,
            x,
            y,
            width,
            height,
        };
        self.link_count += 1;
        *active_link = link_id;
        true
    }
}

fn extend_link(link: &mut Link, x: i16, y: i16, width: u16, height: u16) {
    let left = i32::from(link.x).min(i32::from(x));
    let top = i32::from(link.y).min(i32::from(y));
    let right =
        (i32::from(link.x) + i32::from(link.width)).max(i32::from(x) + i32::from(width));
    let bottom =
        (i32::from(link.y) + i32::from(link.height)).max(i32::from(y) + i32::from(height));
    link.x = left as i16;
    link.y = top as i16;
    link.width = (right - left).clamp(1, i32::from(u16::MAX)) as u16;
    link.height = (bottom - top).clamp(0, i32::from(u16::MAX)) as u16;
}
