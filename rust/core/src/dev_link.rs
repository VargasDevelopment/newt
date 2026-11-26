//! MIT. When the DEV USB link counts as down (#170).
//!
//! HWCDC's `connected` flag also goes false while the cable stays plugged in,
//! for example after one slow write. Ending a session on that one sample made
//! false `ERROR session` replies. The link is down only when the raw SOF
//! monitor has said "unplugged" for `LINK_DOWN_MS` without a break.

/// How long the raw monitor must report "unplugged" before a session ends.
pub const LINK_DOWN_MS: u32 = 250;

/// Running totals since boot, for the DEV `USB link` row. They wrap; `u16`
/// keeps the watch small, since it lives on the UI task's stack.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkCounts {
    /// Monitor "unplugged" spells that ended inside the window.
    pub blips: u16,
    /// Monitor "unplugged" spells that reached the window: the link was lost.
    pub lost: u16,
    /// Times HWCDC's flag went false while the monitor still saw the host.
    pub latched: u16,
}

#[derive(Clone, Copy, Default)]
enum Monitor { #[default] Present, Absent { since: u32 }, Lost }

#[derive(Default)]
pub struct LinkWatch {
    monitor: Monitor,
    latch_low: bool,
    counts: LinkCounts,
}

impl LinkWatch {
    /// One sample. `present` is the raw SOF monitor; `latched` is HWCDC's
    /// flag. Returns whether the link counts as up.
    pub fn sample(&mut self, present: bool, latched: bool, now: u32) -> bool {
        if present {
            if matches!(self.monitor, Monitor::Absent { .. }) { self.counts.blips = self.counts.blips.wrapping_add(1); }
            self.monitor = Monitor::Present;
            if !latched && !self.latch_low { self.counts.latched = self.counts.latched.wrapping_add(1); }
            self.latch_low = !latched;
            return true;
        }
        self.latch_low = false;
        match self.monitor {
            Monitor::Present => { self.monitor = Monitor::Absent { since: now }; true }
            Monitor::Absent { since } if now.wrapping_sub(since) < LINK_DOWN_MS => true,
            Monitor::Absent { .. } => {
                self.counts.lost = self.counts.lost.wrapping_add(1);
                self.monitor = Monitor::Lost;
                false
            }
            Monitor::Lost => false,
        }
    }

    pub fn counts(&self) -> LinkCounts { self.counts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_absent_sample_keeps_the_link_and_counts_a_blip() {
        let mut watch = LinkWatch::default();
        assert!(watch.sample(true, true, 0));
        assert!(watch.sample(false, false, 10));
        assert!(watch.sample(false, false, 10 + LINK_DOWN_MS - 1));
        assert!(watch.sample(true, true, 300));
        assert_eq!(watch.counts(), LinkCounts { blips: 1, ..LinkCounts::default() });
    }

    #[test]
    fn a_monitor_absent_for_the_window_loses_the_link_once() {
        let mut watch = LinkWatch::default();
        assert!(watch.sample(false, false, u32::MAX - 100));
        assert!(!watch.sample(false, false, (u32::MAX - 100).wrapping_add(LINK_DOWN_MS)));
        assert!(!watch.sample(false, false, 1_000));
        assert!(watch.sample(true, true, 2_000));
        assert_eq!(watch.counts(), LinkCounts { lost: 1, ..LinkCounts::default() });
    }

    #[test]
    fn a_false_flag_with_the_host_present_is_up_and_counted_per_spell() {
        let mut watch = LinkWatch::default();
        for now in 0..3 { assert!(watch.sample(true, false, now)); }
        assert!(watch.sample(true, true, 3));
        assert!(watch.sample(true, false, 4));
        assert_eq!(watch.counts(), LinkCounts { latched: 2, ..LinkCounts::default() });
    }
}
