# Provenance

Source reference: Newt bc9ffda52b85f277e46c63718ac88d62fba5d368, as specified by the owner. Local files read from the frozen Root334b2025 worktree; this packet did not perform a git history comparison. New Rust orchestration derives from these MIT Newt sources. Reused Core/Atlas/Style/Nav/Sink algorithms retain their existing SDK/Newt license and provenance closures. No original closure is copied or relicensed.

Read-source SHA-256 (documentation only, not a new source guard):

- `include/newt/phased_reader.h`: `368b447b0c8a06e30350632ad2fd4774912fbb7b2578b4def21f4ff4ac2861f1`
- `src/phased_reader.cpp`: `e2ffc7ce94315d701381cccdfdde3435408a66a82977defbb92a89db29e7746c`
- `src/phased_reader_lifecycle.cpp`: `fc74e314ac2ab5724509851dc85f39728396a520c486011c38f5cd760000ac0b`
- `src/phased_reader_chapters.cpp`: `2c4203cb56a58e15d227d18d8f555b079e7555c85a9bbba7ca53d00a206dcbfe`
- `src/phased_reader_semantics.cpp`: `425ed8f651fd65daf9dee439f730a2173ab13f0e331d25bd8706ad7781dc92b5`
- `src/phased_reader_styles.cpp`: `0ba479c30f9c1ab0efa9539d02392b22a9bc9c0046c554e5b94ce1fc57ca6f2d`
- `src/phased_reader_metadata.h`: `65b0f118ae960d2b83b516f048ddcc349311ab906537f909612f36cb2ffdcdba`
- `src/reader_anchor_validation.cpp`: `3fbd132226df25bd15bd3049040b3ca74b5097811f347d5ff543aed3e14332f9`

Historical preparation path map (superseded by the adopted map below):
- Actual Root `rust/core/src/{chapter_layout,zip_container.rs,xml_session.rs,package_parsers.rs,zip_window,css.rs}` and `rust/reader-atlas`.
- Sibling `reader-navigation/src` and `phased-sink/src`, unchanged.
- Sibling `reader-styles/src/{styles,scan,watch}.rs`, unchanged path imports.
- Reviewed `epub-canonical-adoption/rust/core/src/{admission,canonical,phased_metadata}.rs`, pending Root adoption; no fork linked.

Keep this packet LICENSE (Newt MIT), Root SDK MIT/NOTICE, the existing font OFL files and the existing vendor licenses. No generated/native output is included.

## Adopted paths

The workspace now uses canonical `rust/core`, `rust/reader-atlas`,
`rust/reader-navigation` and `rust/phased-sink` packages. Styles, scan and watch
are ordinary modules in `rust/phased-reader/src`; no sibling path import or
admission-candidate crate remains. Admission, canonical metadata and semantic
counting come from the single Core package. Native build inputs stay in
`rust/tests/phased_reader`; the standalone C++ parity packets were retired.
The historical read hashes above record the original preparation, not expected
hashes for live Rust files. Firmware and physical resource acceptance remain
separate from the recorded host and RV32 library proofs.
