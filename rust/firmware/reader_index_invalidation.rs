// SPDX-License-Identifier: MIT
//! A failed deferred digest revokes its derived index as well as source trust.
use super::session::Session;
use crate::reader_verification::Verification;
use newt_native_storage::{Authority, Native, chapter_cache::ChapterCacheStorage};

#[inline(never)]
pub(super) fn invalidate_failed_index<N: Native>(session:&mut Session,native:&mut N,
    digest:&[u8;32],cancel:&dyn Authority,now:fn()->u32)->bool {
    if !matches!(session.verification.state(),Verification::Changed|Verification::Failed) { return true; }
    if session.verification.index_revoked() { return true; }
    let route=session.route.lend(cancel,now);
    let Ok(mut pages)=ChapterCacheStorage::lend(&mut session.pages,native,&route) else { return false; };
    let invalidated=pages.invalidate_index(digest);
    if invalidated { session.verification.mark_index_revoked(); }
    invalidated
}
