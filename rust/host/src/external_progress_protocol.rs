// SPDX-License-Identifier: MIT
use newt_core::{durable::*, external_progress_store::*, progress::*, progress_store::*};
use std::collections::BTreeMap;
type State = BTreeMap<(String, String), Vec<u8>>;
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|x| format!("{x:02x}")).collect()
}
fn state(s: &State) -> String {
    s.iter()
        .map(|((n, k), v)| format!("{n}/{k}={}", hex(v)))
        .collect::<Vec<_>>()
        .join(",")
}
#[derive(Default)]
struct Host {
    visible: State,
    durable: State,
    namespace: String,
    trace: Vec<String>,
    open: u8,
    fail_read: usize,
    corrupt_read: usize,
    readback_override: Vec<u8>,
    reads: usize,
    failures: u8,
    durable_reads: bool,
}
impl Backend for Host {
    fn begin(&mut self, n: &str, r: bool) -> StorageOpen {
        self.trace.push(format!("begin:{n}:{}", u8::from(r)));
        self.namespace = n.into();
        match self.open {
            1 => StorageOpen::Missing,
            2 => StorageOpen::Failed,
            3 => StorageOpen::Damaged,
            _ => {
                if r && !self.visible.keys().any(|(name, _)| name == n) {
                    StorageOpen::Missing
                } else {
                    StorageOpen::Opened
                }
            }
        }
    }
    fn end(&mut self) {
        self.trace.push("end".into());
    }
    fn read_blob(&mut self, k: &str, out: &mut [u8]) -> BlobRead {
        self.trace.push(format!("read:{k}"));
        self.reads += 1;
        if self.reads == self.fail_read {
            return BlobRead::Failed;
        }
        let Some(b) = self.visible.get(&(self.namespace.clone(), k.into())) else {
            return BlobRead::Missing;
        };
        let count = out.len().min(b.len());
        out[..count].copy_from_slice(&b[..count]);
        if self.reads == self.corrupt_read && count != 0 {
            out[0] ^= 1;
        }
        if self.reads >= 4
            && matches!(k, "slot0" | "slot1")
            && self.readback_override.len() == 52
            && out.len() == 52
        {
            out.copy_from_slice(&self.readback_override);
        }
        BlobRead::Present(b.len())
    }
    fn put_bytes(&mut self, _: DurableWrite, k: &str, b: &[u8]) -> usize {
        self.trace.push(format!("put:{k}:{}", hex(b)));
        self.visible
            .insert((self.namespace.clone(), k.into()), b.to_vec());
        if self.failures != 0 {
            self.failures -= 1;
            if self.durable_reads {
                self.durable = self.visible.clone();
            }
            return 0;
        }
        self.durable = self.visible.clone();
        b.len()
    }
    fn readback_durable(&self) -> bool {
        self.durable_reads
    }
}
struct Input {
    bytes: Vec<u8>,
    at: usize,
}
impl Input {
    fn take(&mut self, n: usize) -> Result<Vec<u8>, String> {
        let end = self.at.checked_add(n).ok_or("overflow")?;
        let v = self.bytes.get(self.at..end).ok_or("short")?.to_vec();
        self.at = end;
        Ok(v)
    }
    fn n(&mut self, n: usize) -> Result<u64, String> {
        Ok(self
            .take(n)?
            .iter()
            .enumerate()
            .fold(0, |v, (i, b)| v | (u64::from(*b) << (8 * i))))
    }
    fn blob(&mut self) -> Result<Vec<u8>, String> {
        let n = self.n(2)? as usize;
        self.take(n)
    }
}
fn load_code(result: Load<()>) -> u8 {
    match result {
        Load::Loaded(()) => 0,
        Load::Empty => 1,
        Load::Ambiguous => 2,
        Load::Failed => 3,
    }
}
/// One line: xp.load|xp.checkpoint|xp.peers|xp.save|xp.save-peers SPACE hex payload.
pub fn run(line: &str) -> Result<String, String> {
    let (op, raw) = line.split_once(' ').ok_or("command")?;
    if raw.len() % 2 != 0 {
        return Err("odd hex".into());
    }
    let bytes = (0..raw.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).map_err(|_| "hex".to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut r = Input { bytes, at: 0 };
    let has_digest = r.n(1)? != 0;
    let digest: [u8; 32] = r.take(32)?.try_into().map_err(|_| "digest")?;
    let mut value = Progress::decode(&r.take(BYTES)?).ok_or("progress")?;
    value.session_milliseconds = r.n(4)? as u32;
    let protected_present = r.n(1)? != 0;
    let checkpoint = r.n(4)? as u32;
    let strict = r.n(1)? != 0;
    let cancel = r.n(1)? != 0;
    let mut host = Host {
        open: r.n(1)? as u8,
        fail_read: r.n(2)? as usize,
        failures: r.n(1)? as u8,
        durable_reads: r.n(1)? != 0,
        corrupt_read: r.n(2)? as usize,
        ..Host::default()
    };
    host.readback_override = r.blob()?;
    let count = r.n(2)?;
    for _ in 0..count {
        let n = String::from_utf8(r.blob()?).map_err(|_| "namespace")?;
        let k = String::from_utf8(r.blob()?).map_err(|_| "key")?;
        let v = r.blob()?;
        host.visible.insert((n, k), v);
    }
    if r.at != r.bytes.len() {
        return Err("trailing".into());
    }
    host.durable = host.visible.clone();
    let mut quiet = || !cancel;
    let mut admission = Admission {
        quiet: Some(&mut quiet),
        entered: false,
        deferred: false,
    };
    let mut peers = PeerLocators {
        slots: [
            Locator {
                spine: 71,
                character: 72,
            },
            Locator {
                spine: 73,
                character: 74,
            },
        ],
        present: 3,
    };
    let (result, status, book, namespace) = {
        let mut store = ExternalProgressStore::new(&mut host, has_digest.then_some(&digest));
        let result = match op {
            "xp.load" => load_code(store.load(&mut value)),
            "xp.checkpoint" => load_code(store.load_checkpoint(&mut value, checkpoint)),
            "xp.peers" => load_code(store.load_peer_locators(&mut peers)),
            "xp.save" | "xp.save-peers" => u8::from(
                store.save(
                    &mut value,
                    if op == "xp.save-peers" {
                        Some(&mut peers)
                    } else {
                        None
                    },
                    protected_present.then_some(checkpoint),
                    strict,
                    Some(&mut admission),
                ) == Commit::Saved,
            ),
            _ => return Err("operation".into()),
        };
        (
            result,
            store.status() as u8,
            store.book_id(),
            store.storage_namespace().to_string(),
        )
    };
    let mut output = value.encode().ok_or("result")?.to_vec();
    output.extend(value.session_milliseconds.to_le_bytes());
    output.push(peers.present);
    for l in peers.slots {
        output.extend(l.spine.to_le_bytes());
        output.extend(l.character.to_le_bytes());
    }
    Ok(format!(
        "{result}|{status}|{book}|{namespace}|{}|{}{}|{}|{}|{}",
        hex(&output),
        u8::from(admission.entered),
        u8::from(admission.deferred),
        host.trace.join(","),
        state(&host.visible),
        state(&host.durable)
    ))
}
