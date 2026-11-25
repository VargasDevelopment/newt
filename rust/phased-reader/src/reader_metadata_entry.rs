// MIT. Parser-oracle entry points over canonical metadata.
fn select_metadata<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    spine: u32,
    selected: &mut Selection,
    arena: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    toc: &mut [TocEntry],
    sr: &mut StyleResources<'_, '_>,
    sources: &mut Sources<'_>,
    on_yield: newt_core::admission::Yield<'_>,
    observer: Observer,
    plain: bool,
) -> Result<(bool, bool), Status> {
    select_metadata_capture(
        source, spine, selected, arena, r, toc, sr, sources, on_yield, observer, plain, None,
    )
}

// MIT. Parser-oracle entry points over canonical metadata.
fn select_metadata_capture<S: BookSource + ?Sized, B: ParserBackend + ?Sized>(
    source: &mut S,
    spine: u32,
    selected: &mut Selection,
    arena: &mut Arena<'_>,
    r: &mut canonical::Resources<'_, B>,
    toc: &mut [TocEntry],
    sr: &mut StyleResources<'_, '_>,
    sources: &mut Sources<'_>,
    on_yield: newt_core::admission::Yield<'_>,
    observer: Observer,
    plain: bool,
    suffix_capacity: Option<&mut usize>,
) -> Result<(bool, bool), Status> {
    arena.reset();
    let mut scheduled = ScheduledSource::new(source, on_yield, None);
    let book = canonical::open(&mut scheduled, arena, r, false, on_yield, observer)?;
    select_book_metadata(
        book,
        spine,
        selected,
        sr,
        sources,
        on_yield,
        plain,
        suffix_capacity,
        None,
        toc,
    )
}
