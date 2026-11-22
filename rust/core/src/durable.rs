//! Generic two-slot durable record commits and validated loads.
//! Fixed scratch is local to each operation. A backend write acknowledgement
//! means its durability barrier succeeded; readable bytes alone are not proof.
use crate::wire::{fnv1a, get32, put32};
use core::marker::PhantomData;

pub const SLOT_KEYS: [&str; 2] = ["slot0", "slot1"];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageOpen {
    Opened,
    Missing,
    Failed,
    Damaged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlobRead {
    Present(usize),
    Missing,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum Commit {
    Saved,
    Unconfirmed,
}

/// Only commit_bytes can create the capability used to write through Backend.
/// Its private field prevents external code from constructing a raw-write token.
///
/// ```compile_fail
/// use newt_core::durable::DurableWrite;
/// let raw_write = DurableWrite(());
/// ```
pub struct DurableWrite(());

pub trait Backend {
    fn begin(&mut self, namespace: &str, readonly: bool) -> StorageOpen;
    /// ProgressStore lends its sole two-slot snapshot for this call only.
    /// Some metadata means these exact buffers were filled during begin;
    /// None requests ordinary reads. A failed/missing open owns no session.
    fn begin_progress(
        &mut self,
        namespace: &str,
        readonly: bool,
        _slots: &mut [[u8; crate::progress::BYTES]; 2],
    ) -> (StorageOpen, Option<[BlobRead; 2]>) {
        (self.begin(namespace, readonly), None)
    }
    /// Exact intended bytes are borrowed only during verification, never retained.
    fn readback_bytes(&mut self, key: &str, expected: &[u8], output: &mut [u8]) -> BlobRead {
        let _ = expected;
        self.read_blob(key, output)
    }
    fn end(&mut self);
    fn read_blob(&mut self, key: &str, output: &mut [u8]) -> BlobRead;
    fn put_bytes(&mut self, permit: DurableWrite, key: &str, bytes: &[u8]) -> usize;
    /// True only if every currently readable byte is durable, not cached after
    /// a failed sync. There is deliberately no optimistic default.
    fn readback_durable(&self) -> bool;
}

pub fn commit_bytes<B: Backend>(
    store: &mut B,
    key: &str,
    bytes: &[u8],
    readback: &mut [u8],
) -> Commit {
    if readback.len() != bytes.len() {
        return Commit::Unconfirmed;
    }
    for _ in 0..2 {
        if store.put_bytes(DurableWrite(()), key, bytes) == bytes.len()
            && store.readback_bytes(key, bytes, readback) == BlobRead::Present(bytes.len())
            && readback == bytes
        {
            return Commit::Saved;
        }
        if store.readback_durable() {
            break;
        }
    }
    Commit::Unconfirmed
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Load<V> {
    Loaded(V),
    Empty,
    Ambiguous,
    Failed,
}

pub trait Codec<const N: usize> {
    type Value: Copy;
    const NAMESPACE: &'static str;
    const REPLACE_DAMAGED: bool = false;
    fn known(bytes: &[u8]) -> bool;
    fn decode(bytes: &[u8]) -> Option<Self::Value>;
    fn encode(value: Self::Value, sequence: u32, bytes: &mut [u8; N]) -> bool;
}

/// The save attempt cannot be cancelled once it entered durable work.
pub struct Admission<'a> {
    pub quiet: Option<&'a mut dyn FnMut() -> bool>,
    pub entered: bool,
    pub deferred: bool,
}

impl Admission<'_> {
    pub fn ready(&mut self) -> bool {
        self.entered || self.quiet.as_mut().is_none_or(|quiet| quiet())
    }
    pub fn enter(&mut self) -> bool {
        if !self.ready() {
            self.deferred = true;
            return false;
        }
        self.entered = true;
        true
    }
}

pub(crate) struct Transaction<'a, B: Backend>(pub(crate) &'a mut B);
impl<B: Backend> Drop for Transaction<'_, B> {
    fn drop(&mut self) {
        self.0.end();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Choice {
    Empty,
    Slot(usize),
    Ambiguous,
}

struct Slots<const N: usize> {
    bytes: [[u8; N]; 2],
    valid: [bool; 2],
    present: bool,
    invalid: bool,
}

impl<const N: usize> Slots<N> {
    fn read<B: Backend, C: Codec<N>>(store: &mut B) -> Option<Self> {
        let mut slots = Self {
            bytes: [[0; N]; 2],
            valid: [false; 2],
            present: false,
            invalid: false,
        };
        for (i, key) in SLOT_KEYS.iter().enumerate() {
            match store.read_blob(key, &mut slots.bytes[i]) {
                BlobRead::Failed => return None,
                BlobRead::Missing => {}
                BlobRead::Present(observed) => {
                    slots.present = true;
                    // A reported extent beyond capacity is unknown, never a
                    // reason to slice beyond the caller-owned buffer.
                    if observed > N || !C::known(&slots.bytes[i][..observed]) {
                        return None;
                    }
                    slots.valid[i] = C::decode(&slots.bytes[i][..observed]).is_some();
                    slots.invalid |= !slots.valid[i];
                }
            }
        }
        Some(slots)
    }

    fn choose(&self) -> Choice {
        match self.valid {
            [false, false] => Choice::Empty,
            [true, false] => Choice::Slot(0),
            [false, true] => Choice::Slot(1),
            [true, true] => {
                let delta = get32(&self.bytes[0], 8).wrapping_sub(get32(&self.bytes[1], 8));
                if delta == 0x8000_0000 || (delta == 0 && self.bytes[0] != self.bytes[1]) {
                    Choice::Ambiguous
                } else {
                    Choice::Slot(usize::from(delta >= 0x8000_0000))
                }
            }
        }
    }
}

fn stamp<const N: usize>(bytes: &mut [u8; N], sequence: u32) {
    put32(bytes, 8, sequence);
    let checksum = fnv1a(&bytes[..N - 4]);
    put32(bytes, N - 4, checksum);
}

pub struct RecordStore<'a, B: Backend, C: Codec<N>, const N: usize, const ALLOW_TORN: bool = true> {
    backend: &'a mut B,
    codec: PhantomData<C>,
}

impl<'a, B: Backend, C: Codec<N>, const N: usize, const ALLOW_TORN: bool>
    RecordStore<'a, B, C, N, ALLOW_TORN>
{
    pub fn new(backend: &'a mut B) -> Self {
        const {
            assert!(
                N >= 16,
                "durable records require header, generation and checksum"
            );
        }
        Self {
            backend,
            codec: PhantomData,
        }
    }

    pub fn load(&mut self) -> Load<C::Value> {
        match self.backend.begin(C::NAMESPACE, true) {
            StorageOpen::Missing => return Load::Empty,
            StorageOpen::Opened => {}
            _ => return Load::Failed,
        }
        let slots = {
            let transaction = Transaction(self.backend);
            let Some(slots) = Slots::<N>::read::<B, C>(transaction.0) else {
                return Load::Failed;
            };
            slots
        };
        if !ALLOW_TORN && slots.invalid {
            return Load::Failed;
        }
        match slots.choose() {
            Choice::Ambiguous => Load::Ambiguous,
            Choice::Empty => {
                if slots.present {
                    Load::Failed
                } else {
                    Load::Empty
                }
            }
            Choice::Slot(index) => {
                C::decode(&slots.bytes[index]).map_or(Load::Failed, Load::Loaded)
            }
        }
    }

    pub fn load_matching(
        &mut self,
        mut matches: impl FnMut(C::Value) -> bool,
        mut same: impl FnMut(C::Value, C::Value) -> bool,
    ) -> Load<C::Value> {
        match self.backend.begin(C::NAMESPACE, true) {
            StorageOpen::Missing => return Load::Empty,
            StorageOpen::Opened => {}
            _ => return Load::Failed,
        }
        let slots = {
            let transaction = Transaction(self.backend);
            let Some(slots) = Slots::<N>::read::<B, C>(transaction.0) else {
                return Load::Failed;
            };
            slots
        };
        if !ALLOW_TORN && slots.invalid {
            return Load::Failed;
        }
        let newest = slots.choose();
        if newest == Choice::Ambiguous {
            return Load::Ambiguous;
        }
        let mut found = None;
        for i in 0..2 {
            let Some(candidate) = slots.valid[i].then(|| C::decode(&slots.bytes[i])).flatten()
            else {
                continue;
            };
            if !matches(candidate) {
                continue;
            }
            if found.is_some_and(|chosen| !same(chosen, candidate)) {
                return Load::Ambiguous;
            }
            if found.is_none() || newest == Choice::Slot(i) {
                found = Some(candidate);
            }
        }
        found.map_or_else(
            || {
                if newest != Choice::Empty || !slots.present {
                    Load::Empty
                } else {
                    Load::Failed
                }
            },
            Load::Loaded,
        )
    }

    pub fn save(&mut self, value: C::Value) -> Commit {
        self.save_inner(value, None)
    }

    pub fn save_admitted(&mut self, value: C::Value, admission: &mut Admission<'_>) -> Commit {
        self.save_inner(value, Some(admission))
    }

    fn save_inner(&mut self, value: C::Value, admission: Option<&mut Admission<'_>>) -> Commit {
        let mut encoded = [0; N];
        if !C::encode(value, 0, &mut encoded)
            || admission.is_some_and(|gate| !gate.enter())
            || self.backend.begin(C::NAMESPACE, false) != StorageOpen::Opened
        {
            return Commit::Unconfirmed;
        }
        let transaction = Transaction(self.backend);
        let Some(mut slots) = Slots::<N>::read::<B, C>(transaction.0) else {
            return Commit::Unconfirmed;
        };
        let selected = match slots.choose() {
            Choice::Ambiguous => return Commit::Unconfirmed,
            Choice::Empty if slots.present && !C::REPLACE_DAMAGED => return Commit::Unconfirmed,
            Choice::Empty => None,
            Choice::Slot(i) => Some(i),
        };
        let sequence = selected.map_or(1, |i| get32(&slots.bytes[i], 8));
        stamp(&mut encoded, sequence);
        if selected.is_some_and(|i| encoded == slots.bytes[i])
            && (ALLOW_TORN || !slots.invalid)
            && transaction.0.readback_durable()
        {
            return Commit::Saved;
        }
        if selected.is_some() {
            stamp(&mut encoded, sequence.wrapping_add(1));
        }
        let target = if selected == Some(0) { 1 } else { 0 };
        commit_bytes(
            transaction.0,
            SLOT_KEYS[target],
            &encoded,
            &mut slots.bytes[target],
        )
    }

    pub fn save_protected(&mut self, value: C::Value, protected: C::Value) -> Commit {
        let mut encoded = [0; N];
        let mut protected_bytes = [0; N];
        if !C::encode(value, 0, &mut encoded)
            || !C::encode(protected, 0, &mut protected_bytes)
            || self.backend.begin(C::NAMESPACE, false) != StorageOpen::Opened
        {
            return Commit::Unconfirmed;
        }
        let transaction = Transaction(self.backend);
        let Some(mut slots) = Slots::<N>::read::<B, C>(transaction.0) else {
            return Commit::Unconfirmed;
        };
        let Choice::Slot(newest) = slots.choose() else {
            return Commit::Unconfirmed;
        };
        let mut protected_slot = None;
        for i in 0..2 {
            if !slots.valid[i] {
                continue;
            }
            stamp(&mut protected_bytes, get32(&slots.bytes[i], 8));
            if protected_bytes == slots.bytes[i] && (protected_slot.is_none() || i == newest) {
                protected_slot = Some(i);
            }
        }
        let Some(kept) = protected_slot else {
            return Commit::Unconfirmed;
        };
        let sequence = get32(&slots.bytes[newest], 8);
        stamp(&mut encoded, sequence);
        if encoded == slots.bytes[newest] && transaction.0.readback_durable() {
            return Commit::Saved;
        }
        stamp(&mut encoded, sequence.wrapping_add(1));
        let target = 1 - kept;
        commit_bytes(
            transaction.0,
            SLOT_KEYS[target],
            &encoded,
            &mut slots.bytes[target],
        )
    }
}

// Only the actual schemas are public. There is no arbitrary magic/max pair
// that could accidentally serialize a known header into an unknown namespace.
macro_rules! scalar_codec {
    ($name:ident, $namespace:literal, $magic:literal, $max:literal) => {
        pub struct $name;
        impl Codec<20> for $name {
            type Value = u32;
            const NAMESPACE: &'static str = $namespace;
            fn known(bytes: &[u8]) -> bool {
                bytes.len() == 20 && get32(bytes, 0) == $magic && get32(bytes, 4) == 1
            }
            fn decode(bytes: &[u8]) -> Option<u32> {
                if !Self::known(bytes) || get32(bytes, 16) != fnv1a(&bytes[..16]) {
                    return None;
                }
                let value = get32(bytes, 12);
                (value <= $max).then_some(value)
            }
            fn encode(value: u32, sequence: u32, bytes: &mut [u8; 20]) -> bool {
                if value > $max {
                    return false;
                }
                bytes.fill(0);
                put32(bytes, 0, $magic);
                put32(bytes, 4, 1);
                put32(bytes, 8, sequence);
                put32(bytes, 12, value);
                let checksum = fnv1a(&bytes[..16]);
                put32(bytes, 16, checksum);
                true
            }
        }
    };
}
scalar_codec!(ClockFormatCodec, "newt_clock", 0x4354_574e, 1);
scalar_codec!(TimeZoneCodec, "newt_zone", 0x5a54_574e, 14);
scalar_codec!(SleepScreenCodec, "newt_sleep", 0x5054_574e, 7);
// Developer page "Test builds": 0 Off, 1 On. Update mode reads it too.
scalar_codec!(TestBuildsCodec, "newt_builds", 0x5554_574e, 1);

/// Restore the durable preference independently of reading/resume state. The
/// caller supplies the product default; missing or corrupt storage is not repaired.
pub fn restore<B: Backend, C: Codec<N>, const N: usize>(
    backend: &mut B,
    default: C::Value,
) -> C::Value {
    match RecordStore::<B, C, N>::new(backend).load() {
        Load::Loaded(value) => value,
        _ => default,
    }
}

/// Publish only a verified commit. A lost acknowledgement may be reconciled
/// from exact durable readback; a buffered readback cannot authorize publication.
pub fn commit_publish<B: Backend, C: Codec<N>, const N: usize>(
    backend: &mut B,
    intended: C::Value,
    published: &mut C::Value,
) -> Commit
where
    C::Value: PartialEq,
{
    let result = RecordStore::<B, C, N>::new(backend).save(intended);
    let reconciled = result == Commit::Unconfirmed
        && backend.readback_durable()
        && RecordStore::<B, C, N>::new(backend).load() == Load::Loaded(intended);
    if result == Commit::Saved || reconciled {
        *published = intended;
        Commit::Saved
    } else {
        Commit::Unconfirmed
    }
}
