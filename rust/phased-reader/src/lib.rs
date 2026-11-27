//! Phased EPUB reader, chapter cache builder and imported-revision owner. MIT.
#![no_std]
#![deny(unsafe_code, unsafe_op_in_unsafe_fn)]
pub use newt_core::{
    admission, canonical, css, package_parsers, xml_input, xml_session, zip_container, zip_window,
};
mod reader;
mod guarded_store;
mod resources;
pub mod scan;
pub mod styles;
pub mod watch;
pub use reader::*;
pub use newt_reader_navigation::ContentsFallback;
pub use resources::*;
pub mod borrowed;
pub mod bounded;

pub mod semantic_builder;

pub mod semantic_metadata;

// Revision authority and operation-scoped reader/cache collaboration.
pub mod imported_revision;
pub mod hash_provider;
pub mod session_probe;

// Bank casts are confined to the private physical phase implementation.
#[allow(unsafe_code)]
pub mod phase_storage;

pub use reader::phase_session::{OperationPhased, PhasedChapterCache};

/// Attempt-scoped cache writes; no reader or parse-bank ownership.
pub mod producer_store;

/// Segmented producer callbacks over the existing cache serializer.
pub mod producer_writer;

/// Observational resume-point capture carved from the shared bank remainder.
pub mod resume_capture;

/// Verified idle-producer resume-point restore and prefix-start collection.
pub mod resume_restore;

pub mod producer_probe;

pub mod index_build;
