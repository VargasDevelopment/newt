// SPDX-License-Identifier: MIT
//! The existing FAT model behind scoped UiIo views. Only this host fixture
//! shares it; no native SDK entry or firmware storage implementation changes.
use crate::home_startup::CardPort;
use crate::model::{Effect, Fault, Model, Op};
use crate::{Directory, Kind, Native, Open, Result, Stat};
use newt_native_io::UiIo;
use std::{cell::RefCell, collections::BTreeMap, marker::PhantomData, rc::Rc};

#[derive(Default)]
pub struct CardState {
    pub model: Model,
    pub enforce_reader_limit: bool,
    pub most_open: usize,
    pub handles: BTreeMap<i32, String>,
    pub state_with_pages: bool,
    pub state_open_seen: bool,
    pub pages_at_arm: bool,
    pub watching: bool,
    pub io_fault: Option<(String, Op)>,
    pub io_fault_seen: bool,
    pub page_write_reopens: u32,
}
#[derive(Clone, Default)]
pub struct Card(pub Rc<RefCell<CardState>>);
pub struct Port<'ui> {
    state: Rc<RefCell<CardState>>,
    _loan: PhantomData<&'ui UiIo>,
}
impl CardPort for Card {
    type Port<'ui> = Port<'ui>;
    fn port<'ui>(&mut self, _: &'ui UiIo) -> Port<'ui> {
        Port {
            state: self.0.clone(),
            _loan: PhantomData,
        }
    }
}
impl Card {
    pub fn seed(&self, path: &str, bytes: Vec<u8>) {
        let mut state = self.0.borrow_mut();
        let mut at = String::new();
        for part in path
            .rsplit_once('/')
            .unwrap()
            .0
            .split('/')
            .filter(|p| !p.is_empty())
        {
            at.push('/');
            at.push_str(part);
            state.model.dirs.insert(at.clone());
        }
        state.model.files.insert(
            path.to_owned(),
            crate::model::File {
                durable: bytes.clone(),
                readable: bytes,
            },
        );
    }
    pub fn arm_pages_observation(&self) {
        let mut state = self.0.borrow_mut();
        state.pages_at_arm = state.handles.values().any(|p| p.contains("/Pages/"));
        assert!(
            state.pages_at_arm,
            "live Pages lease before State is a precondition"
        );
        state.state_with_pages = false;
        state.state_open_seen = false;
        state.watching = true;
    }
    pub fn cold_bytes(&self) -> Card {
        let state = self.0.borrow();
        assert!(
            state.handles.is_empty(),
            "all reader descriptors close before cold reopen"
        );
        let mut fresh = CardState::default();
        fresh.model.dirs = state.model.dirs.clone();
        // Fresh model, owners, nonce, handles, faults and facts. Only card
        // directory entries and durable file bytes cross the cold boundary.
        for (path, file) in &state.model.files {
            fresh.model.files.insert(
                path.clone(),
                crate::model::File {
                    durable: file.durable.clone(),
                    readable: file.durable.clone(),
                },
            );
        }
        Card(Rc::new(RefCell::new(fresh)))
    }
}
impl Native for Port<'_> {
    fn open(&mut self, path: &str, mode: Open) -> Result<i32> {
        let mut state = self.state.borrow_mut();
        if path.contains("/Pages/") && matches!(mode, Open::ReadWriteNonblock) { state.page_write_reopens += 1; }
        if state.watching && path.contains("/State/") {
            state.state_open_seen = true;
            state.state_with_pages |= state.handles.values().any(|p| p.contains("/Pages/"));
        }
        let fd = state.model.open(path, mode)?;
        state.handles.insert(fd, path.to_owned());
        state.most_open=state.most_open.max(state.handles.len());
        if state.enforce_reader_limit {assert!(state.handles.len()<=3,"index handoff exceeded three open files");}
        Ok(fd)
    }
    fn close(&mut self, fd: i32) -> Result<()> {
        let mut state = self.state.borrow_mut();
        let result = state.model.close(fd);
        if result.is_ok() {
            state.handles.remove(&fd);
        }
        result
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize> {
        let mut state = self.state.borrow_mut();
        let target = state
            .io_fault
            .as_ref()
            .is_some_and(|(path, op)| *op == Op::Read && state.handles.get(&fd) == Some(path));
        if target {
            state.io_fault = None;
            state
                .model
                .faults
                .push(Fault::new(Op::Read, 1, 1, Effect::Errno("EIO"), 0));
        }
        let result = state.model.read(fd, out);
        state.io_fault_seen |= target && result == Err(crate::Error::Unavailable);
        result
    }
    fn write(&mut self, fd: i32, bytes: &[u8]) -> Result<usize> {
        let mut state = self.state.borrow_mut();
        let target = state
            .io_fault
            .as_ref()
            .is_some_and(|(path, op)| *op == Op::Write && state.handles.get(&fd) == Some(path));
        if target {
            state.io_fault = None;
            // Every write fails once the checkpoint's first write is reached:
            // neither raw retry nor the effective settling marker may succeed.
            state
                .model
                .faults
                .push(Fault::new(Op::Write, 1, u32::MAX, Effect::Errno("EIO"), 0));
        }
        let result = state.model.write(fd, bytes);
        state.io_fault_seen |= target && result == Err(crate::Error::Unavailable);
        result
    }
    fn seek(&mut self, fd: i32, at: u32) -> Result<u64> {
        self.state.borrow_mut().model.seek(fd, at)
    }
    fn truncate(&mut self, fd: i32, bytes: u32) -> Result<()> {
        self.state.borrow_mut().model.truncate(fd, bytes)
    }
    fn sync(&mut self, fd: i32) -> Result<()> {
        self.state.borrow_mut().model.sync(fd)
    }
    fn fstat(&mut self, fd: i32) -> Result<Stat> {
        self.state.borrow_mut().model.fstat(fd)
    }
    fn stat(&mut self, path: &str, follow: bool) -> Result<Stat> {
        let found = self.state.borrow_mut().model.stat(path, follow)?;
        assert!(matches!(found.kind, Kind::Regular | Kind::Directory));
        Ok(found)
    }
    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.state.borrow_mut().model.mkdir(path)
    }
    fn unlink(&mut self, path: &str) -> Result<()> {
        self.state.borrow_mut().model.unlink(path)
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        self.state.borrow_mut().model.rename(from, to)
    }
    fn next_nonce(&mut self) -> u32 {
        self.state.borrow_mut().model.next_nonce()
    }
}
impl Directory for Port<'_> {
    type Listing = <Model as Directory>::Listing;
    fn open_dir(&mut self, path: &str) -> Result<Self::Listing> {
        self.state.borrow_mut().model.open_dir(path)
    }
    fn read_dir(&mut self, listing: &mut Self::Listing, name: &mut [u8]) -> Result<Option<usize>> {
        self.state.borrow_mut().model.read_dir(listing, name)
    }
    fn close_dir(&mut self, listing: Self::Listing) -> Result<()> {
        self.state.borrow_mut().model.close_dir(listing)
    }
}

impl newt_native_storage::page_entries::PageEntries for Port<'_> {
    fn walk_entries(&mut self, folder: &core::ffi::CStr, sized: bool,
        visit: &mut dyn FnMut(&mut Self, newt_native_storage::page_entries::PageEntry<'_>) -> bool,
    ) -> bool { newt_native_storage::page_entries::walk_directory(self, folder, sized, visit) }
}
