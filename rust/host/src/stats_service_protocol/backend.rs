// SPDX-License-Identifier: MIT
use newt_core::{
    durable::*, reading_stats_store::StatsBackend, stats_service_backend::NamespaceBackend,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
// Host-only event clock observes callback position without aliasing the store.
static EVENT_TICK: AtomicU64 = AtomicU64::new(0);
pub fn tick() -> u64 {
    EVENT_TICK.fetch_add(1, Ordering::Relaxed)
}
pub fn reset_tick() {
    EVENT_TICK.store(0, Ordering::Relaxed);
}
#[derive(Default)]
pub struct Host {
    pub visible: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub durable: BTreeMap<String, BTreeMap<String, Vec<u8>>>,
    pub order: Vec<String>,
    pub trace: Vec<String>,
    pub current: String,
    pub opens: BTreeMap<String, u8>,
    pub unavailable: bool,
    pub read_fail: bool,
    pub fail_from: usize,
    pub lost: usize,
    pub puts: usize,
    pub aside_fail: bool,
    pub list_fail: bool,
    pub synced: bool,
    pub fail_namespace: String,
    pub namespace_fail_from: usize,
    pub namespace_puts: usize,
}
impl Host {
    fn event(&mut self, value: String) {
        tick();
        self.trace.push(value);
    }
    pub fn inject(&mut self, n: &str, k: &str, b: Vec<u8>) {
        if !self.order.iter().any(|s| s == n) {
            self.order.push(n.into());
        }
        self.visible
            .entry(n.into())
            .or_default()
            .insert(k.into(), b);
        self.durable = self.visible.clone();
        self.synced = true;
    }
}
impl Backend for Host {
    fn begin(&mut self, n: &str, ro: bool) -> StorageOpen {
        self.event(format!("begin:{n}:{}", u8::from(ro)));
        self.current = n.into();
        match self.opens.get(n).copied().unwrap_or(0) {
            1 => return StorageOpen::Missing,
            2 => return StorageOpen::Failed,
            3 => return StorageOpen::Damaged,
            _ => {}
        }
        if self.unavailable {
            return StorageOpen::Failed;
        }
        if ro && !self.visible.contains_key(n) {
            return StorageOpen::Missing;
        }
        if !self.visible.contains_key(n) {
            self.order.push(n.into());
            self.visible.insert(n.into(), BTreeMap::new());
        }
        StorageOpen::Opened
    }
    fn end(&mut self) {
        self.event("end".into());
    }
    fn read_blob(&mut self, k: &str, out: &mut [u8]) -> BlobRead {
        self.event(format!("read:{k}"));
        if self.read_fail {
            return BlobRead::Failed;
        }
        let Some(b) = self.visible.get(&self.current).and_then(|m| m.get(k)) else {
            return BlobRead::Missing;
        };
        let n = b.len().min(out.len());
        out[..n].copy_from_slice(&b[..n]);
        BlobRead::Present(b.len())
    }
    fn put_bytes(&mut self, _: DurableWrite, k: &str, b: &[u8]) -> usize {
        self.event(format!("put:{k}:{}", super::hex(b)));
        self.puts += 1;
        if self.current == self.fail_namespace {
            self.namespace_puts += 1;
        }
        self.visible
            .entry(self.current.clone())
            .or_default()
            .insert(k.into(), b.to_vec());
        self.synced = false;
        if (self.fail_from != 0 && self.puts >= self.fail_from)
            || self.lost != 0
            || (self.namespace_fail_from != 0
                && self.current == self.fail_namespace
                && self.namespace_puts >= self.namespace_fail_from)
        {
            self.lost = self.lost.saturating_sub(1);
            return 0;
        }
        self.durable = self.visible.clone();
        self.synced = true;
        b.len()
    }
    fn readback_durable(&self) -> bool {
        self.synced
    }
}
impl StatsBackend for Host {
    fn reading_state_unavailable(&self) -> bool {
        self.unavailable
    }
    fn set_aside(&mut self, n: &str) -> bool {
        self.event(format!("aside:{n}"));
        if self.aside_fail {
            return false;
        }
        self.visible.remove(n);
        self.durable = self.visible.clone();
        self.synced = true;
        true
    }
    fn repair_in_place(&mut self, n: &str, b: &[u8]) -> bool {
        self.event(format!("repair:{n}:{}", super::hex(b)));
        if self.aside_fail {
            return false;
        }
        self.inject(n, "slot0", b.to_vec());
        self.visible.get_mut(n).unwrap().remove("slot1");
        self.durable = self.visible.clone();
        true
    }
}
impl NamespaceBackend for Host {
    fn for_each_namespace(
        &mut self,
        prefix: &str,
        visitor: &mut dyn FnMut(&mut Self, &str) -> bool,
    ) -> bool {
        self.event(format!("list:{prefix}"));
        if self.list_fail {
            return false;
        }
        // Stable insertion order, not a sorted replacement for the backend order.
        let names = self.order.clone();
        for n in names {
            if n.starts_with(prefix) && self.visible.contains_key(&n) && !visitor(self, &n) {
                return false;
            }
        }
        true
    }
}
