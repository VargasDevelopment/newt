use newt_reader_navigation::*;
use std::io::{self, Read};
#[derive(Default)]
struct Input {
    spines: Vec<(Option<Vec<u8>>, Option<Vec<u8>>)>,
    members: Vec<(Vec<u8>, Member)>,
    toc: Vec<(Vec<u8>, Vec<u8>, Vec<u8>, u8)>,
    selected: u32,
    required: u16,
    page: u32,
    focus: u32,
    anchors: Vec<(u32, u32)>,
}
impl BookView for Input {
    fn spine_count(&self) -> usize {
        self.spines.len()
    }
    fn spine_item(&self, i: usize) -> Option<Item<'_>> {
        self.spines.get(i).map(|(h, m)| Item {
            href: h.as_deref(),
            media: m.as_deref(),
        })
    }
    fn toc_count(&self) -> usize {
        self.toc.len()
    }
    fn toc_entry(&self, i: usize) -> Option<Declaration<'_>> {
        self.toc.get(i).map(|(t, h, f, d)| Declaration {
            title: t,
            href: h,
            fragment: f,
            depth: *d,
        })
    }
    fn member(&self, h: &[u8]) -> Option<Member> {
        self.members
            .iter()
            .find(|(name, _)| name == h)
            .map(|(_, e)| *e)
    }
}
fn number(s: &str, max: u64) -> Result<u64, ()> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0'))
    {
        return Err(());
    }
    s.parse::<u64>()
        .map_err(|_| ())
        .and_then(|n| if n <= max { Ok(n) } else { Err(()) })
}
fn decode(s: &str) -> Result<Vec<u8>, ()> {
    if s == "-" {
        return Ok(Vec::new());
    }
    if s.len() > 2048
        || s.is_empty()
        || s.len() % 2 != 0
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(());
    }
    let b: Result<Vec<u8>, _> = s
        .as_bytes()
        .chunks_exact(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).map_err(|_| ())?, 16).map_err(|_| ()))
        .collect();
    let b = b?;
    if b.contains(&0) { Err(()) } else { Ok(b) }
}
fn optional(s: &str) -> Result<Option<Vec<u8>>, ()> {
    if s == "n" {
        Ok(None)
    } else {
        decode(s).map(Some)
    }
}
fn hex(s: &[u8]) -> String {
    if s.is_empty() {
        "-".into()
    } else {
        s.iter().map(|b| format!("{b:02x}")).collect()
    }
}
fn words<'a>(
    lines: &mut impl Iterator<Item = &'a str>,
    kind: &str,
    count: usize,
) -> Result<Vec<&'a str>, ()> {
    let w: Vec<_> = lines.next().ok_or(())?.split(' ').collect();
    if w.len() != count || w[0] != kind {
        Err(())
    } else {
        Ok(w)
    }
}
fn parse<'a>(lines: &mut impl Iterator<Item = &'a str>) -> Result<Input, ()> {
    let h = words(lines, "N1", 10)?;
    let ns = number(h[1], 128)? as usize;
    let nz = number(h[2], 128)? as usize;
    let nt = number(h[3], 128)? as usize;
    if ns == 0 && (nz != 0 || nt != 0) {
        return Err(());
    }
    let mut input = Input {
        selected: number(h[4], u64::from(u32::MAX))? as u32,
        required: number(h[5], u64::from(u16::MAX))? as u16,
        page: number(h[6], u64::from(u32::MAX))? as u32,
        focus: number(h[7], u64::from(u32::MAX))? as u32,
        ..Input::default()
    };
    let na = number(h[8], 256)? as usize;
    if h[9] != "0" {
        return Err(());
    }
    for _ in 0..ns {
        let w = words(lines, "S", 3)?;
        let href = optional(w[1])?;
        if href.as_deref().is_some_and(|name| {
            [
                b"META-INF/container.xml".as_slice(),
                b"OPS/meta.opf",
                b"OPS/nav.xhtml",
            ]
            .contains(&name)
        }) {
            return Err(());
        }
        input.spines.push((href, optional(w[2])?));
    }
    for _ in 0..nz {
        let w = words(lines, "Z", 4)?;
        let name = decode(w[1])?;
        if name.is_empty()
            || [
                b"META-INF/container.xml".as_slice(),
                b"OPS/meta.opf",
                b"OPS/nav.xhtml",
            ]
            .contains(&name.as_slice())
        {
            return Err(());
        }
        input.members.push((
            name,
            Member {
                method: number(w[2], u64::from(u16::MAX))? as u16,
                uncompressed: number(w[3], u64::from(u32::MAX))? as u32,
            },
        ));
    }
    for _ in 0..nt {
        let w = words(lines, "T", 5)?;
        input.toc.push((
            decode(w[1])?,
            decode(w[2])?,
            decode(w[3])?,
            number(w[4], 255)? as u8,
        ));
    }
    for _ in 0..na {
        let w = words(lines, "A", 3)?;
        input.anchors.push((
            number(w[1], u64::from(u32::MAX))? as u32,
            number(w[2], u64::from(u32::MAX))? as u32,
        ));
    }
    words(lines, "END", 1)?;
    Ok(input)
}
fn section(tag: &str, s: Section<'_>) {
    println!(
        "{tag} {} {} {} {} {} {} {} {} {} {} {}",
        s.toc,
        s.spine,
        s.char_start,
        s.depth,
        u8::from(s.generic),
        u8::from(s.inherited),
        u8::from(s.target_known),
        u8::from(s.anchor_known),
        hex(s.label),
        hex(s.href),
        hex(s.fragment)
    );
}
fn run(input: &Input) {
    let mut nav = Navigation::default();
    nav.stage_required_toc(input.required);
    let bound = nav.bind(input, input.selected);
    let initial = nav.issues();
    nav.begin_anchors();
    for &(hash, start) in &input.anchors {
        nav.observe_anchor(hash, start);
    }
    let p = nav.progress_inputs();
    println!("B {} {initial}", u8::from(bound));
    println!(
        "P {} {} {} {} {} {}",
        p.before,
        p.current,
        p.total,
        p.index,
        p.count,
        u8::from(p.valid)
    );
    println!("G {} {}", nav.issues(), nav.target_count());
    for i in 0..nav.target_count() {
        section("T", nav.target(i));
    }
    let current = nav.section(input.page);
    section("S", current);
    let mut menu = Navigation::default();
    let mut mapping = vec![0; input.toc.len()];
    let bound = menu.bind_menu(input, current, input.focus, &mut mapping);
    let m = menu.menu_info();
    println!(
        "M {} {} {} {} {} {} {} {}",
        u8::from(bound),
        m.count,
        m.focus,
        m.first,
        m.visible,
        u8::from(m.authored),
        u8::from(m.current_exact),
        u8::from(m.valid)
    );
    for i in m.first..m.first + u32::from(m.visible) {
        let r = menu.menu_row(i);
        println!(
            "R {} {} {} {} {} {} {} {}",
            r.index,
            r.spine,
            r.toc,
            u8::from(r.generic),
            u8::from(r.fragment),
            u8::from(r.current),
            u8::from(r.valid),
            hex(r.label)
        );
    }
    let reused = menu.reuse_menu(current, 0);
    println!("U {} {}", u8::from(reused), menu.menu_info().focus);
    println!("END");
}
fn main() {
    let result = (|| {
        let mut raw = Vec::new();
        io::stdin()
            .take(2097153)
            .read_to_end(&mut raw)
            .map_err(|_| ())?;
        if raw.is_empty() || raw.len() > 2097152 || !raw.ends_with(b"\n") || raw.contains(&b'\r') {
            return Err(());
        }
        let text = std::str::from_utf8(&raw).map_err(|_| ())?;
        let mut lines = text[..text.len() - 1].split('\n').peekable();
        // Admit a complete batch before emitting any result, including later rows.
        let mut batch = Vec::new();
        while lines.peek().is_some() {
            batch.push(parse(&mut lines)?);
        }
        for input in batch {
            run(&input);
        }
        Ok(())
    })();
    if result.is_err() {
        std::process::exit(2);
    }
}
