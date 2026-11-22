/// Borrowed runtime state and buffers: never a second catalog on target stack,
/// static storage, heap or an owned global. Codegen evidence, no device claim.
#[unsafe(no_mangle)]
pub fn newt_rust_catalog_probe(
    catalog: &mut newt_core::library_catalog::LibraryCatalog,
    copy_bytes: &mut [u32; newt_core::library_catalog::BOOK_CAPACITY],
    name: &[u8],
    directory: &[u8],
    suffix: &[u8],
    output: &mut [u8],
    bytes: u32,
    flags: u8,
) -> usize {
    use newt_core::library_catalog::*;
    let mut calls = 0usize;
    match flags % 9 {
        0 => catalog.reset(if flags & 128 != 0 {
            LibraryStatus::Unavailable
        } else {
            LibraryStatus::Ok
        }),
        1 => {
            catalog.add_plain(name, bytes);
        }
        2 => {
            // The 512-byte insertion-slot map is scanner state, separate from
            // the resident 12-KiB catalog. This temporary 512-byte bridge
            // makes no second catalog; it exercises the generic comparator.
            let mut proofs = LibraryCopies {
                bytes: *copy_bytes,
                same: Some(|a: &[u8], b: &[u8]| {
                    calls += 1;
                    a == b && flags & 128 != 0
                }),
            };
            catalog.add(name, bytes, Some(&mut proofs));
            *copy_bytes = proofs.bytes;
        }
        3 => catalog.omit(LibraryOmission::Name),
        4 => catalog.omit(LibraryOmission::Folder),
        5 => catalog.omit(LibraryOmission::Stopped),
        6 => {
            catalog.finish();
        }
        7 => {
            calls += usize::from(library_book_path(Some(directory), Some(name), Some(output)));
        }
        _ => {
            calls += usize::from(library_name_file(
                Some(directory),
                Some(name),
                Some(suffix),
                Some(output),
            ));
        }
    }
    let index = bytes as usize % (BOOK_CAPACITY + 1);
    calls += catalog.filename(index).map_or(0, |n| n.len());
    calls += usize::from(catalog.path(index, Some(output)));
    calls += library_base_name(Some(name)).map_or(0, |n| n.len());
    calls += usize::from(listable(name));
    calls
        + catalog.size()
        + usize::from(catalog.copies())
        + usize::from(catalog.omissions().full)
        + usize::from(catalog.omissions().names)
        + usize::from(catalog.omissions().folders)
        + usize::from(catalog.omissions().stopped)
        + catalog.notice().map_or(0, str::len)
        + catalog.status() as usize
}
