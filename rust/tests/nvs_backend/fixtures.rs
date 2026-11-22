// SPDX-License-Identifier: MIT

// Own the NVS fixture's model so a joined driver may name its SD model freely.
#[path = "model.rs"]
pub mod model;
use super::*;
use model::{Case, Model};

fn model() -> Model {
    Model::new(Case::parse("unit read 1 0 0 0 4 0 4 0 0 0"))
}

#[test]
fn malformed_extent_does_not_touch_output_or_issue_data_call() {
    for size in [0, 3, 5, usize::MAX] {
        let mut native = model();
        native.case.query_size = size;
        let mut backend = NvsBlobBackend::new(&mut native);
        assert_eq!(backend.begin("newt_reader", true), StorageOpen::Opened);
        let mut bytes = [0xa5; 4];
        assert_eq!(
            backend.read_blob("slot0", &mut bytes),
            BlobRead::Present(size)
        );
        assert_eq!(bytes, [0xa5; 4]);
        backend.end();
        assert_eq!(
            native.trace,
            "ready:1;open:newt_reader:1;query:7:slot0:0;close:7;"
        );
    }
}

#[test]
fn failed_set_short_circuits_commit_and_failed_commit_never_acknowledges() {
    let mut native = model();
    native.case.set = 0x1105;
    let mut backend = NvsBlobBackend::new(&mut native);
    backend.begin("newt_reader", false);
    assert_eq!(backend.set_and_commit("slot0", &[1, 2, 3, 4]), 0);
    backend.end();
    assert_eq!(
        native.trace,
        "ready:1;open:newt_reader:0;set:7:slot0:4;close:7;"
    );
    native = model();
    native.case.commit = -1;
    let mut backend = NvsBlobBackend::new(&mut native);
    backend.begin("newt_reader", false);
    assert_eq!(backend.set_and_commit("slot0", &[1, 2, 3, 4]), 0);
    backend.end();
    assert_eq!(
        native.trace,
        "ready:1;open:newt_reader:0;set:7:slot0:4;commit:7;close:7;"
    );
}

#[test]
fn absent_erase_still_requires_commit() {
    for commit in [0, -1] {
        let mut native = model();
        native.case.erase = 0x1102;
        native.case.commit = commit;
        let mut backend = NvsBlobBackend::new(&mut native);
        backend.begin("newt_reader", false);
        assert_eq!(backend.erase_key("slot0"), commit == 0);
        backend.end();
        assert_eq!(
            native.trace,
            "ready:1;open:newt_reader:0;erase:7:slot0;commit:7;close:7;"
        );
    }
}

#[test]
fn closed_backend_never_calls_native_data_operations() {
    let mut native = model();
    let mut backend = NvsBlobBackend::new(&mut native);
    assert_eq!(backend.read_blob("slot0", &mut [0; 4]), BlobRead::Failed);
    assert_eq!(backend.set_and_commit("slot0", &[1, 2, 3, 4]), 0);
    assert!(!backend.erase_key("slot0"));
    backend.end();
    assert!(native.trace.is_empty());
}

#[test]
fn bounded_names_and_exact_maximum_work_without_truncation() {
    for name in ["", "sixteen-bytesxxx!", "bad\0suffix"] {
        assert!(Name::new(name).is_none());
        let mut native = model();
        let mut backend = NvsBlobBackend::new(&mut native);
        assert_eq!(backend.begin(name, false), StorageOpen::Failed);
        assert_eq!(native.trace, "ready:1;");
    }
    let mut native = model();
    let mut backend = NvsBlobBackend::new(&mut native);
    assert_eq!(backend.begin("123456789012345", true), StorageOpen::Opened);
    for key in ["", "1234567890123456", "slot0\0x"] {
        assert_eq!(backend.read_blob(key, &mut [0; 4]), BlobRead::Failed);
        assert_eq!(backend.set_and_commit(key, &[1, 2, 3, 4]), 0);
        assert!(!backend.erase_key(key));
    }
    backend.end();
    assert_eq!(native.trace, "ready:1;open:123456789012345:1;close:7;");
}

#[test]
fn native_errors_are_lossless_and_readback_has_no_optimistic_default() {
    for code in [-1, i32::MIN, 0x1102, 0x1105, 0x110c, i32::MAX] {
        assert_eq!(NativeError::from_code(code).unwrap_err().code(), code);
    }
    assert_eq!(NativeError::from_code(0), Ok(()));
    let mut native = model();
    native.durable = false;
    assert!(!NvsBlobBackend::new(&mut native).readback_durable());
    native.durable = true;
    assert!(NvsBlobBackend::new(&mut native).readback_durable());
}
