// SPDX-License-Identifier: MIT
//! #179: every non-decorative picture takes one chapter position, so pages of
//! picture notes reopen through the real opener like any other page. Spines of
//! the generated book (check.py `write_pictures`): 0 only pictures, 1 opens
//! with more than a page of pictures, 2 has three non-decorative pictures
//! (and one alt="" picture) before long text.
use super::*;
use boot_resume::open_page;
use newt_core::progress::Locator;
use reader_restore_policy::{DurableCheckpoint, boot_restore_policy};

pub fn check(bytes: Vec<u8>) {
    let mut tokens = [0u8; 2];
    // SAFETY: the existing host Board fake treats these as disjoint tokens.
    let (mut board, pending, _) = unsafe { newt_c3_board::Board::acquire_x3_optional_sd(
        (&mut tokens[0] as *mut u8).cast(), (&mut tokens[1] as *mut u8).cast()) }.unwrap();
    drop(pending);
    let mut digest = [0; 32];
    assert!(hash(&bytes, &mut digest));
    let book = u64::from_le_bytes(digest[..8].try_into().unwrap());
    let at = |spine, character| Progress { book, locator: Locator { spine, character }, ..Progress::default() };
    let settings = ViewSettings::default();
    let cold = boot_restore_policy(Some(DurableCheckpoint::PageStart), None, settings);
    board.with_reader_io(|panel, io| {
        let mut card = card::Card::default();
        let mut open = |saved, policy| open_page(panel, io, &mut card, &bytes, settings, saved, policy);
        // Each page's start, found by opening every position (short chapters).
        // A new start must be the next physical page, and the walk must reach
        // every page, so no page hides behind a shared start.
        let mut starts = |spine| {
            let mut starts: Vec<u32> = Vec::new();
            let mut character = 0;
            let mut count = 0;
            while let Some(page) = open(at(spine, character), RestorePolicy::Validated) {
                if starts.last() != Some(&page.char_start) {
                    assert_eq!(page.page as usize, starts.len(), "spine {spine}: page {} unreachable", starts.len());
                    starts.push(page.char_start);
                }
                count = page.page_count;
                character += 1;
                if character >= page.total_chars { break; }
            }
            assert_eq!(starts.len(), count as usize, "spine {spine}: pages share a start: {starts:?}");
            starts
        };
        let only = starts(0);
        let opens = starts(1);
        drop(starts);

        // A chapter of only pictures has positions: every page, the last one
        // included, reopens exactly, cold and warm.
        assert!(only.len() >= 2, "picture-only chapter should span pages: {only:?}");
        for &start in &only {
            for policy in [RestorePolicy::ExactPage, cold] {
                let page = open(at(0, start), policy).unwrap_or_else(|| panic!("picture page {start} refused ({policy:?})"));
                assert_eq!(page.char_start, start, "picture page {start} ({policy:?})");
                assert!(page.total_chars > start);
            }
        }

        // A chapter opening with a page of notes opens on its first page.
        assert!(opens.len() >= 2 && opens[1] > 0, "the first page of notes shares a locator: {opens:?}");
        for policy in [RestorePolicy::Validated, RestorePolicy::ExactPage, cold] {
            let page = open(at(1, 0), policy).expect("chapter start refused");
            assert_eq!((page.page, page.char_start), (0, 0), "chapter start skips its notes ({policy:?})");
        }

        // A place saved before the upgrade counted no pictures, so it sits 3
        // positions (spine 2's 3 pictures) before its text now. It reopens on
        // the page that holds that text.
        let page = open(at(2, 1500), RestorePolicy::Validated).expect("a page after the pictures");
        let text = page.char_start + 40;
        assert_eq!(open(at(2, text), RestorePolicy::Validated).map(|p| p.char_start), Some(page.char_start));
        let shown = open(at(2, text - 3), cold).expect("pre-upgrade place refused");
        assert_eq!(shown.char_start, page.char_start, "pre-upgrade place moved off its page");
    }).unwrap();
    println!("PASS picture pages reopen: picture-only chapter, a chapter opening with notes, pre-upgrade places");
}
