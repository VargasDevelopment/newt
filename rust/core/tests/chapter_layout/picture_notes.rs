use super::*;
use newt_core::chapter_layout::engine::StartAttrs;

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
fn missing_or_blank_alt_says_picture() {
    for alt in [None, Some(&b""[..]), Some(&b" \t\n "[..])] {
        let (sink, _) = layout(true, |e| img(e, alt));
        let lines = lines(&sink);
        assert_eq!(lines.len(), 1, "alt {alt:?}");
        assert_eq!(lines[0].1, "[Picture]");
        assert!(lines[0].2.iter().all(|f| f & ITALIC != 0));
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

#[test]
fn notes_take_no_chapter_positions() {
    // Pages hold 5 lines. The note starts on page 0 and runs over the break,
    // so page 1 starts inside it. That page must start at the note's position
    // (39, right after the first paragraph), later pages must not go back,
    // and the chapter total must not change: saved places stay valid.
    let book = |e: &mut Engine<'_>| {
        para(e, b"aaaa bbbb cccc dddd eeee ffff gggg hhhh");
        img(e, Some(b"one two three four five six seven eight nine ten eleven twelve"));
        para(e, b"iiii jjjj kkkk llll mmmm nnnn oooo pppp qqqq rrrr");
    };
    let (off, off_chars) = layout_in(false, 40, book);
    let (on, on_chars) = layout_in(true, 40, book);
    assert_eq!(on_chars, off_chars);
    assert!(lines(&off).iter().all(|(_, t, _)| !t.contains("Picture")));
    let on_lines = lines(&on);
    assert_eq!(on_lines[2], (0, "[Picture: one two th".into(), vec![ITALIC]));
    assert_eq!(on_lines[4].0, 1, "the note should run over to page 1");
    let starts: Vec<_> = on.pages.iter().map(|p| p.1).collect();
    assert_eq!(starts[1], 39, "a page starting inside the note takes its position");
    assert!(starts.windows(2).all(|w| w[0] <= w[1]), "page starts go back: {starts:?}");
    assert!(starts.iter().all(|&s| s <= on_chars));
}
