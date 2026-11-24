use super::*;

fn linked_paragraph(e: &mut Engine<'_>, href: &[u8], text: &[u8]) {
    e.start(b"p", None, None, None, None, None);
    e.start(b"a", None, None, None, Some(href), None);
    e.text(text);
    e.end(b"a");
    e.end(b"p");
}

#[test]
fn justified_multi_word_link_shares_one_slot() {
    // Five words in a justified link exceed the four page link slots if each
    // segment takes its own slot; sharing keeps them in one rect covering all
    // five words while leaving the following unlinked word outside the rect.
    let mut sink = Capture::default();
    fixture(&mut sink, 256, |e| {
        e.params.width = 47;
        e.params.height = 40;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:justify"), None, None, None);
        e.start(b"a", None, None, None, Some(b"ch#multi"), None);
        e.text(b"a b c d e");
        e.end(b"a");
        e.text(b" zzzzz");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.start(b"a", None, None, None, Some(b"ch#styled"), None);
        e.text(b"x");
        e.start(b"b", None, None, None, None, None);
        e.text(b"y");
        e.end(b"b");
        e.text(b"z");
        e.end(b"a");
        e.end(b"p");
        assert!(e.finish());
    });
    assert_eq!(sink.links, [
        (0, b"ch".to_vec(), b"multi".to_vec()),
        (0, b"ch".to_vec(), b"styled".to_vec()),
    ]);
    let page0_runs = &sink.pages[0].2;
    assert_eq!(page0_runs[0].0, b"a");
    assert_eq!(page0_runs[4].0, b"e");
    assert_eq!(page0_runs[5].0, b"z");
    assert_eq!(page0_runs[5].2, 8);
    // Justified line 0 has 5 gaps and 3 px left over: a(0..4), b(9..13),
    // c(18..22), d(27..31), e(35..39), z(43..47).
    assert_eq!(sink.link_rects[0], (0, 0, 0, 39, 10));
    // Same-height inline bold merges x, y, and z into one rect (y 19..29).
    assert_eq!(sink.link_rects[1], (0, 0, 19, 12, 10));
}

#[test]
fn mixed_size_link_does_not_overlap_neighboring_line_link() {
    // Link A starts with a 250% glyph then normal text; link B sits on the
    // following line under A's normal text. Keeping distinct vertical bands in
    // separate slots prevents A's tall box from covering point (9, 11) in B.
    let mut direct = Capture::default();
    fixture(&mut direct, 256, |e| {
        e.params.paragraph_spacing = 0;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.start(b"a", None, None, None, Some(b"p#a"), None);
        e.start(b"span", None, Some(b"font-size:250%"), None, None, None);
        e.text(b"x");
        e.end(b"span");
        e.text(b"yyy");
        e.end(b"a");
        e.end(b"p");
        e.start(b"p", None, None, None, None, None);
        e.text(b"aa");
        e.start(b"a", None, None, None, Some(b"p#b"), None);
        e.text(b"b");
        e.end(b"a");
        e.end(b"p");
        assert!(e.finish());
    });
    assert_eq!(direct.links, [
        (0, b"p".to_vec(), b"a".to_vec()),
        (0, b"p".to_vec(), b"a".to_vec()),
        (0, b"p".to_vec(), b"b".to_vec()),
    ]);
    assert_eq!(direct.link_rects, [
        (0, 0, -12, 4, 25),
        (0, 4, 0, 12, 10),
        (0, 8, 8, 4, 10),
    ]);
    let hits = |px: i16, py: i16| -> Vec<&[u8]> {
        direct.links.iter().zip(&direct.link_rects)
            .filter(|(_, rect)| {
                let (_, x, y, w, h) = **rect;
                px >= x && px < x + w as i16 && py >= y && py < y + h as i16
            })
            .map(|((_, _, frag), _)| frag.as_slice())
            .collect()
    };
    assert_eq!(hits(9, 11), [b"b".as_slice()]);

    // Uniform-size justified words after the tall glyph still share one slot.
    let mut justified = Capture::default();
    fixture(&mut justified, 256, |e| {
        e.params.paragraph_spacing = 0;
        e.params.width = 47;
        e.params.height = 40;
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, Some(b"text-align:justify"), None, None, None);
        e.start(b"a", None, None, None, Some(b"p#a"), None);
        e.start(b"span", None, Some(b"font-size:250%"), None, None, None);
        e.text(b"x");
        e.end(b"span");
        e.text(b"yyy zzz");
        e.end(b"a");
        e.text(b" w wwww");
        e.end(b"p");
        assert!(e.finish());
    });
    assert_eq!(justified.links, [
        (0, b"p".to_vec(), b"a".to_vec()),
        (0, b"p".to_vec(), b"a".to_vec()),
    ]);
    assert_eq!(justified.link_rects[0], (0, 0, -12, 4, 25));
    assert_eq!(justified.link_rects[1].1, 4);
    assert_eq!(justified.link_rects[1].2, 0);
    assert_eq!(justified.link_rects[1].4, 10);
    assert!(justified.link_rects[1].3 > 28);
}

#[test]
fn link_slot_limit_moves_overflowing_lines_to_next_page() {
    // Six distinct links fit vertically on one tall page, but the page only has
    // four link slots. Entry 4 is a justified multi-word link occupying the
    // fourth slot (verifying merge still works at capacity); entries 5 and 6
    // must move to page 1 rather than being dropped silently.
    let mut sink = Capture::default();
    fixture(&mut sink, 512, |e| {
        e.params.width = 42;
        e.params.height = 120;
        e.start(b"body", None, None, None, None, None);
        for (href, label) in [(b"c#1", b"one"), (b"c#2", b"two"), (b"c#3", b"thr")] {
            linked_paragraph(e, href, label);
        }
        e.start(b"p", None, Some(b"text-align:justify"), None, None, None);
        e.start(b"a", None, None, None, Some(b"c#4"), None);
        e.text(b"aa bb cc");
        e.end(b"a");
        e.text(b" tail");
        e.end(b"p");
        for (href, label) in [(b"c#5", b"fiv"), (b"c#6", b"six")] {
            linked_paragraph(e, href, label);
        }
        assert!(e.finish());
    });
    assert_eq!(sink.pages.len(), 2);
    assert_eq!(sink.links, [
        (0, b"c".to_vec(), b"1".to_vec()),
        (0, b"c".to_vec(), b"2".to_vec()),
        (0, b"c".to_vec(), b"3".to_vec()),
        (0, b"c".to_vec(), b"4".to_vec()),
        (1, b"c".to_vec(), b"5".to_vec()),
        (1, b"c".to_vec(), b"6".to_vec()),
    ]);
    // Entry 4 was justified into separate word runs yet still merged into slot 4.
    let page0_texts: Vec<_> = sink.pages[0].2.iter().map(|run| run.0.as_slice()).collect();
    assert_eq!(page0_texts, [
        b"one".as_slice(), b"two", b"thr", b"aa", b"bb", b"cc", b"t", b"ail",
    ]);
    // Page 1 starts at y = 0 with the exact character offset after page 0.
    assert_eq!(sink.pages[1].1, 22);
    assert_eq!(sink.pages[1].2[0].0, b"fiv");
    assert_eq!(sink.link_rects[4], (1, 0, 0, 12, 10));
    assert_eq!(sink.link_rects[5], (1, 0, 11, 12, 10));

    // A line with two links overflowing a page that has only one slot left
    // must roll back its first link and move the entire line to the next page.
    let mut rollback = Capture::default();
    fixture(&mut rollback, 512, |e| {
        e.storage.links = &mut core::mem::take(&mut e.storage.links)[..2];
        e.params.height = 64;
        e.start(b"body", None, None, None, None, None);
        linked_paragraph(e, b"c#a", b"first");
        e.start(b"p", None, None, None, None, None);
        e.start(b"a", None, None, None, Some(b"c#b"), None);
        e.text(b"b");
        e.end(b"a");
        e.text(b" ");
        e.start(b"a", None, None, None, Some(b"c#c"), None);
        e.text(b"c");
        e.end(b"a");
        e.end(b"p");
        assert!(e.finish());
    });
    assert_eq!(rollback.links, [
        (0, b"c".to_vec(), b"a".to_vec()),
        (1, b"c".to_vec(), b"b".to_vec()),
        (1, b"c".to_vec(), b"c".to_vec()),
    ]);
    assert_eq!(rollback.pages[0].2.len(), 1);
    assert_eq!(rollback.pages[1].1, 5);
}

#[test]
fn a_line_with_more_links_than_an_empty_page_fails_without_a_page() {
    let mut sink = Capture { stop: true, ..Capture::default() };
    fixture(&mut sink, 256, |e| {
        e.storage.links = &mut core::mem::take(&mut e.storage.links)[..2];
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        for (href, label) in [(b"p#1", b"a"), (b"p#2", b"b"), (b"p#3", b"c")] {
            e.start(b"a", None, None, None, Some(href), None);
            e.text(label);
            e.end(b"a");
        }
        e.end(b"p");
        assert!(!e.finish());
        assert!(e.failed);
    });
    assert!(sink.pages.is_empty());
}

#[test]
fn non_adjacent_and_wrapped_link_segments_keep_separate_slots() {
    // A wrapped link gets one rect per line, and the second line moves to the
    // next page when the first line used the page's last link slot.
    let mut wrapped = Capture::default();
    fixture(&mut wrapped, 256, |e| {
        e.storage.links = &mut core::mem::take(&mut e.storage.links)[..1];
        e.params.width = 12;
        e.params.height = 32;
        e.start(b"body", None, None, None, None, None);
        linked_paragraph(e, b"p#wrap", b"abcdef");
        assert!(e.finish());
    });
    assert_eq!(wrapped.links, [
        (0, b"p".to_vec(), b"wrap".to_vec()),
        (1, b"p".to_vec(), b"wrap".to_vec()),
    ]);
    assert_eq!(wrapped.link_rects, [
        (0, 0, 0, 12, 10),
        (1, 0, 0, 12, 10),
    ]);

    // BiDi reordering places unlinked " 34" visually between linked "12" and
    // linked "אב "; those non-adjacent segments must not merge across " 34".
    let mut bidi = Capture::default();
    fixture(&mut bidi, 256, |e| {
        e.start(b"body", None, None, None, None, None);
        e.start(b"p", None, None, None, None, None);
        e.start(b"a", None, None, None, Some(b"p#bidi"), None);
        e.text("אב 12".as_bytes());
        e.end(b"a");
        e.text(b" 34");
        e.end(b"p");
        assert!(e.finish());
    });
    assert_eq!(bidi.links, [
        (0, b"p".to_vec(), b"bidi".to_vec()),
        (0, b"p".to_vec(), b"bidi".to_vec()),
    ]);
    // Visual order in RTL right-aligned line of width 80 (8 glyphs * 4 = 32 px,
    // x starts at 48): "12" (48..56), " 34" (56..68), "אב " reversed (68..80).
    assert_eq!(bidi.link_rects, [
        (0, 48, 0, 8, 10),
        (0, 68, 0, 12, 10),
    ]);
}
