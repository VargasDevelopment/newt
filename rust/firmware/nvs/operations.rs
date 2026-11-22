// SPDX-License-Identifier: MIT
//! One implementation for ordinary NVS and transaction-owned NVS state.
use super::*;

pub(super) struct Session<'a, N: Native> {
    pub(super) native: &'a mut N,
    pub(super) state: &'a mut TransactionState,
}
impl<N: Native> Session<'_, N> {
    pub(super) fn set_and_commit(&mut self, key: &str, bytes: &[u8]) -> usize {
        if !self.state.opened {
            return 0;
        }
        let Some(key) = Name::new(key) else { return 0 };
        if self
            .native
            .set_blob(self.state.handle, key.as_c_str(), bytes)
            .is_err()
            || self.native.commit(self.state.handle).is_err()
        {
            return 0;
        }
        bytes.len()
    }

    /// Already absent still commits, exactly as the pinned backend does.
    pub(super) fn erase_key(&mut self, key: &str) -> bool {
        if !self.state.opened {
            return false;
        }
        let Some(key) = Name::new(key) else {
            return false;
        };
        let result = self.native.erase_key(self.state.handle, key.as_c_str());
        (result.is_ok() || result == Err(NativeError::NOT_FOUND))
            && self.native.commit(self.state.handle).is_ok()
    }
}

impl<N: Native> Backend for Session<'_, N> {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen {
        if !self.native.storage_ready() {
            // Return before changing opened or handle state.
            return StorageOpen::Failed;
        }
        let Some(name) = Name::new(namespace) else {
            return StorageOpen::Failed;
        };
        let result = self
            .native
            .open(name.as_c_str(), readonly, &mut self.state.handle);
        self.state.opened = result.is_ok();
        match result {
            Ok(()) => StorageOpen::Opened,
            Err(error) if readonly && error == NativeError::NOT_FOUND => StorageOpen::Missing,
            Err(_) => StorageOpen::Failed,
        }
    }

    fn end(&mut self) {
        if self.state.opened {
            self.native.close(self.state.handle);
        }
        self.state.opened = false;
    }

    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead {
        if !self.state.opened {
            return BlobRead::Failed;
        }
        let Some(key) = Name::new(key) else {
            return BlobRead::Failed;
        };
        let mut size = 0;
        match self
            .native
            .get_blob(self.state.handle, key.as_c_str(), None, &mut size)
        {
            Err(error) if error == NativeError::NOT_FOUND => return BlobRead::Missing,
            Err(_) => return BlobRead::Failed,
            Ok(()) => {}
        }
        let observed = size;
        if observed != output.len() {
            return BlobRead::Present(observed);
        }
        let result = self
            .native
            .get_blob(self.state.handle, key.as_c_str(), Some(output), &mut size);
        if result.is_err() || size != observed {
            BlobRead::Failed
        } else {
            BlobRead::Present(observed)
        }
    }

    fn put_bytes(&mut self, _permit: DurableWrite, key: &str, bytes: &[u8]) -> usize {
        self.set_and_commit(key, bytes)
    }

    fn readback_durable(&self) -> bool {
        self.native.readback_durable()
    }
}
