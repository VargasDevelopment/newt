extern crate std;
use super::*;
use std::format;

const RELEASE: &str = r#"{"tag_name":"v9.0.0","author":{"assets":[]},"assets":[
 {"name":"newt.manifest","size":120,"browser_download_url":"https://example.invalid/newt.manifest"},
 {"name":"newt.bin","size":4096,"browser_download_url":"https://example.invalid/newt.bin"}]}"#;

fn parse(text: &str, chunk: usize) -> Option<ReleaseJson> {
    let mut release = ReleaseJson::default();
    for bytes in text.as_bytes().chunks(chunk) {
        if !release.feed(bytes) { return None; }
    }
    release.complete().then_some(release)
}

fn url(region: &[u8]) -> &[u8] {
    &region[..region.iter().position(|&b| b == 0).unwrap()]
}

#[test]
fn a_one_release_list_reads_as_that_release() {
    let listed = format!("[ {RELEASE} ]");
    for chunk in [1, 7, 4096] {
        let (alone, listed) = (parse(RELEASE, chunk).unwrap(), parse(&listed, chunk).unwrap());
        for release in [&alone, &listed] {
            let assets = release.assets();
            assert_eq!(url(&assets.manifest), b"https://example.invalid/newt.manifest");
            assert_eq!(url(&assets.image), b"https://example.invalid/newt.bin");
            assert_eq!((assets.image_size, assets.repeated), (4096, false));
        }
    }
}

#[test]
fn any_other_list_is_refused() {
    let two = format!("[{RELEASE},{RELEASE}]");
    let nested = format!("[[{RELEASE}]]");
    for text in ["[]", "[ ]", r#"["v9.0.0"]"#, "[1]", "[null]", two.as_str(), nested.as_str()] {
        assert!(parse(text, 1).is_none(), "{text}");
    }
    // An unclosed list never completes.
    assert!(parse(&format!("[{RELEASE}"), 1).is_none());
}

#[test]
fn test_builds_ask_the_same_repository_for_its_newest_release() {
    assert_eq!(TestBuilds::default().release_url(), LATEST_RELEASE_URL);
    let latest = LATEST_RELEASE_URL.to_bytes();
    let newest = TestBuilds::On.release_url().to_bytes();
    let repository = &latest[..latest.len() - b"/latest".len()];
    assert_eq!(newest, [repository, b"?per_page=1"].concat());
}

#[test]
fn a_test_build_ranks_between_its_neighbours() {
    let order = |a: &CStr, b: &CStr| semver2_compare(Some(a), Some(b));
    let test = c"0.2.2-dev.20261123.1";
    assert_eq!(order(test, c"0.2.1"), 1);
    assert_eq!(order(test, c"0.2.2"), -1);
    assert_eq!(order(c"0.2.2-dev.20261123.2", test), 1);
    assert_eq!(order(c"0.2.2-dev.20261124.1", c"0.2.2-dev.20261123.9"), 1);
    // Numeric identifiers compare as numbers, not text.
    assert_eq!(order(c"0.2.2-dev.20261123.10", c"0.2.2-dev.20261123.9"), 1);
}

#[test]
fn a_test_build_version_fits_the_manifest() {
    // Manifest versions are at most 23 characters (installer, seen-build record).
    let manifest = |version: &str| format!(
        "format=newt-update-1\nkind=newt\nversion={version}\nbuild=0123456789ab\nsize=4096\nsha256={}\n",
        "0".repeat(64));
    let mut parsed = UpdateManifest::default();
    for version in ["0.2.2-dev.20261123.1", "1.20.30-dev.20261123.99"] {
        let text = manifest(version);
        assert!(parse_update_manifest(Some(text.as_bytes()), text.len(), &mut parsed), "{version}");
    }
    let long = manifest("0.2.2-nightly.20261123.1");
    assert!(!parse_update_manifest(Some(long.as_bytes()), long.len(), &mut parsed));
}
