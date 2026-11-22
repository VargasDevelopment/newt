// SPDX-License-Identifier: MIT
// Host transport only. Actual canonical parser + Expat + styles + phase banks.
use newt_core::{
    xml_session::{Handler, ParserBackend},
    zip_container::Status,
    zip_window::source::BookSource,
};
use newt_phased_reader::{
    dynamic_metadata_fixture, optional_toc_failure_fixture,
    phase_storage::{BANK_BYTES, BuildLoan, Storage},
    toc_scope_policy_fixture,
};
use newt_target_expat::Expat;
struct Source {
    bytes: Vec<u8>,
    fail: bool,
    fail_at: Option<usize>,
}
impl BookSource for Source {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn read_at(&mut self, at: u64, out: &mut [u8]) -> i32 {
        if self.fail {
            return -1;
        }
        let Ok(at) = usize::try_from(at) else {
            return -1;
        };
        if self.fail_at.is_some_and(|limit| at >= limit) {
            return -1;
        }
        if at > self.bytes.len() {
            return -1;
        }
        let take = out.len().min(self.bytes.len() - at);
        out[..take].copy_from_slice(&self.bytes[at..at + take]);
        take as i32
    }
}
#[derive(Default)]
struct Xml {
    inner: Expat,
    fail: bool,
    nav_passes: usize,
    fail_navigation_fill: bool,
}
impl ParserBackend for Xml {
    fn create(&mut self) -> bool {
        !self.fail && self.inner.create()
    }
    fn feed(&mut self, b: &[u8], last: bool, h: &mut dyn Handler) -> bool {
        let navigation = b.windows(b"epub:type".len()).any(|s| s == b"epub:type");
        if navigation {
            self.nav_passes += 1;
        }
        let ok = self.inner.feed(b, last, h);
        ok && !(navigation && self.nav_passes == 2 && self.fail_navigation_fill)
    }
    fn close(&mut self) {
        self.inner.close();
    }
}
#[repr(align(16))]
struct Bank([u8; BANK_BYTES]);
struct Loan(Box<Bank>);
impl BuildLoan for Loan {
    fn with_parse<R>(&mut self, op: impl for<'a> FnOnce(&'a mut [u8]) -> R) -> Result<R, Status> {
        let result = op(&mut self.0.0);
        self.0.0.fill(255);
        Ok(result)
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let fixture = std::path::Path::new(&args[1]);
    let pressure_entries = 1;
    let mut shared = Box::new(Bank([0; BANK_BYTES]));
    let mut loan = Loan(Box::new(Bank([0; BANK_BYTES])));
    let mut operation = |entries, short, change: &str, expected| {
        let mut source = Source {
            bytes: std::fs::read(
                fixture
                    .join(if entries == 0 { "full" } else { "pressure" })
                    .join("base.epub"),
            )
            .unwrap(),
            fail: false,
            fail_at: None,
        };
        let mut backend = Xml::default();
        let mut storage = Storage::new(&mut shared.0);
        let result = dynamic_metadata_fixture(
            &mut source,
            &mut storage,
            &mut loan,
            &mut backend,
            short,
            |source, backend| match change {
                "same" => (),
                "io" => source.fail = true,
                "oom" => backend.fail = true,
                name => source.bytes = std::fs::read(fixture.join("pressure").join(name)).unwrap(),
            },
            expected,
        );
        assert!(loan.0.0.iter().all(|b| *b == 255));
        result
    };
    let full = operation(0, None, "same", Status::Ok);
    if args.get(2).is_some_and(|a| a == "--measure") {
        // Actual private Home placement and parsed strings determine free bytes.
        println!(
            "{} {} {}",
            full.unwrap().3,
            core::mem::size_of::<newt_core::zip_container::Entry>() + 79,
            newt_phased_reader::styles::EMBEDDED
        );
        return;
    }
    assert_eq!(
        operation(pressure_entries, Some(true), "same", Status::Ok)
            .map(|row| (row.0, row.1, row.2)),
        full.map(|row| (row.0, row.1, row.2))
    );
    // Same-length, same-offset stored chapter replacements change only head topology.
    operation(pressure_entries, Some(true), "events.epub", Status::Stale);
    operation(pressure_entries, Some(true), "mask.epub", Status::Stale);
    // Failed scan clears its record (zero topology); error must win over Stale.
    operation(pressure_entries, Some(true), "io", Status::IoError);
    operation(pressure_entries, Some(true), "oom", Status::OutOfMemory);
    drop(operation);
    let mut complete = |entries, short| {
        let mut source = Source {
            bytes: std::fs::read(
                fixture
                    .join(if entries == 0 { "full" } else { "pressure" })
                    .join("small.epub"),
            )
            .unwrap(),
            fail: false,
            fail_at: None,
        };
        let mut backend = Xml::default();
        let mut storage = Storage::new(&mut shared.0);
        let result = dynamic_metadata_fixture(
            &mut source,
            &mut storage,
            &mut loan,
            &mut backend,
            short,
            |_, backend| backend.fail = true,
            Status::Ok,
        );
        assert!(loan.0.0.iter().all(|b| *b == 255));
        result
    };
    let full = complete(0, None);
    assert_eq!(
        complete(pressure_entries, Some(false)).map(|row| (row.0, row.1, row.2)),
        full.map(|row| (row.0, row.1, row.2))
    );
    for exhaust_strings in [false, true] {
        let mut source = Source {
            bytes: std::fs::read(fixture.join("full/base.epub")).unwrap(),
            fail: false,
            fail_at: None,
        };
        let mut backend = Xml {
            fail_navigation_fill: !exhaust_strings,
            ..Xml::default()
        };
        let mut storage = Storage::new(&mut shared.0);
        optional_toc_failure_fixture(
            &mut source,
            &mut storage,
            &mut loan,
            &mut backend,
            exhaust_strings,
        );
        assert_eq!(backend.nav_passes, 2, "actual count and partial/error fill");
        assert!(loan.0.0.iter().all(|b| *b == 255));
    }
    toc_policy_cases();
    println!("counted physical metadata/TOC and capture cases passed");
}

fn toc_policy_cases() {
    #[repr(align(16))]
    struct UnalignedEnd([u8; 66]);
    let empty =
        b"<html xmlns:epub='http://www.idpf.org/2007/ops'><nav epub:type='toc'></nav></html>";
    for fail_fill in [false, true] {
        let mut source = Source {
            bytes: empty.to_vec(),
            fail: false,
            fail_at: None,
        };
        let mut bytes = UnalignedEnd([0; 66]);
        toc_scope_policy_fixture(
            &mut source,
            &mut Xml::default(),
            &mut bytes.0,
            empty.len() as u32,
            |source, _| source.fail = fail_fill,
            if fail_fill {
                Err(Status::IoError)
            } else {
                Ok(0)
            },
        );
    }
    // Long valid XML permits a later source failure after handler OOM.
    let prefix = b"<html xmlns:epub='http://www.idpf.org/2007/ops'><nav epub:type='toc'><ol><li><a href='c.xhtml'>Title</a></li></ol></nav>";
    let mut valid = prefix.to_vec();
    valid.extend(std::iter::repeat_n(b' ', 5000));
    valid.extend_from_slice(b"</html>");
    for (capacity, change, expected) in [
        (8192, "same", Ok(1)),
        (0, "same", Err(Status::OutOfMemory)),
        (128, "same", Ok(1)),
        (
            std::mem::size_of::<newt_core::package_parsers::TocEntry>(),
            "same",
            Err(Status::OutOfMemory),
        ),
        (
            std::mem::size_of::<newt_core::package_parsers::TocEntry>(),
            "xml",
            Err(Status::ParseError),
        ),
        (
            std::mem::size_of::<newt_core::package_parsers::TocEntry>(),
            "io",
            Err(Status::IoError),
        ),
        (8192, "parser", Err(Status::OutOfMemory)),
    ] {
        let mut source = Source {
            bytes: valid.clone(),
            fail: false,
            fail_at: None,
        };
        let mut backend = Xml::default();
        let mut bank = Box::new(Bank([0; BANK_BYTES]));
        toc_scope_policy_fixture(
            &mut source,
            &mut backend,
            &mut bank.0[..capacity],
            valid.len() as u32,
            |source, backend| match change {
                "xml" => *source.bytes.last_mut().unwrap() = b'!',
                "io" => source.fail_at = Some(2048),
                "parser" => backend.fail = true,
                _ => (),
            },
            expected,
        );
    }
    for (capacity, expected) in [(128, Status::Ok), (0, Status::OutOfMemory)] {
        let mut source = Source {
            bytes: valid.clone(),
            fail: false,
            fail_at: None,
        };
        let mut backend = Xml::default();
        let mut bytes = [0; 128];
        let mut arena = newt_core::zip_window::arena::Arena::new(&mut bytes[..capacity]);
        let mut xml = [0; 2048];
        let mut entries = [newt_core::package_parsers::TocEntry::default(); 1];
        let mut count = 97;
        let status = newt_core::package_parsers::parse_toc(
            &mut newt_core::package_parsers::Workflow {
                policy: None,
                source: &mut source,
                backend: &mut backend,
                decoder: None,
                buffer: Some(&mut xml),
                filter: None,
            },
            &newt_core::zip_container::Entry::raw(valid.len() as u32),
            b"",
            &mut arena,
            newt_core::package_parsers::TocKind::Nav,
            &mut entries,
            &mut count,
        );
        assert_eq!(status, expected);
        assert_eq!(count, if expected == Status::Ok { 1 } else { 97 });
    }
    // Count XML wins before impossible typed allocation. Fixed-slice policy too.
    let mut malformed = valid;
    *malformed.last_mut().unwrap() = b'!';
    let mut source = Source {
        bytes: malformed,
        fail: false,
        fail_at: None,
    };
    let length = source.bytes.len() as u32;
    toc_scope_policy_fixture(
        &mut source,
        &mut Xml::default(),
        &mut [],
        length,
        |_, _| panic!("failed count must not allocate/fill"),
        Err(Status::ParseError),
    );
    let mut backend = Xml::default();
    let mut xml = [0; 2048];
    let mut arena = newt_core::zip_window::arena::Arena::default();
    let mut count = 97;
    let status = newt_core::package_parsers::parse_toc(
        &mut newt_core::package_parsers::Workflow {
                policy: None,
            source: &mut source,
            backend: &mut backend,
            decoder: None,
            buffer: Some(&mut xml),
            filter: None,
        },
        &newt_core::zip_container::Entry::raw(length),
        b"",
        &mut arena,
        newt_core::package_parsers::TocKind::Nav,
        &mut [],
        &mut count,
    );
    assert_eq!(status, Status::ParseError);
    assert_eq!(count, 97, "failed parse must not publish count");
}
