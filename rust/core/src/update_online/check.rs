use super::*;

/// One frozen workflow over required borrowed capabilities; caller owns all data.
pub fn fetch_online_update(http: &mut dyn Http, card: &mut dyn CardDownload, hash: &mut dyn Hash, clock: &mut dyn Clock, progress: &mut dyn Progress, work: &mut OnlineWork<'_>, test_builds: TestBuilds, running: Option<&crate::update_card::slot_installer::identity::NewtAppIdentity>, manifest: &mut UpdateManifest) -> OnlineCheck {
    fetch_online_update_with_http(http, card, hash, clock,
        &mut |_, done, total| progress.report(done, total), work, test_builds, running, manifest)
}

/// Same workflow and report sites, with the concrete client's short loan.
pub fn fetch_online_update_with_http<H: Http + ?Sized>(http: &mut H, card: &mut dyn CardDownload, hash: &mut dyn Hash, clock: &mut dyn Clock, progress: &mut dyn FnMut(&mut H, u32, u32), work: &mut OnlineWork<'_>, test_builds: TestBuilds, running: Option<&crate::update_card::slot_installer::identity::NewtAppIdentity>, manifest: &mut UpdateManifest) -> OnlineCheck {
    let mut head = HttpHead::default();
    copy_url(work.url, test_builds.release_url());
    http.limit(30000, clock);
    if !open_following(http, clock, work.url, work.location, true, &mut head) { return OnlineCheck::Unreachable; }
    if head.status != 200 { http.close(); return if head.status == 404 { OnlineCheck::NoRelease } else { OnlineCheck::Unreachable }; }
    let end = read_body_with_http(http, clock, work.buffer, 512 * 1024, &mut |_, bytes| work.release.feed(bytes));
    http.close();
    if end != BodyEnd::Whole || !work.release.complete() { return OnlineCheck::Unreachable; }
    let assets = work.release.assets();
    if assets.repeated { return OnlineCheck::WrongFile; }
    if assets.manifest[0] == 0 || assets.image[0] == 0 { return OnlineCheck::NoRelease; }
    http.limit(15000, clock);
    // ReleaseJson's output regions always have a terminator, including stale tails.
    let Ok(address) = CStr::from_bytes_until_nul(&assets.manifest) else { return OnlineCheck::Unreachable };
    if !copy_url(work.url, address) || !open_following(http, clock, work.url, work.location, false, &mut head) { return OnlineCheck::Unreachable; }
    if head.status != 200 { http.close(); return OnlineCheck::Unreachable; }
    *work.manifest_length = 0;
    let end = read_body_with_http(http, clock, work.buffer, crate::update_manifest::MAX_BYTES as u32, &mut |_, bytes| {
        let at = *work.manifest_length;
        work.manifest[at..at + bytes.len()].copy_from_slice(bytes);
        *work.manifest_length += bytes.len(); true
    });
    http.close();
    if end == BodyEnd::TooLong { return OnlineCheck::WrongFile; }
    if end != BodyEnd::Whole { return OnlineCheck::Unreachable; }
    if !parse_update_manifest(Some(work.manifest), *work.manifest_length, manifest) || manifest.kind != UpdateKind::Newt ||
        manifest.size > USABLE_SLOT_BYTES || manifest.size < SLOT_HEAD_BYTES ||
        (assets.image_size >= 0 && assets.image_size != i64::from(manifest.size)) { return OnlineCheck::WrongFile; }
    let version = CStr::from_bytes_until_nul(&manifest.version).ok();
    if test_builds == TestBuilds::Off && !crate::update_manifest::semver2_is_stable(version) {
        return OnlineCheck::WrongFile;
    }
    let running_version = running.and_then(|source| CStr::from_bytes_until_nul(&source.version).ok());
    let returning = match running.map(|source| crate::official_return::ReturnAuthorization::from_check(source, manifest, test_builds)).transpose() {
        Ok(value) => value,
        Err(crate::official_return::ReturnIssue::Incompatible) => return OnlineCheck::ReturnIncompatible,
        Err(crate::official_return::ReturnIssue::CheckAgain) => None,
    };
    if semver2_compare(version, running_version) <= 0 && returning.is_none() {
        return OnlineCheck::Latest;
    }
    for tries in 1..=3 {
        progress(http, 0, manifest.size);
        http.limit(300000, clock);
        let Ok(address) = CStr::from_bytes_until_nul(&assets.image) else { return OnlineCheck::Unreachable };
        if !copy_url(work.url, address) || !open_following(http, clock, work.url, work.location, false, &mut head) { return OnlineCheck::Unreachable; }
        if head.status != 200 || (head.length >= 0 && head.length != i64::from(manifest.size)) {
            http.close(); return if head.status == 200 { OnlineCheck::WrongFile } else { OnlineCheck::Unreachable };
        }
        if !card.begin() { http.close(); return OnlineCheck::CardFailed; }
        let mut written = true;
        let mut hashed = hash.start();
        let mut done = 0;
        let end = read_body_with_http(http, clock, work.buffer, manifest.size, &mut |http, bytes| {
            written = card.write(bytes);
            hashed = hashed && hash.update(bytes);
            done += bytes.len() as u32;
            progress(http, done, manifest.size);
            written
        });
        http.close();
        let mut digest = [0;32];
        let outcome = if !written { OnlineCheck::CardFailed }
            else if end == BodyEnd::TooLong { OnlineCheck::WrongFile }
            else if end != BodyEnd::Whole || done != manifest.size || !hashed || !hash.finish(&mut digest) { OnlineCheck::Interrupted }
            else if digest != manifest.sha256 { OnlineCheck::WrongFile } else { OnlineCheck::Ready };
        if outcome == OnlineCheck::Ready {
            let bytes = &work.manifest[..*work.manifest_length];
            return if let Some(authorization) = returning.as_ref() {
                if card.finish_return(bytes, authorization) { OnlineCheck::Ready } else { OnlineCheck::CheckAgain }
            } else if card.finish(bytes) { OnlineCheck::Ready } else { OnlineCheck::CardFailed };
        }
        card.abort();
        if outcome != OnlineCheck::CardFailed || tries == 3 { return outcome; }
    }
    OnlineCheck::CardFailed
}
