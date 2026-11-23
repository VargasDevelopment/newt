// SPDX-License-Identifier: MIT
//! A card, a serial channel and a SHA-256 for the lease, each as plain as the
//! firmware's: the card stores every write at once (so a partial file is
//! visible), refuses a rename onto an existing name, and fails a named step
//! on request.
use newt_core::dev_put::{Card, Entry, Wire};
use newt_core::update_online::Hash;
use newt_core::wire::crc32;
use std::collections::{BTreeMap, VecDeque};
use std::ffi::CStr;

pub const FAILED: i32 = -5;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Node { File(Vec<u8>), Folder }

pub struct Handle { path: String, at: usize }

pub struct FakeCard {
    pub nodes: BTreeMap<String, Node>,
    pub handles: BTreeMap<i32, Handle>,
    pub next_fd: i32,
    pub free: u64,
    /// Each write stores at most this many bytes.
    pub write_limit: usize,
    /// The card stores this offset flipped, as a failing card would.
    pub flip: Option<usize>,
    /// This step fails with `FAILED`.
    pub fail: Option<&'static str>,
}

impl Default for FakeCard {
    fn default() -> Self {
        Self { nodes: BTreeMap::from([("/newt-sd".into(), Node::Folder)]), handles: BTreeMap::new(), next_fd: 3,
            free: 1 << 30, write_limit: usize::MAX, flip: None, fail: None }
    }
}

impl FakeCard {
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        match self.nodes.get(path) {
            Some(Node::File(bytes)) => Some(bytes),
            _ => None,
        }
    }
    pub fn put_file(&mut self, path: &str, bytes: &[u8]) {
        let parent = &path[..path.rfind('/').expect("absolute")];
        let mut at = 0;
        while let Some(next) = parent[at + 1..].find('/').map(|i| i + at + 1) {
            self.nodes.insert(parent[..next].into(), Node::Folder);
            at = next;
        }
        self.nodes.insert(parent.into(), Node::Folder);
        self.nodes.insert(path.into(), Node::File(bytes.to_vec()));
    }
    pub fn open_files(&self) -> usize { self.handles.len() }
    fn step(&self, name: &str) -> Result<(), i32> {
        if self.fail == Some(name) { Err(FAILED) } else { Ok(()) }
    }
    fn parent_is_folder(&self, path: &str) -> bool {
        path.rfind('/').is_some_and(|at| self.nodes.get(&path[..at]) == Some(&Node::Folder))
    }
}

fn text(path: &CStr) -> String { path.to_str().expect("UTF-8 path").into() }

impl Card for FakeCard {
    fn entry(&mut self, path: &CStr) -> Result<Entry, i32> {
        Ok(match self.nodes.get(&text(path)) {
            None => Entry::Missing,
            Some(Node::File(_)) => Entry::File,
            Some(Node::Folder) => Entry::Folder,
        })
    }
    fn make_folder(&mut self, path: &CStr) -> Result<(), i32> {
        let path = text(path);
        if !self.parent_is_folder(&path) || self.nodes.contains_key(&path) { return Err(-17); }
        self.nodes.insert(path, Node::Folder);
        Ok(())
    }
    fn create(&mut self, path: &CStr) -> Result<i32, i32> {
        self.step("create")?;
        let path = text(path);
        if !self.parent_is_folder(&path) || self.nodes.get(&path) == Some(&Node::Folder) { return Err(-21); }
        self.nodes.insert(path.clone(), Node::File(Vec::new()));
        self.next_fd += 1;
        self.handles.insert(self.next_fd, Handle { path, at: 0 });
        Ok(self.next_fd)
    }
    fn open_read(&mut self, path: &CStr) -> Result<i32, i32> {
        let path = text(path);
        if self.file(&path).is_none() { return Err(-2); }
        self.next_fd += 1;
        self.handles.insert(self.next_fd, Handle { path, at: 0 });
        Ok(self.next_fd)
    }
    fn write(&mut self, fd: i32, bytes: &[u8]) -> Result<usize, i32> {
        self.step("write")?;
        let Some(handle) = self.handles.get(&fd) else { return Err(-9) };
        let Some(Node::File(file)) = self.nodes.get_mut(&handle.path) else { return Err(-9) };
        let put = bytes.len().min(self.write_limit);
        for &byte in &bytes[..put] {
            let flip = if self.flip == Some(file.len()) { 1 } else { 0 };
            file.push(byte ^ flip);
        }
        Ok(put)
    }
    fn read(&mut self, fd: i32, out: &mut [u8]) -> Result<usize, i32> {
        let Some(handle) = self.handles.get_mut(&fd) else { return Err(-9) };
        let Some(Node::File(file)) = self.nodes.get(&handle.path) else { return Err(-9) };
        let got = out.len().min(file.len() - handle.at);
        out[..got].copy_from_slice(&file[handle.at..handle.at + got]);
        handle.at += got;
        Ok(got)
    }
    fn sync(&mut self, _fd: i32) -> Result<(), i32> { self.step("sync") }
    fn close(&mut self, fd: i32) -> Result<(), i32> {
        self.handles.remove(&fd).map(drop).ok_or(-9)
    }
    fn remove(&mut self, path: &CStr) -> Result<(), i32> {
        match self.nodes.remove(&text(path)) {
            Some(Node::File(_)) => Ok(()),
            _ => Err(-2),
        }
    }
    fn rename(&mut self, from: &CStr, to: &CStr) -> Result<(), i32> {
        self.step("rename")?;
        let (from, to) = (text(from), text(to));
        if self.nodes.contains_key(&to) { return Err(-17); }
        let node = self.nodes.remove(&from).ok_or(-2)?;
        self.nodes.insert(to, node);
        Ok(())
    }
    fn free_bytes(&mut self) -> Result<u64, i32> {
        self.step("free")?;
        Ok(self.free)
    }
}

/// What the host does next, in order. A press or an unplug happens when the
/// lease next looks while no byte is waiting; after the script, silence.
pub enum Event { Line(String), Press, Unplug }

pub struct FakeWire {
    script: VecDeque<Event>,
    pending: VecDeque<u8>,
    connected: bool,
    pub clock: u32,
    pub sent: Vec<u8>,
    /// Bytes accepted per write call, so replies go out in pieces.
    pub write_limit: usize,
    /// The host stops reading: every write accepts nothing.
    pub stalled: bool,
}

impl FakeWire {
    pub fn new(script: Vec<Event>) -> Self {
        Self { script: script.into(), pending: VecDeque::new(), connected: true, clock: 1_000, sent: Vec::new(),
            write_limit: usize::MAX, stalled: false }
    }
    fn front(&mut self, wanted: fn(&Event) -> bool) -> bool {
        if !self.pending.is_empty() || !self.script.front().is_some_and(wanted) { return false; }
        self.script.pop_front();
        true
    }
    /// Each reply's ID and body, after checking its framing and CRC.
    pub fn replies(&self, boot: u32, session: u32) -> Vec<(u32, String)> {
        let text = std::str::from_utf8(&self.sent).expect("ASCII replies");
        text.split_terminator('\n').map(|line| {
            let (framed, checksum) = line.rsplit_once(" *").expect("checksum");
            assert_eq!(format!("{:08x}", crc32(framed.as_bytes())), checksum, "{line}");
            let mut fields = framed.splitn(5, ' ');
            assert_eq!(fields.next(), Some("!N1"));
            assert_eq!(fields.next(), Some(boot.to_string().as_str()));
            assert_eq!(fields.next(), Some(session.to_string().as_str()));
            let id = fields.next().and_then(|id| id.parse().ok()).expect("id");
            (id, fields.next().expect("body").to_owned())
        }).collect()
    }
}

impl Wire for FakeWire {
    fn connected(&mut self) -> bool {
        if self.front(|event| matches!(event, Event::Unplug)) { self.connected = false; }
        self.connected
    }
    fn read_byte(&mut self) -> i32 {
        if self.pending.is_empty() && matches!(self.script.front(), Some(Event::Line(_))) {
            let Some(Event::Line(line)) = self.script.pop_front() else { unreachable!() };
            self.pending.extend(line.into_bytes());
        }
        self.pending.pop_front().map_or(-1, i32::from)
    }
    fn write(&mut self, bytes: &[u8]) -> usize {
        if self.stalled { return 0; }
        let put = bytes.len().min(self.write_limit);
        self.sent.extend_from_slice(&bytes[..put]);
        put
    }
    fn now(&mut self) -> u32 { self.clock }
    fn physical_input(&mut self) -> bool { self.front(|event| matches!(event, Event::Press)) }
    fn pause(&mut self) { self.clock += 1; }
}

#[derive(Default)]
pub struct HostHash { data: Vec<u8> }
impl Hash for HostHash {
    fn start(&mut self) -> bool {
        self.data.clear();
        true
    }
    fn update(&mut self, bytes: &[u8]) -> bool {
        self.data.extend_from_slice(bytes);
        true
    }
    fn finish(&mut self, digest: &mut [u8; 32]) -> bool {
        *digest = crate::sha256::sha256(&self.data);
        true
    }
}
