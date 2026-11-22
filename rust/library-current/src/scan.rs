//! Card traversal and external run merge for the Books catalog.
use crate::{
    catalog::*,
    library_name::name_bytes,
    list::{self, Files},
};
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum ListState {
    #[default]
    None,
    Kept,
    Built,
    Ram,
}
#[derive(Default, Debug)]
pub struct Report {
    pub entries: u32,
    pub folders: u16,
    pub compared: u16,
    pub walks: u8,
    pub list: ListState,
    pub runs: u16,
}
pub enum Read {
    Entry {
        name: Name,
        bytes: u32,
        folder: bool,
        hidden: bool,
    },
    End,
    Failed,
}
/// Native BSP only enumerates folders; Rust owns traversal and all policy.
/// Entry names are NUL terminated (FAT single component); >95-byte components
/// use `LongEntry`, preserving folder/name omission without truncation aliases.
pub enum EntryRead {
    Short(Read),
    LongEntry {
        folder: bool,
        hidden: bool,
        epub: bool,
        dot: bool,
    },
}
pub trait Folders {
    fn open(&mut self, level: usize, path: &[u8]) -> bool;
    fn next(&mut self, level: usize, path: &[u8]) -> EntryRead;
    fn close(&mut self, level: usize) -> bool;
}
/// Native monotonic clock and the actual scan callback. No cancellation hook.
pub trait Progress {
    fn milliseconds(&mut self) -> u32;
    fn progress(&mut self, elapsed: u32);
}
pub struct NoProgress;
impl Progress for NoProgress {
    fn milliseconds(&mut self) -> u32 {
        0
    }
    fn progress(&mut self, _: u32) {}
}
struct Elapsed<'a> {
    native: &'a mut dyn Progress,
    began: u32,
}
impl Progress for Elapsed<'_> {
    fn milliseconds(&mut self) -> u32 {
        self.native.milliseconds()
    }
    fn progress(&mut self, _: u32) {
        let elapsed = self.native.milliseconds().wrapping_sub(self.began);
        self.native.progress(elapsed);
    }
}
fn pace(p: &mut dyn Progress, n: u32) {
    if n % 64 == 0 {
        p.progress(0);
    }
}
struct WalkResult {
    spilled: u32,
    outside: u32,
    print: u64,
    spill_ok: bool,
}
fn append(out: &mut [u8], prefix: &[u8], suffix: &[u8]) -> bool {
    let n = prefix.len() + suffix.len();
    if n >= out.len() {
        return false;
    }
    out[..prefix.len()].copy_from_slice(prefix);
    out[prefix.len()..n].copy_from_slice(suffix);
    out[n] = 0;
    true
}
fn path(root: &[u8], leaf: &[u8]) -> Option<[u8; 192]> {
    let mut p = [0; 192];
    if append(&mut p, root, leaf) {
        Some(p)
    } else {
        None
    }
}
// The owned bytes are Build's actual paths, not an allocation probe.
fn build_paths(root: &[u8]) -> Option<alloc::vec::Vec<u8>> {
    let mut storage = crate::zeroed(3 * 192)?;
    let (paths, remainder) = storage.as_chunks_mut::<192>();
    assert!(remainder.is_empty());
    for (out, suffix) in paths.iter_mut().zip([
        b"/Newt/Cache/Library/books.nlb".as_slice(),
        b"/Newt/Cache/Library/books.s0",
        b"/Newt/Cache/Library/books.s1",
    ]) {
        if !append(out, root, suffix) {
            return None;
        }
    }
    Some(storage)
}
fn walk(
    root: &[u8],
    catalog: &mut Catalog,
    seen: &mut Report,
    folders: &mut dyn Folders,
    files: &mut dyn Files,
    proof: &mut Option<&mut dyn FnMut(&[u8], &[u8]) -> bool>,
    p: &mut dyn Progress,
    listing: bool,
    spill: Option<i32>,
) -> Option<WalkResult> {
    seen.walks += 1;
    // Allocate before resetting counters or opening folders, as alpha15 does.
    // All path lengths fit one byte under the checked 256-byte path bound.
    let Some(mut scratch) = crate::zeroed(256 + DEPTH + 1) else {
        catalog.reset(LibraryStatus::Unavailable);
        return None;
    };
    let (full, lengths) = scratch.split_at_mut(256);
    seen.entries = 0;
    seen.folders = 0;
    catalog.reset(LibraryStatus::Ok);
    append(full, root, b"");
    if !folders.open(0, full) {
        catalog.reset(LibraryStatus::Unavailable);
        return None;
    }
    lengths[0] = u8::try_from(root.len()).unwrap();
    seen.folders = 1;
    let mut open = 1;
    for part in [b"/Newt".as_slice(), b"/Books"] {
        let base = usize::from(lengths[open - 1]);
        full[base..base + part.len()].copy_from_slice(part);
        full[base + part.len()] = 0;
        if !folders.open(open, full) {
            break;
        }
        lengths[open] = u8::try_from(base + part.len()).unwrap();
        open += 1;
        seen.folders += 1;
    }
    let first = open;
    let mut failed = false;
    let mut compared = 0u16;
    let mut result = WalkResult {
        spilled: 0,
        outside: 0,
        print: list::BASIS,
        spill_ok: true,
    };
    let mut spill_ok = true;
    while open != 0 {
        let depth = open - 1;
        full[usize::from(lengths[depth])] = 0;
        let entry = folders.next(depth, full);
        let (name, bytes, folder, hidden) = match entry {
            EntryRead::Short(Read::Entry {
                name,
                bytes,
                folder,
                hidden,
            }) => (name, bytes, folder, hidden),
            EntryRead::Short(read) => {
                let closed = folders.close(depth);
                open -= 1;
                if depth == 0 {
                    failed = matches!(read, Read::Failed) || !closed;
                } else if matches!(read, Read::Failed) {
                    catalog.omit(LibraryOmission::Folder);
                }
                continue;
            }
            EntryRead::LongEntry {
                folder,
                hidden,
                epub,
                dot,
            } => {
                seen.entries += 1;
                if seen.entries > ENTRIES {
                    catalog.omit(LibraryOmission::Stopped);
                    break;
                }
                pace(p, seen.entries);
                let newt = depth == 1
                    && full[root.len()..usize::from(lengths[depth])].eq_ignore_ascii_case(b"/Newt");
                if !dot && !(folder && (hidden || newt)) {
                    if folder {
                        catalog.omit(LibraryOmission::Folder);
                    } else if epub && !newt {
                        catalog.omit(LibraryOmission::Name);
                    }
                }
                continue;
            }
        };
        seen.entries += 1;
        if seen.entries > ENTRIES {
            catalog.omit(LibraryOmission::Stopped);
            break;
        }
        pace(p, seen.entries);
        let n = name_bytes(&name).expect("native entry must be NUL terminated");
        if n.first() == Some(&b'.') {
            continue;
        }
        let relative = &full[root.len()..usize::from(lengths[depth])];
        let newt = depth == 1 && relative.eq_ignore_ascii_case(b"/Newt");
        let length = relative.len();
        if folder {
            if hidden || (newt && !n.eq_ignore_ascii_case(b"Books")) {
                continue;
            }
            if (depth == 0 && first > 1 && n.eq_ignore_ascii_case(b"Newt")) || (newt && first > 2) {
                continue;
            }
            let books = newt || (depth == 0 && n.eq_ignore_ascii_case(b"Newt"));
            if depth == DEPTH || n.contains(&b':') || (!books && length + 1 + n.len() + 8 > 96) {
                catalog.omit(LibraryOmission::Folder);
                continue;
            }
            let start = usize::from(lengths[depth]);
            full[start] = b'/';
            full[start + 1..start + 1 + n.len()].copy_from_slice(n);
            full[start + 1 + n.len()] = 0;
            if !folders.open(depth + 1, full) {
                catalog.omit(LibraryOmission::Folder);
                continue;
            }
            lengths[open] = u8::try_from(start + 1 + n.len()).unwrap();
            open += 1;
            seen.folders += 1;
            continue;
        }
        if n.len() <= 5 || !n[n.len() - 5..].eq_ignore_ascii_case(b".epub") || newt {
            continue;
        }
        if n.contains(&b':') {
            catalog.omit(LibraryOmission::Name);
            continue;
        }
        let mut card = [0; 96];
        let listed = if depth == 2 && relative.eq_ignore_ascii_case(b"/Newt/Books") {
            &name
        } else {
            if length + 1 + n.len() >= 96 {
                catalog.omit(LibraryOmission::Name);
                continue;
            }
            card[..length].copy_from_slice(relative);
            card[length] = b'/';
            for c in &mut card[..length + 1] {
                if *c == b'/' {
                    *c = b':';
                }
            }
            card[length + 1..length + 1 + n.len()].copy_from_slice(n);
            &card
        };
        let mut bounded = |a: &[u8], b: &[u8]| {
            if compared == PROOFS {
                return false;
            }
            let Some(same) = proof.as_mut() else {
                return false;
            };
            compared += 1;
            same(a, b)
        };
        if catalog.add(listed, bytes, Some(&mut bounded), listing) == Added::Beyond {
            result.print = list::hash(result.print, c_name(listed));
            result.spilled += 1;
            result.outside += u32::from(listed[0] == b':');
            if let Some(fd) = spill {
                if spill_ok {
                    spill_ok =
                        list::write_record(files, fd, list::at(0, result.spilled - 1), listed);
                }
            }
        }
    }
    while open != 0 {
        open -= 1;
        folders.close(open);
    }
    if failed {
        catalog.reset(LibraryStatus::Unavailable);
        return None;
    }
    seen.compared = compared;
    if catalog.finish() != LibraryStatus::Ok {
        return None;
    }
    let t = catalog.table();
    for n in &t.names[..t.count] {
        result.print = list::hash(result.print, c_name(n));
    }
    result.print = list::hash(
        result.print,
        &(t.count as u32 + result.spilled).to_le_bytes(),
    );
    // A failed spill must still finish traversal; caller then falls back.
    result.spill_ok = spill_ok;
    Some(result)
}
struct Records {
    fd: i32,
    base: u32,
    count: u32,
    ok: bool,
}
impl Records {
    fn new(fd: i32, base: u32) -> Self {
        Self {
            fd,
            base,
            count: 0,
            ok: fd >= 0,
        }
    }
    fn add(&mut self, f: &mut dyn Files, name: &[u8]) -> bool {
        self.ok = self.ok && list::write_record(f, self.fd, list::at(self.base, self.count), name);
        self.count += 1;
        self.ok
    }
}
fn close(f: &mut dyn Files, fd: i32, ok: &mut bool) {
    if fd >= 0 && !f.close(fd) {
        *ok = false;
    }
}
fn sort_runs(
    f: &mut dyn Files,
    t: &mut Table<'_>,
    scratch: &[[u8; 192]],
    spilled: u32,
    p: &mut dyn Progress,
) -> bool {
    let input = f.open(&scratch[0], false);
    let fd = f.open(&scratch[1], true);
    let mut out = Records::new(fd, 0);
    let mut ok = input >= 0;
    for i in 0..t.count {
        if ok {
            ok = out.add(f, &t.names[i]);
        }
    }
    let mut done = 0;
    while ok && done < spilled {
        let chunk = 128.min(spilled - done);
        for i in 0..chunk {
            if ok {
                ok = list::read_record(f, input, list::at(0, done + i), &mut t.names[i as usize]);
            }
        }
        sort(&mut t.names[..chunk as usize]);
        for i in 0..chunk {
            if ok {
                ok = out.add(f, &t.names[i as usize]);
            }
        }
        done += chunk;
        pace(p, done);
    }
    close(f, input, &mut ok);
    let closed = fd < 0 || f.close(fd);
    closed && ok && out.ok
}
fn cursor(t: &Table<'_>, run: usize) -> (u32, usize, usize) {
    let v = t.value(run);
    (v & 65535, ((v >> 16) & 255) as usize, (v >> 24) as usize)
}
fn store(t: &mut Table<'_>, run: usize, read: u32, at: usize, end: usize) {
    t.set_value(run, read | ((at as u32) << 16) | ((end as u32) << 24));
}
fn refill(
    f: &mut dyn Files,
    t: &mut Table<'_>,
    input: i32,
    total: u32,
    length: u32,
    group: u32,
    runs: u32,
    run: usize,
) -> bool {
    let slots = 128 / runs as usize;
    let (read, _, _) = cursor(t, run);
    let start = (group + run as u32) * length;
    let records = length.min(total - start);
    let count = (slots as u32).min(records - read);
    for i in 0..count {
        if !list::read_record(
            f,
            input,
            list::at(0, start + read + i),
            &mut t.names[run * slots + i as usize],
        ) {
            return false;
        }
    }
    store(t, run, read + count, 0, count as usize);
    true
}
fn merge(
    f: &mut dyn Files,
    t: &mut Table<'_>,
    input: i32,
    out: &mut Records,
    total: u32,
    length: u32,
    group: u32,
    runs: u32,
    p: &mut dyn Progress,
) -> bool {
    let slots = 128 / runs as usize;
    for run in 0..runs as usize {
        store(t, run, 0, 0, 0);
        if !refill(f, t, input, total, length, group, runs, run) {
            return false;
        }
    }
    loop {
        let mut least: Option<(usize, usize)> = None;
        for run in 0..runs as usize {
            let (_, at, end) = cursor(t, run);
            if at == end {
                continue;
            }
            let slot = run * slots + at;
            if least.is_none_or(|(_, prev)| before(&t.names[slot], &t.names[prev])) {
                least = Some((run, slot));
            }
        }
        let Some((run, slot)) = least else {
            return true;
        };
        if !out.add(f, &t.names[slot]) {
            return false;
        }
        pace(p, out.count);
        let (read, at, end) = cursor(t, run);
        store(t, run, read, at + 1, end);
        if at + 1 == end && !refill(f, t, input, total, length, group, runs, run) {
            return false;
        }
    }
}
fn pass(
    f: &mut dyn Files,
    t: &mut Table<'_>,
    from: &[u8],
    to: &[u8],
    total: u32,
    length: u32,
    p: &mut dyn Progress,
) -> bool {
    let input = f.open(from, false);
    let fd = f.open(to, true);
    let mut out = Records::new(fd, 0);
    let mut ok = input >= 0;
    let runs = total.div_ceil(length);
    let mut group = 0;
    while ok && group < runs {
        ok = merge(
            f,
            t,
            input,
            &mut out,
            total,
            length,
            group,
            128.min(runs - group),
            p,
        );
        group += 128;
    }
    close(f, input, &mut ok);
    let closed = fd < 0 || f.close(fd);
    closed && ok && out.ok
}
fn sweep(f: &mut dyn Files, scratch: &[[u8; 192]]) {
    f.remove(&scratch[0]);
    f.remove(&scratch[1]);
}
fn build(
    f: &mut dyn Files,
    t: &mut Table<'_>,
    list_path: &[u8],
    scratch: &[[u8; 192]],
    spilled: u32,
    print: u64,
    outside: u32,
    runs: &mut u16,
    p: &mut dyn Progress,
) -> bool {
    let total = t.count as u32 + spilled;
    *runs = total.div_ceil(128) as u16;
    let mut ok = spilled == 0 || (t.count == 128 && sort_runs(f, t, scratch, spilled, p));
    let mut input = 1;
    let mut length = 128;
    while ok && spilled != 0 && total.div_ceil(length) > 128 {
        ok = pass(f, t, &scratch[input], &scratch[input ^ 1], total, length, p);
        length *= 128;
        input ^= 1;
    }
    let fd = if ok { f.open(list_path, true) } else { -1 };
    let mut out = Records::new(fd, list::HEADER);
    ok = fd >= 0 && f.write(fd, 0, &[0; 32]);
    if spilled == 0 {
        for i in 0..t.count {
            if ok {
                ok = out.add(f, &t.names[i]);
            }
        }
    } else if ok {
        let from = f.open(&scratch[input], false);
        ok = from >= 0
            && merge(
                f,
                t,
                from,
                &mut out,
                total,
                length,
                0,
                total.div_ceil(length),
                p,
            );
        close(f, from, &mut ok);
    }
    ok = ok && out.ok && out.count == total && list::write_header(f, fd, total, outside, print);
    close(f, fd, &mut ok);
    sweep(f, scratch);
    if !ok {
        f.remove(list_path);
    }
    ok
}
/// `root` is a mounted UTF-8 C path, bounded exactly as alpha15; file callbacks
/// cannot reenter the borrowed catalog. `ram` is SdLibrary::keepReadable fallback.
pub fn scan(
    root: &[u8],
    catalog: &mut Catalog,
    seen: &mut Report,
    folders: &mut dyn Folders,
    f: &mut dyn Files,
    mut proof: Option<&mut dyn FnMut(&[u8], &[u8]) -> bool>,
    p: &mut dyn Progress,
    ram: bool,
) -> LibraryStatus {
    let began = p.milliseconds();
    let mut elapsed = Elapsed { native: p, began };
    let p: &mut dyn Progress = &mut elapsed;
    *seen = Report::default();
    catalog.reset(LibraryStatus::Ok);
    let Some(n) = root.iter().position(|&b| b == 0) else {
        catalog.reset(LibraryStatus::Unavailable);
        return catalog.status();
    };
    if n + 96 + 16 >= 256 {
        catalog.reset(LibraryStatus::Unavailable);
        return catalog.status();
    }
    let root = &root[..n];
    let Some(mut result) = walk(root, catalog, seen, folders, f, &mut proof, p, true, None) else {
        return catalog.status();
    };
    let mut paths = if ram { None } else { build_paths(root) };
    let Some(storage) = paths.as_mut() else {
        seen.list = ListState::Ram;
        if result.spilled != 0 {
            walk(root, catalog, seen, folders, f, &mut proof, p, false, None);
        }
        return catalog.status();
    };
    let (list_path, scratch) = storage.split_at_mut(192);
    let (scratch, remainder) = scratch.as_chunks_mut::<192>();
    assert!(remainder.is_empty());
    let total = catalog.size() as u32 + result.spilled;
    let outside = catalog.outside() + result.outside;
    if list::current(f, list_path, total, outside, result.print) {
        seen.list = ListState::Kept;
        sweep(f, scratch);
        return catalog.use_list(list_path, total, outside);
    }
    let mut built = true;
    for suffix in [b"/Newt".as_slice(), b"/Newt/Cache", b"/Newt/Cache/Library"] {
        if !built {
            break;
        }
        built = path(root, suffix).is_some_and(|path| f.make_folder(&path));
    }
    if built && result.spilled != 0 {
        let fd = f.open(&scratch[0], true);
        built = fd >= 0;
        if built {
            if let Some(second) = walk(
                root,
                catalog,
                seen,
                folders,
                f,
                &mut proof,
                p,
                true,
                Some(fd),
            ) {
                result = second;
                built = result.spill_ok;
            } else {
                built = false;
            }
        }
        close(f, fd, &mut built);
        if !built {
            f.remove(&scratch[0]);
        }
        if catalog.status() != LibraryStatus::Ok {
            return catalog.status();
        }
        // Allocate the next build paths before dropping the previous set,
        // even after failed spill I/O; failure falls through to the RAM rescan.
        paths = build_paths(root);
        built = built && paths.is_some();
    }
    let count = catalog.size() as u32 + result.spilled;
    let outside = catalog.outside() + result.outside;
    if built {
        let storage = paths.as_mut().unwrap();
        let (list_path, scratch) = storage.split_at_mut(192);
        let (scratch, remainder) = scratch.as_chunks_mut::<192>();
        assert!(remainder.is_empty());
        if build(
            f,
            &mut catalog.table(),
            list_path,
            scratch,
            result.spilled,
            result.print,
            outside,
            &mut seen.runs,
            p,
        ) {
            seen.list = ListState::Built;
            return catalog.use_list(list_path, count, outside);
        }
    }
    seen.list = ListState::Ram;
    if result.spilled != 0 {
        walk(root, catalog, seen, folders, f, &mut proof, p, false, None);
    }
    catalog.status()
}

/// The three native effects in SdLibrary::paceScan; policy stays in Rust.
pub trait PaceNative {
    fn milliseconds(&mut self) -> u32;
    fn delay_one_millisecond(&mut self);
    fn loading(&mut self);
}
pub struct FirmwareProgress<'a> {
    pub native: &'a mut dyn PaceNative,
    pub yielded: u32,
    pub loading_pending: bool,
}
impl Progress for FirmwareProgress<'_> {
    fn milliseconds(&mut self) -> u32 {
        self.native.milliseconds()
    }
    fn progress(&mut self, elapsed: u32) {
        if elapsed.wrapping_sub(self.yielded) >= 50 {
            self.native.delay_one_millisecond();
            self.yielded = elapsed;
        }
        if self.loading_pending && elapsed >= 500 {
            self.loading_pending = false;
            self.native.loading();
        }
    }
}
