extern crate std;
use super::*;
use crate::update_online::*;
use std::{format, vec, vec::Vec};

struct Client { replies: Vec<Vec<u8>>, current: Vec<u8>, opens: usize }
struct Timer;
impl Clock for Timer { fn millis(&mut self) -> u32 { 0 } }
impl Http for Client {
    fn limit(&mut self, _: u32, _: &mut dyn Clock) {}
    fn open(&mut self, _: &CStr, _: bool, head: &mut HttpHead, _: &mut [u8; URL_BYTES], _: &mut dyn Clock) -> bool {
        self.opens += 1;
        self.current = self.replies.remove(0);
        *head = HttpHead { status: 200, length: self.current.len() as i64, ..HttpHead::default() };
        true
    }
    fn read(&mut self, out: &mut [u8], _: &mut dyn Clock) -> i32 {
        let count = out.len().min(self.current.len()).min(137);
        out[..count].copy_from_slice(&self.current[..count]); self.current.drain(..count);
        count as i32
    }
    fn close(&mut self) {}
}
struct Digest;
impl Hash for Digest {
    fn start(&mut self) -> bool { true }
    fn update(&mut self, _: &[u8]) -> bool { true }
    fn finish(&mut self, out: &mut [u8; 32]) -> bool { out.fill(7); true }
}
struct Card { saved: bool, grant: Option<ReturnAuthorization>, regular: bool }
impl CardDownload for Card {
    fn begin(&mut self) -> bool { true }
    fn write(&mut self, _: &[u8]) -> bool { true }
    fn finish(&mut self, _: &[u8]) -> bool { self.regular = true; true }
    fn finish_return(&mut self, _: &[u8], grant: &ReturnAuthorization) -> bool {
        self.grant = Some(*grant); self.saved
    }
    fn abort(&mut self) { panic!("no download failure in this fixture"); }
}

#[test]
fn latest_feed_only_issues_an_older_return_after_verified_staging() {
    for (running, candidate, channel, saved, expected, granted) in [
        (c"0.5.1-dev.20261125.1", "0.5.0", TestBuilds::Off, true, OnlineCheck::Ready, true),
        (c"0.5.1-dev.20261125.1", "0.5.0", TestBuilds::Off, false, OnlineCheck::CheckAgain, true),
        (c"0.5.1-dev.20261125.1", "0.5.0", TestBuilds::On, true, OnlineCheck::Latest, false),
        (c"0.5.1", "0.5.0", TestBuilds::Off, true, OnlineCheck::Latest, false),
        (c"0.5.1-dev.20261125.1", "0.4.1", TestBuilds::Off, true, OnlineCheck::ReturnIncompatible, false),
        (c"0.5.1-dev.20261125.1", "0.5.1-alpha", TestBuilds::Off, true, OnlineCheck::WrongFile, false),
        (c"0.5.1-dev.20261125.1", "0.6.0", TestBuilds::Off, true, OnlineCheck::Ready, false),
    ] {
        let source = crate::update_card::slot_installer::identity::newt_app_identity(running, Some(c"0123456789ab"));
        let listing = format!("{{\"assets\":[{{\"name\":\"newt.manifest\",\"browser_download_url\":\"https://example.invalid/newt.manifest\"}},{{\"name\":\"newt.bin\",\"browser_download_url\":\"https://example.invalid/newt.bin\",\"size\":512}}]}}");
        let listing = if channel == TestBuilds::On { format!("[{listing}]") } else { listing };
        let manifest = format!("format=newt-update-1\nkind=newt\nversion={candidate}\nbuild=0123456789ab\nsize=512\nsha256={}\n", "07".repeat(32));
        let mut client = Client { replies: vec![listing.into_bytes(), manifest.into_bytes(), vec![0; 512]], current: Vec::new(), opens: 0 };
        let mut card = Card { saved, grant: None, regular: false };
        let mut work = OnlineWork { url: &mut [0; URL_BYTES], location: &mut [0; URL_BYTES],
            release: &mut crate::update_release::ReleaseJson::default(), manifest: &mut [0; crate::update_manifest::MAX_BYTES],
            manifest_length: &mut 0, buffer: &mut [0; READ_BYTES] };
        let mut target = UpdateManifest::default();
        let result = fetch_online_update_with_http(&mut client, &mut card, &mut Digest, &mut Timer,
            &mut |_, _, _| (), &mut work, channel, Some(&source), &mut target);
        assert_eq!(result, expected, "{running:?} -> {candidate}");
        assert_eq!(card.grant.is_some(), granted);
        assert_eq!(client.opens, if expected == OnlineCheck::Ready || expected == OnlineCheck::CheckAgain { 3 } else { 2 });
        if let Some(grant) = card.grant {
            let cold = ReturnAuthorization::restore(&grant.receipt(), &source, &target, channel).unwrap();
            assert!(cold.matches(&source, &target, channel));
            assert!(!card.regular, "return uses the verified receipt path");
        }
    }
}
