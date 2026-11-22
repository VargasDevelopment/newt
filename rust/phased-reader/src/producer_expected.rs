//! Test-only, bounded transport for actual C++ observations; never a model.
extern crate std;
use newt_core::inflate::{BitBuffer, State, Status, COMPUTE_ADLER32, NON_WRAPPING, PARSE_ZLIB_HEADER};
use std::boxed::Box;
use std::sync::OnceLock;
use std::vec::Vec;

const FIXTURE: &[u8] = include_bytes!("../../tests/producer_forwarding/expected-outputs.bin.zlib");
const CAP: usize = 262144;
const FILE_CAP: usize = 32 * 1024 * 1024;
static INFLATED: OnceLock<Vec<u8>> = OnceLock::new();

fn inflate_fixture() -> Vec<u8> {
    assert!(FIXTURE.len() <= FILE_CAP, "producer capture file cap");
    let mut state = Box::new(State::new(BitBuffer::Bits64));
    let mut out = std::vec![0u8; CAP];
    let (mut ip, mut op) = (0usize, 0usize);
    loop {
        let cap = out.len() - op;
        let progress = state.decompress(
            &FIXTURE[ip..],
            &mut out,
            op,
            cap,
            PARSE_ZLIB_HEADER | NON_WRAPPING | COMPUTE_ADLER32,
        );
        ip += progress.consumed;
        op += progress.produced;
        if progress.status == Status::HasMoreOutput {
            let next = out.len().saturating_mul(2).min(FILE_CAP);
            assert!(next > out.len(), "producer capture file cap");
            out.resize(next, 0);
            continue;
        }
        assert_eq!(progress.status, Status::Done, "producer capture inflate failed");
        break;
    }
    assert_eq!(ip, FIXTURE.len(), "trailing compressed producer capture bytes");
    out.truncate(op);
    out
}

pub(super) struct Record {
    pub fields: [u32; 12],
    pub state: [u32; 16],
    pub starts: [u32; 32],
    pub data: &'static [u8],
    pub trace: Vec<u32>,
    pub probes: u32,
    pub facts: [u32; 2],
}

struct Reader { bytes: &'static [u8] }
impl Reader {
    fn take(&mut self, n: usize) -> &'static [u8] {
        assert!(n <= self.bytes.len(), "truncated producer capture");
        let (value, rest) = self.bytes.split_at(n);
        self.bytes = rest;
        value
    }
    fn word(&mut self) -> u32 { u32::from_le_bytes(self.take(4).try_into().unwrap()) }
    fn words<const N: usize>(&mut self) -> [u32; N] { core::array::from_fn(|_| self.word()) }
    fn blob(&mut self, cap: usize) -> &'static [u8] {
        let n = self.word() as usize;
        assert!(n <= cap, "producer capture capacity");
        self.take(n)
    }
}

// Every call checks the complete matrix. Full input bytes bind the selected
// observation, without a hash collision risk or a resident product cache.
pub(super) fn load(kind: u32, id: u32, input: &[u8]) -> Record {
    let bytes = INFLATED.get_or_init(inflate_fixture).as_slice();
    assert!(bytes.len() <= FILE_CAP, "producer capture file cap");
    let mut r = Reader { bytes };
    assert_eq!(r.take(4), b"NPF1");
    assert_eq!(r.word(), 55);
    let mut selected = None;
    for index in 0..55 {
        let expected = match index {
            0..=21 => (0, index),
            22..=33 => (1, index),
            34..=37 => (2, index - 34),
            38..=47 => (3, index - 38),
            _ => (4, index - 48),
        };
        let key = (r.word(), r.word());
        assert_eq!(key, expected, "complete ordered producer capture matrix");
        let bound_input = r.blob(16384);
        let small = key.0 < 3;
        let fields = if small { r.words() } else { [0; 12] };
        let state = if small { [0; 16] } else { r.words() };
        let starts = if small { [0; 32] } else { r.words() };
        let data = r.blob(if small { 4096 } else { CAP });
        let trace_len = r.word() as usize;
        assert!(trace_len <= if small { 8192 } else { CAP });
        let trace_bytes = r.take(trace_len * 4);
        if small {
            assert_eq!(fields[10] as usize, data.len());
            assert_eq!(fields[11] as usize, trace_len);
        } else {
            assert_eq!(state[15] as usize, trace_len);
        }
        let probes = if key.0 == 1 { r.word() } else { 0 };
        let facts = if key.0 == 4 { r.words() } else { [0; 2] };
        if key == (kind, id) {
            assert_eq!(input, bound_input, "producer capture input changed");
            let trace = trace_bytes.chunks_exact(4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap())).collect();
            selected = Some(Record { fields, state, starts, data, trace, probes, facts });
        }
    }
    assert!(r.bytes.is_empty(), "trailing producer capture bytes");
    selected.expect("missing producer capture request")
}
