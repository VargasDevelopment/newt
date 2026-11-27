use super::*;
use newt_core::{book_semantic_index::Counter, chapter_layout::engine::StartAttrs};

// FixtureImages finds no picture, as on SD books today, so every <img> here
// is skipped and gets a note when `picture_notes` is on.
fn img(e: &mut Engine<'_>, alt: Option<&[u8]>) {
    e.start_tag(b"img", StartAttrs { src: Some(b"pic.png"), alt, ..StartAttrs::default() });
    e.end(b"img");
}

fn para(e: &mut Engine<'_>, text: &[u8]) {
    e.start(b"p", None, None, None, None, None);
    e.text(text);
    e.end(b"p");
}

/// Each page's runs joined per line (by baseline), with the line's run flags.
fn lines(sink: &Capture) -> Vec<(u32, String, Vec<u8>)> {
    let mut out: Vec<(u32, i16, String, Vec<u8>)> = Vec::new();
    for (page, _, runs) in &sink.pages {
        for (text, _, baseline, _, flags) in runs {
            match out.last_mut() {
                Some((p, b, line, f)) if p == page && b == baseline => {
                    line.push_str(core::str::from_utf8(text).unwrap());
                    f.push(*flags);
                }
                _ => out.push((*page, *baseline, String::from_utf8(text.clone()).unwrap(), vec![*flags])),
            }
        }
    }
    out.into_iter().map(|(page, _, line, flags)| (page, line, flags)).collect()
}

fn layout(notes: bool, f: impl FnOnce(&mut Engine<'_>)) -> (Capture, u32) {
    layout_in(notes, 64, f)
}

/// Lines are 20 fixed-width characters, 8 px high; the breaker may split words.
fn layout_in(notes: bool, height: i16, f: impl FnOnce(&mut Engine<'_>)) -> (Capture, u32) {
    let mut sink = Capture::default();
    let mut chars = 0;
    fixture(&mut sink, 1024, |e| {
        e.params.picture_notes = notes;
        e.params.height = height;
        e.start(b"body", None, None, None, None, None);
        f(e);
        assert!(e.finish());
        chars = e.chars;
    });
    (sink, chars)
}

#[test]
fn alt_text_shows_in_italics_between_the_paragraphs() {
    let (sink, _) = layout(true, |e| {
        para(e, b"before");
        img(e, Some(b"  A map \n of the coast "));
        para(e, b"after");
    });
    let lines = lines(&sink);
    let texts: Vec<_> = lines.iter().map(|(_, t, _)| t.as_str()).collect();
    assert_eq!(texts, ["before", "[Picture: A map of t", "he coast]", "after"]);
    assert!(lines[1].2.iter().chain(&lines[2].2).all(|f| f & ITALIC != 0));
    assert!(lines[3].2.iter().all(|f| f & ITALIC == 0), "italic leaks past the note");
}

#[test]
fn missing_alt_says_picture_and_empty_alt_is_decorative() {
    let (sink, chars) = layout(true, |e| img(e, None));
    let lines = lines(&sink);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].1, "[Picture]");
    assert!(lines[0].2.iter().all(|f| f & ITALIC != 0));
    assert_eq!(chars, 1);
    for alt in [&b""[..], b" \t\n "] {
        let (sink, chars) = layout(true, |e| img(e, Some(alt)));
        assert!(sink.pages.is_empty(), "alt {alt:?} is decorative: no note");
        assert_eq!(chars, 0, "alt {alt:?} is decorative: no position");
    }
}

#[test]
fn pictures_in_a_row_get_one_note_each() {
    let (sink, _) = layout(true, |e| {
        img(e, Some(b"one"));
        img(e, None);
        img(e, Some(b"three"));
    });
    let texts: Vec<_> = lines(&sink).into_iter().map(|(_, t, _)| t).collect();
    assert_eq!(texts, ["[Picture: one]", "[Picture]", "[Picture: three]"]);
}

#[test]
fn a_linked_picture_keeps_its_link_and_a_figure_keeps_its_caption() {
    let (sink, _) = layout(true, |e| {
        e.start(b"a", None, None, None, Some(b"ch#map"), None);
        img(e, Some(b"Map"));
        e.end(b"a");
        e.start(b"figure", None, None, None, None, None);
        img(e, Some(b"Coast"));
        e.start(b"figcaption", None, None, None, None, None);
        e.text(b"Fig. 1");
        e.end(b"figcaption");
        e.end(b"figure");
    });
    assert_eq!(sink.links, [(0, b"ch".to_vec(), b"map".to_vec())]);
    let texts: Vec<_> = lines(&sink).into_iter().map(|(_, t, _)| t).collect();
    assert_eq!(texts, ["[Picture: Map]", "[Picture: Coast]", "Fig. 1"]);
}

/// Page starts and the chapter total for `pictures` pictures, then a paragraph.
fn starts_after(pictures: usize, alt: &'static [u8]) -> (Vec<u32>, u32, Capture) {
    let (sink, chars) = layout_in(true, 40, |e| {
        for _ in 0..pictures {
            img(e, Some(alt));
        }
        para(e, b"iiii jjjj kkkk");
    });
    (sink.pages.iter().map(|p| p.1).collect(), chars, sink)
}

/// The page a locator opens: the last one starting at or before it, as both
/// the live page search and the cached page lookup choose.
fn page_for(starts: &[u32], at: u32) -> usize {
    starts.iter().rposition(|&s| s <= at).unwrap()
}

#[test]
fn each_picture_takes_one_position() {
    // Pages hold 5 lines. The note starts on page 0 and runs over the break,
    // so page 1 starts inside it, at the note's own position (39, right after
    // the first paragraph). Text after the picture starts one position later.
    let book = |e: &mut Engine<'_>| {
        para(e, b"aaaa bbbb cccc dddd eeee ffff gggg hhhh");
        img(e, Some(b"one two three four five six seven eight nine ten eleven twelve"));
        para(e, b"iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr");
    };
    let (off, off_chars) = layout_in(false, 40, book);
    let (on, on_chars) = layout_in(true, 40, book);
    assert_eq!(on_chars, off_chars + 1);
    assert!(lines(&off).iter().all(|(_, t, _)| !t.contains("Picture")));
    let on_lines = lines(&on);
    assert_eq!(on_lines[2], (0, "[Picture: one two th".into(), vec![ITALIC]));
    assert_eq!(on_lines[4].0, 1, "the note should run over to page 1");
    let starts: Vec<_> = on.pages.iter().map(|p| p.1).collect();
    assert_eq!(starts[1], 39, "a page starting inside the note takes its position");
    assert!(starts.windows(2).all(|w| w[0] < w[1]), "pages share a locator: {starts:?}");
}

#[test]
fn a_chapter_that_opens_with_a_page_of_notes_opens_on_it() {
    let (starts, chars, sink) = starts_after(12, b"chart");
    assert!(starts.len() >= 3, "notes should fill more than a page: {starts:?}");
    assert!(starts.windows(2).all(|w| w[0] < w[1]), "pages share a locator: {starts:?}");
    assert_eq!(page_for(&starts, 0), 0, "opening the chapter skips its first notes");
    assert_eq!(lines(&sink)[0].1, "[Picture: chart]");
    assert!(starts.iter().all(|&s| s < chars));
}

#[test]
fn a_picture_only_chapter_has_positions_to_restore() {
    // Restore takes a locator only below the chapter total; every page here
    // must be reachable, including the last one.
    let (sink, chars) = layout_in(true, 40, |e| {
        for _ in 0..12 {
            img(e, None);
        }
    });
    let starts: Vec<_> = sink.pages.iter().map(|p| p.1).collect();
    assert_eq!(chars, 12);
    assert!(starts.len() >= 2, "{starts:?}");
    assert!(starts.windows(2).all(|w| w[0] < w[1]), "pages share a locator: {starts:?}");
    assert!(starts.iter().all(|&s| s < chars));
    for (page, &start) in starts.iter().enumerate() {
        assert_eq!(page_for(&starts, start), page);
    }
}

#[test]
fn the_note_is_italic_at_the_element_stack_limit() {
    // 16 element slots: body's 15 spans fill them, so the <img> overflows.
    let (sink, _) = layout(true, |e| {
        for _ in 0..15 {
            e.start(b"span", None, None, None, None, None);
        }
        img(e, Some(b"deep"));
        e.text(b"after");
        for _ in 0..15 {
            e.end(b"span");
        }
    });
    let lines = lines(&sink);
    assert_eq!(lines[0].1, "[Picture: deep]");
    assert!(lines[0].2.iter().all(|f| f & ITALIC != 0), "note lost its italics");
    assert!(lines[1].2.iter().all(|f| f & ITALIC == 0), "italic leaks past the note");
}

#[test]
fn a_long_alt_is_cut_on_a_character_boundary() {
    let long = "é".repeat(100);
    let PictureNote::Alt { text, cut: true } = PictureNote::of(Some(long.as_bytes())) else { panic!() };
    assert!(text.len() <= PictureNote::MAX_ALT && core::str::from_utf8(text).is_ok());
    let short = "a".repeat(PictureNote::MAX_ALT);
    assert_eq!(PictureNote::of(Some(short.as_bytes())), PictureNote::Alt { text: short.as_bytes(), cut: false });
}

/// One start tag as both the layout session and the index counter see it.
#[derive(Clone, Copy)]
enum Event {
    Text(&'static [u8]),
    Picture { name: &'static [u8], src: bool, alt: Option<&'static [u8]>, hidden: bool },
}

#[test]
fn layout_and_the_index_counter_agree_on_pictures() {
    use Event::*;
    let picture = |alt| Picture { name: b"img", src: true, alt, hidden: false };
    let book = [
        Text(b"a "), picture(None), Text(b" b "), picture(Some(b"Map")), Text(b"c"),
        picture(Some(b"")), Text(b" d"), picture(Some(b" \t ")), Text(b"e "),
        Picture { name: b"img", src: false, alt: Some(b"no source"), hidden: false },
        Picture { name: b"img", src: true, alt: Some(b"hidden"), hidden: true },
        Picture { name: b"image", src: true, alt: None, hidden: false }, Text(b"f"),
    ];
    let mut counter = Counter::default();
    counter.start_element(b"body", false, false, None);
    for event in book {
        match event {
            Text(t) => counter.text(t),
            Picture { name, src, alt, hidden } => {
                counter.start_element(name, hidden, src, alt);
                counter.end_element(name);
            }
        }
    }
    counter.end_element(b"body");
    let (_, chars) = layout(true, |e| {
        for event in book {
            match event {
                Text(t) => e.text(t),
                Picture { name, src, alt, hidden } => {
                    let inline = hidden.then_some(&b"display: none"[..]);
                    let src = src.then_some(&b"pic.png"[..]);
                    e.start_tag(name, StartAttrs { inline, src, alt, ..StartAttrs::default() });
                    e.end(name);
                }
            }
        }
    });
    // a b c d e f, plus [Picture], [Picture: Map] and the <image>.
    assert_eq!(counter.finish(), Some(chars));
    assert_eq!(chars, 9);
}
