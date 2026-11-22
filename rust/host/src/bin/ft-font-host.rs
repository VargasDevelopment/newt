#[path = "../../../firmware/fonts/native.rs"]
pub mod native;
use native::*;
use newt_core::ft_font::*;
use std::io::{self, Read};
struct Allocator {
    remaining: core::cell::Cell<usize>,
}
unsafe extern "C" {
    fn malloc(n: usize) -> *mut core::ffi::c_void;
    fn free(p: *mut core::ffi::c_void);
    fn realloc(p: *mut core::ffi::c_void, n: usize) -> *mut core::ffi::c_void;
}
unsafe fn admit(c: *mut core::ffi::c_void) -> bool {
    let a = unsafe { &*c.cast::<Allocator>() };
    let n = a.remaining.get();
    if n == 0 {
        return false;
    }
    if n != usize::MAX {
        a.remaining.set(n - 1);
    }
    true
}
unsafe extern "C" fn allocate(c: *mut core::ffi::c_void, n: usize) -> *mut core::ffi::c_void {
    if unsafe { admit(c) } {
        unsafe { malloc(n) }
    } else {
        core::ptr::null_mut()
    }
}
unsafe extern "C" fn deallocate(_: *mut core::ffi::c_void, p: *mut core::ffi::c_void) {
    unsafe {
        free(p);
    }
}
unsafe extern "C" fn reallocate(
    c: *mut core::ffi::c_void,
    p: *mut core::ffi::c_void,
    _: usize,
    n: usize,
) -> *mut core::ffi::c_void {
    if unsafe { admit(c) } {
        unsafe { realloc(p, n) }
    } else {
        core::ptr::null_mut()
    }
}
struct Bytes<'a> {
    data: &'a [u8],
    limit: &'a core::cell::Cell<usize>,
}
impl ReadAt for Bytes<'_> {
    fn read_at(&mut self, o: u64, b: &mut [u8]) -> usize {
        let Ok(o) = usize::try_from(o) else {
            return 0;
        };
        let Some(rest) = self.data.get(o..) else {
            return 0;
        };
        let n = b.len().min(rest.len()).min(self.limit.get());
        b[..n].copy_from_slice(&rest[..n]);
        n
    }
}
impl newt_core::zip_window::source::BookSource for Bytes<'_> {
    fn size(&self) -> u64 {
        self.data.len() as u64
    }
    fn read_at(&mut self, offset: u64, output: &mut [u8]) -> i32 {
        i32::try_from(ReadAt::read_at(self, offset, output)).unwrap_or(-1)
    }
}
fn number(s: &str) -> Option<i64> {
    if s.is_empty()
        || s == "-0"
        || s.starts_with('+')
        || s.bytes().any(|b| !b.is_ascii_digit() && b != b'-')
    {
        return None;
    }
    let n = s.parse::<i64>().ok()?;
    if n.to_string() != s {
        return None;
    }
    Some(n)
}
fn parse(input: &str) -> Option<(Vec<u8>, Vec<(String, Vec<i64>)>)> {
    if !input.ends_with('\n') || input.contains('\r') {
        return None;
    }
    let mut lines = input.lines();
    if lines.next()? != "FTFONT1" {
        return None;
    }
    let hex = lines.next()?;
    if hex.len() > 2 * 1024 * 1024
        || hex.is_empty()
        || hex.len() % 2 != 0
        || !hex
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return None;
    }
    let mut bytes = Vec::new();
    for p in (0..hex.len()).step_by(2) {
        bytes.push(u8::from_str_radix(&hex[p..p + 2], 16).ok()?);
    }
    let mut commands = Vec::new();
    for line in lines {
        let t: Vec<_> = line.split(' ').collect();
        let op = *t.first()?;
        let n = match op {
            "INIT" => 5,
            "OPTIONS" => 6,
            "METRIC" | "GMETRIC" | "RASTER" | "GRASTER" | "LIGA" | "BUDGET" => 2,
            "KERN" | "GKERN" | "LIGAG" => 3,
            "LINE" | "RELEASE" | "ALLOC" => 1,
            "INSPECT" => 2,
            "STATUS" | "DEINIT" => 0,
            _ => return None,
        };
        if t.len() != n + 1 {
            return None;
        }
        let a: Vec<i64> = t[1..].iter().map(|s| number(s)).collect::<Option<_>>()?;
        let valid = match op {
            "INIT" => {
                (0..=1).contains(&a[0])
                    && (0..=65535).contains(&a[1])
                    && i32::try_from(a[2]).is_ok()
                    && (0..=1).contains(&a[3])
                    && (0..=1048576).contains(&a[4])
            }
            "OPTIONS" => {
                (0..=4).contains(&a[0])
                    && (0..=255).contains(&a[1])
                    && (0..=1).contains(&a[2])
                    && (0..=1).contains(&a[3])
                    && i32::try_from(a[4]).is_ok()
                    && i32::try_from(a[5]).is_ok()
            }
            "BUDGET" => a.iter().all(|x| (0..=1048576).contains(x)),
            "RELEASE" => (0..=1).contains(&a[0]),
            "INSPECT" => (0..=1).contains(&a[0]) && (0..=4096).contains(&a[1]),
            _ => a.iter().all(|x| (0..=u32::MAX as i64).contains(x)),
        };
        if !valid {
            return None;
        }
        commands.push((op.to_owned(), a));
        if commands.len() > 4096 {
            return None;
        }
    }
    if commands.is_empty() {
        return None;
    }
    Some((bytes, commands))
}
fn main() {
    let mut input = String::new();
    if io::stdin()
        .take(4 * 1024 * 1024 + 1)
        .read_to_string(&mut input)
        .is_err()
        || input.len() > 4 * 1024 * 1024
    {
        std::process::exit(2);
    }
    let Some((bytes, commands)) = parse(&input) else {
        std::process::exit(2);
    };
    let allocator = Allocator {
        remaining: core::cell::Cell::new(usize::MAX),
    };
    let callbacks = MemoryCallbacks {
        context: (&allocator as *const Allocator).cast_mut().cast(),
        allocate: allocate,
        deallocate: deallocate,
        reallocate: reallocate,
    };
    let Ok(lib) = (unsafe { Library::with_callbacks(&callbacks) }) else {
        std::process::exit(1);
    };
    // Context remains at a stable caller stack address for the backend's whole life.
    let limit = core::cell::Cell::new(usize::MAX);
    let mut source = Bytes {
        data: &bytes,
        limit: &limit,
    };
    let mut book = BookStream::new(&mut source);
    let mut ctx = StreamContext {
        reader: &mut book,
        length: bytes.len() as u64,
    };
    let backend = Native::stream(&lib, &mut ctx);
    let mut mono = vec![0; 1048576];
    let mut sub = vec![0; 1048576];
    let mut pos = vec![0; 1048576];
    let mut font = FtFont::new(backend, &mut mono, &mut sub, &mut pos);
    println!("FTFONT1");
    for (op, a) in commands {
        let u = |i: usize| a[i] as u32;
        match op.as_str() {
            "ALLOC" => {
                allocator.remaining.set(if a[0] == u32::MAX as i64 {
                    usize::MAX
                } else {
                    a[0] as usize
                });
                println!("ALLOC");
            }
            "INIT" => {
                limit.set(if a[4] == 0 { usize::MAX } else { a[4] as usize });
                let ok = font.init(
                    if a[0] == 0 { Some(&bytes) } else { None },
                    a[1] as u16,
                    a[2] as i32,
                    a[3] != 0,
                );
                println!("INIT {}", u8::from(ok));
            }
            "OPTIONS" => {
                let o = RenderOptions {
                    hinting: match a[0] {
                        1 => HintingMode::None,
                        2 => HintingMode::Auto,
                        3 => HintingMode::Light,
                        4 => HintingMode::Native,
                        _ => HintingMode::Default,
                    },
                    interpreter_version: a[1] as u8,
                    monochrome: a[2] != 0,
                    stem_darkening: a[3] != 0,
                    embolden26_6: a[4] as i32,
                    slant16_16: a[5] as i32,
                };
                println!("OPTIONS {}", u8::from(font.set_render_options(o)));
            }
            "METRIC" | "GMETRIC" => {
                let m = if op == "METRIC" {
                    font.metrics26_6(u(0), u(1))
                } else {
                    font.metrics_glyph26_6(u(0), u(1))
                };
                if let Some(m) = m {
                    println!(
                        "METRIC 1 {} {} {} {} {}",
                        m.advance26_6, m.left, m.top, m.width, m.height
                    );
                } else {
                    println!("METRIC 0");
                }
            }
            "RASTER" | "GRASTER" => {
                let b = if op == "RASTER" {
                    font.rasterize26_6(u(0), u(1))
                } else {
                    font.rasterize_glyph26_6(u(0), u(1))
                };
                if let Some(b) = b {
                    let hex: String = b.pixels.iter().map(|v| format!("{v:02x}")).collect();
                    println!(
                        "RASTER 1 {} {} {} {} {} {}",
                        b.width,
                        b.height,
                        b.xoff,
                        b.yoff,
                        b.advance,
                        if hex.is_empty() { "-" } else { &hex }
                    );
                } else {
                    println!("RASTER 0");
                }
            }
            "LINE" => {
                if let Some(m) = font.line_metrics26_6(u(0)) {
                    println!(
                        "LINE 1 {} {} {}",
                        m.ascender26_6, m.descender26_6, m.height26_6
                    );
                } else {
                    println!("LINE 0");
                }
            }
            "KERN" => println!("KERN {}", font.kerning26_6(u(0), u(1), u(2))),
            "GKERN" => println!("KERN {}", font.kerning_glyphs26_6(u(0), u(1), u(2))),
            "LIGA" => println!("LIGA {}", font.ligature_codepoint(u(0), u(1))),
            "LIGAG" => {
                let seq = [u(0), u(1), u(2)];
                println!(
                    "LIGAG {}",
                    font.ligature_glyph_id(if u(2) == 0 { &seq[..2] } else { &seq })
                );
            }
            "BUDGET" => {
                font.set_gsub_byte_budget(a[0] as usize);
                font.set_gpos_byte_budget(a[1] as usize);
                println!("BUDGET");
            }
            "RELEASE" => {
                if u(0) == 0 {
                    font.release_ligature_table();
                } else {
                    font.release_kerning_table();
                }
                println!("RELEASE");
            }
            "STATUS" => println!(
                "STATUS {} {} {} {} {}",
                u8::from(font.ready()),
                font.last_init_failure as u8,
                font.last_init_error,
                font.last_glyph_failure as u8,
                font.last_glyph_error
            ),
            "DEINIT" => {
                font.deinit();
                println!("DEINIT");
            }
            "INSPECT" => {
                let mut reader = Bytes {
                    data: &bytes,
                    limit: &limit,
                };
                let mut book = BookStream::new(&mut reader);
                let mut context = StreamContext {
                    reader: &mut book,
                    length: bytes.len() as u64,
                };
                let mut backend = Native::stream(&lib, &mut context);
                let mut family = vec![0; a[1] as usize];
                let (r, i) = inspect(
                    &mut backend,
                    if a[0] == 0 { Some(&bytes) } else { None },
                    &mut family,
                );
                let status = match r {
                    InspectResult::Ok => 0,
                    InspectResult::Unsupported => -1,
                    InspectResult::Unavailable => -2,
                };
                let hex: String = family.iter().map(|v| format!("{v:02x}")).collect();
                println!(
                    "INSPECT {} {} {} {} {}",
                    status,
                    i.family_length,
                    i.weight,
                    u8::from(i.italic),
                    if hex.is_empty() { "-" } else { &hex }
                );
            }
            _ => unreachable!(),
        }
    }
    println!("END");
}
#[cfg(test)]
mod callback_tests {
    use super::*;
    #[test]
    fn callback_failure_at_shared_library_creation() {
        let allocator = Allocator {
            remaining: core::cell::Cell::new(0),
        };
        let callbacks = MemoryCallbacks {
            context: (&allocator as *const Allocator).cast_mut().cast(),
            allocate,
            deallocate,
            reallocate,
        };
        assert!(matches!(
            unsafe { Library::with_callbacks(&callbacks) },
            Err(0x40)
        ));
    }
}
