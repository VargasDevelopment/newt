// SPDX-License-Identifier: MIT
//! Frozen global/source recovery order; CrossInk files are never rewritten.
use super::io::{Files, Found, Path, read_file};
use crate::crossink_stats as ci;
use core::ffi::CStr;

pub(super) fn read_global<F: Files>(
    files: &mut F,
    root: &CStr,
    copy: &mut ci::CrossInkGlobalCopy,
) -> Found {
    let mut found = false;
    for (file, candidate) in [
        (
            b"global_stats.bin".as_slice(),
            ci::CrossInkCandidate::Primary,
        ),
        (
            b"global_stats.bin.bak".as_slice(),
            ci::CrossInkCandidate::Backup,
        ),
    ] {
        let path = Path::new(&[root.to_bytes(), b"/.crosspoint/", file]);
        let mut size = 0;
        let Ok(path) = path.c_str() else {
            return Found::Failed;
        };
        match read_file(files, path, &mut copy.bytes, &mut size) {
            Found::Failed => return Found::Failed,
            Found::Missing => continue,
            Found::Read => found = true,
        }
        match ci::classify_cross_ink_global_prefix(&copy.bytes, size) {
            ci::CrossInkFile::Valid => {
                copy.bytes[size..].fill(0);
                copy.status = ci::CrossInkStatus::Imported;
                copy.candidate = candidate;
                copy.size = size as u8;
                return Found::Read;
            }
            ci::CrossInkFile::Newer => {
                *copy = ci::CrossInkGlobalCopy {
                    status: ci::CrossInkStatus::Newer,
                    candidate,
                    ..ci::CrossInkGlobalCopy::default()
                };
                return Found::Read;
            }
            ci::CrossInkFile::Invalid => {}
        }
    }
    *copy = ci::CrossInkGlobalCopy::default();
    if found { Found::Read } else { Found::Missing }
}
pub(super) fn read_source<F: Files>(
    files: &mut F,
    root: &CStr,
    name: &[u8],
    copy: &mut ci::CrossInkCopy<{ ci::BOOK_BYTES }>,
) -> Found {
    let candidates = [
        (b"stats_v5.bin".as_slice(), ci::CrossInkCandidate::Primary),
        (b"stats_v5.bin.tmp".as_slice(), ci::CrossInkCandidate::Temp),
        (
            b"stats_v5.bin.bak".as_slice(),
            ci::CrossInkCandidate::Backup,
        ),
        (b"stats_v4.bin".as_slice(), ci::CrossInkCandidate::Previous),
        (b"stats.bin".as_slice(), ci::CrossInkCandidate::Legacy),
    ];
    let mut found = false;
    for (file, candidate) in candidates {
        let path = Path::new(&[root.to_bytes(), b"/.crosspoint/", name, b"/", file]);
        let mut size = 0;
        let Ok(path) = path.c_str() else {
            return Found::Failed;
        };
        match read_file(files, path, &mut copy.bytes, &mut size) {
            Found::Failed => return Found::Failed,
            Found::Missing => continue,
            Found::Read => found = true,
        }
        if !ci::valid_cross_ink_book_prefix(&copy.bytes, size)
            || (candidate == ci::CrossInkCandidate::Temp && size != ci::BOOK_BYTES)
        {
            continue;
        }
        copy.bytes[size..].fill(0);
        copy.status = ci::CrossInkStatus::Imported;
        copy.candidate = candidate;
        copy.size = size as u8;
        return Found::Read;
    }
    *copy = ci::CrossInkCopy::default();
    if found { Found::Read } else { Found::Missing }
}
