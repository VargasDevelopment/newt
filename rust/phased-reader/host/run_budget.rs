// SPDX-License-Identifier: MIT
use crate::{entry::linebreak::NativeBreak, physical::{Bank, Key, Source, Store}};
use newt_core::{preferences::{Orientation, TextSize, ViewSettings}, zip_container::Status};
use newt_phased_reader::{CacheRequest, OperationPhased, PageLayout, PhasedChapterCache, PhasedReader, Selection,
    imported_revision::session::{Build, Cache}, phase_storage::{BANK_BYTES, Storage}};
use newt_phased_sink::{FRAME_BYTES, Plan, RUNS, Run, TEXT, Target};
use std::path::Path;

pub fn run(path: &Path) -> std::io::Result<()> {
    let mut source = Source::open(path)?;
    let mut shared = Box::new(Bank([0; BANK_BYTES]));
    let mut native = Box::new(Bank([0; BANK_BYTES]));
    let mut storage = Storage::new(&mut shared.0);
    let mut parser = newt_target_expat::Expat::default();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), PageLayout::PORTRAIT);
    let mut store = Store::default();
    let settings = ViewSettings { orientation: Orientation::Portrait, ..ViewSettings::default() };
    let digest = [0; 32];
    let mut breaker = NativeBreak;
    let mut frame = [0; FRAME_BYTES];
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut *native, &mut parser), Status::Ok);
    assert_eq!(reader.capture_phased(&mut source, 0, 0, Target::Anchor, &mut storage,
        &mut *native, &mut parser, &mut breaker), Status::Ok);
    assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
    assert_eq!(reader.info().page, 0, "opening at character zero lands on page one");

    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut *native, &mut parser), Status::Ok);
    let mut cache = PhasedChapterCache { store: &mut store, hash: &mut Key, settings };
    {
        let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *native, &mut parser, &mut breaker);
        assert_eq!(cache.build(&mut source, &digest, 0, &mut operation), Build::Ready);
    }
    // Read the persisted page start table independently of the seek code.
    let file = &cache.store.file;
    let footer = &file[file.len()-24..];
    let offset = u32::from_le_bytes(footer[..4].try_into().map_err(std::io::Error::other)?) as usize;
    let count = u32::from_le_bytes(footer[4..8].try_into().map_err(std::io::Error::other)?) as usize;
    let starts: Vec<_> = file[offset..offset + count * 8].chunks_exact(8)
        .map(|row| u32::from_le_bytes([row[4], row[5], row[6], row[7]])).collect();
    assert!(starts.len() >= 3 && starts[0] == 0);
    assert!(starts.windows(2).all(|pair| pair[0] < pair[1]), "{starts:?}: unique page starts");

    for (page, target) in [(0, 0), (1, starts[1]), (2, starts[2] + 1)] {
        // A fresh reader selection and a saved anchor use the same cache query
        // as open, resume and go-to-position.
        assert_eq!(reader.open_phased(&mut source, &mut storage, &mut *native, &mut parser), Status::Ok);
        let request = CacheRequest { spine: 0, target, anchor: true, staged: false,
            build_on_miss: false, cache_only: true };
        let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *native, &mut parser, &mut breaker);
        assert!(cache.capture(&mut source, &digest, request, &mut operation).hit);
        assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
        assert_eq!(reader.info().page as usize, page);
        assert_eq!(reader.info().char_start, starts[page]);
        let runs = reader.page_plan().ok_or_else(|| std::io::Error::other("missing retained reading page"))?.runs();
        assert_eq!(runs[0].x, newt_reader_atlas::Geometry::reader(false).margin as i16);
        if page == 0 { assert!((280..RUNS).contains(&runs.len()), "dense first page: {} runs", runs.len()); }
    }
    println!("PASS whole-line run budget: page starts {starts:?} and cached position lookup");
    for orientation in [Orientation::Portrait, Orientation::LandscapeCcw] {
        for text_size in TextSize::ALL {
            sized(path, ViewSettings { orientation, text_size, ..ViewSettings::default() })?;
        }
    }
    Ok(())
}

/// Every text size paginates the dense book within the run budget, and a
/// saved position reopens on the page that holds it.
fn sized(path: &Path, settings: ViewSettings) -> std::io::Result<()> {
    let mut source = Source::open(path)?;
    let mut shared = Box::new(Bank([0; BANK_BYTES]));
    let mut native = Box::new(Bank([0; BANK_BYTES]));
    let mut storage = Storage::new(&mut shared.0);
    let mut parser = newt_target_expat::Expat::default();
    let mut selections = [Selection::default(), Selection::default()];
    let mut runs = [Run::default(); RUNS];
    let mut text = [0; TEXT];
    let mut plan = Plan::new(&mut runs, &mut text);
    let mut reader = PhasedReader::new(&mut selections, Some(&mut plan), settings.page_layout());
    let mut store = Store::default();
    let mut breaker = NativeBreak;
    let mut frame = [0; FRAME_BYTES];
    assert_eq!(reader.open_phased(&mut source, &mut storage, &mut *native, &mut parser), Status::Ok);
    let mut cache = PhasedChapterCache { store: &mut store, hash: &mut Key, settings };
    {
        let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *native, &mut parser, &mut breaker);
        assert_eq!(cache.build(&mut source, &[0; 32], 0, &mut operation), Build::Ready);
    }
    let file = &cache.store.file;
    let footer = &file[file.len()-24..];
    let offset = u32::from_le_bytes(footer[..4].try_into().map_err(std::io::Error::other)?) as usize;
    let count = u32::from_le_bytes(footer[4..8].try_into().map_err(std::io::Error::other)?) as usize;
    let starts: Vec<_> = file[offset..offset + count * 8].chunks_exact(8)
        .map(|row| u32::from_le_bytes([row[4], row[5], row[6], row[7]])).collect();
    assert!(starts.first() == Some(&0) && starts.windows(2).all(|pair| pair[0] < pair[1]), "{settings:?}: {starts:?}");
    let mut peak = 0;
    for page in [0, starts.len() / 2, starts.len() - 1] {
        assert_eq!(reader.open_phased(&mut source, &mut storage, &mut *native, &mut parser), Status::Ok);
        let request = CacheRequest { spine: 0, target: starts[page] + 1, anchor: true, staged: false,
            build_on_miss: false, cache_only: true };
        let mut operation = OperationPhased::new(&mut reader, &mut storage, &mut *native, &mut parser, &mut breaker);
        assert!(cache.capture(&mut source, &[0; 32], request, &mut operation).hit);
        assert_eq!(reader.publish(&mut source, &mut frame), Status::Ok);
        assert_eq!((reader.info().page as usize, reader.info().char_start), (page, starts[page]), "{settings:?}");
        peak = peak.max(reader.page_plan().ok_or_else(|| std::io::Error::other("missing page"))?.runs().len());
    }
    println!("PASS {} {} px: {} pages, peak {peak} of {RUNS} runs", settings.orientation.name(),
        settings.text_size.px(), starts.len());
    Ok(())
}
