# Reader UI and SampleReader owner

`newt-reader-ui` implements Newt's reading screen: dispatch, lifecycle,
overlay/chrome composition, book-open working view, reader statistics panels,
and the external and embedded-sample reader owners (`no_std`, `#![forbid(unsafe_code)]`).

`OperationReader` is consumed per operation so parser, resource, source and
frame loans end before publication or continuation. `VerifiedSource` owns the
`ImportedRevision` source loan with explicit reset, and `PageWork` keeps its
completion field private so callers cannot manufacture completion from status
alone.

`native-host` (off by default) enables the host test binary; `embedded-sample`
enables the built-in sample book owner. MIT and Inter OFL notices are in
`LICENSE`, `NOTICES.txt` and `Inter-OFL.txt`. Provenance records stay in
`PROVENANCE.json` and [docs/provenance.md](../../docs/provenance.md).
