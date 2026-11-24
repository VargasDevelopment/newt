#!/usr/bin/env python3
"""Focused host test for the synthetic large-EPUB boundary corpus (#61).

Verifies generator determinism, builds the real Rust host runner over
newt_core::admission::admit and PhasedReader::open_phased (real 48 KiB banks),
and checks each boundary case. Runs the fast subset by default so the normal
gate stays quick; pass --full for the complete 64 MiB and multi-thousand-page
corpus, and --measure to print per-case timings and arena metrics.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from gate_tier import negative_controls
import large_epub_corpus as corpus

ADMISSION = "core/src/admission.rs"
ZIP_RULES = "core/src/zip_record_rules.rs"
ZIP_CONTAINER = "core/src/zip_container.rs"
PAGE_CACHE = "core/src/page_cache.rs"
PHASE_STORAGE = "phased-reader/src/phase_storage.rs"
PHASED = "phased-reader/src/reader_phased.rs"
READER = "phased-reader/src/reader.rs"

SYNC_SIX_CHUNKS = (
    "let mut text_parts = Parts(&mut text[..TEXT_BYTES]);\n"
    "        let chunk_bytes = text_parts.bytes(6 * size_of::<IndexChunk>(), 16)?;\n"
    "        let chunks = Parts(chunk_bytes).array::<IndexChunk>(6)?;"
)
SYNC_SEVEN_CHUNKS = (
    "let mut text_parts = Parts(text);\n"
    "        let chunk_bytes = text_parts.bytes(7 * size_of::<IndexChunk>(), 16)?;\n"
    "        let chunks = Parts(chunk_bytes).array::<IndexChunk>(7)?;"
)

# (what breaks, file under rust/, exact text, broken text, failing case name)
CONTROLS = (
    ("the 1,024-entry cap is raised", ADMISSION, "d.entries > 1024", "d.entries > 2048", "zip-entries-over"),
    ("the 1 MiB central-directory cap is raised", ADMISSION, "d.size > 1024 * 1024", "d.size > 2 * 1024 * 1024", "central-dir-over"),
    ("the 4 MiB member cap is raised", ZIP_RULES, "uncompressed_bytes > EPUB_MAX_MEMBER_BYTES", "false", "member-size-over"),
    ("the 64 MiB expanded-total cap is removed", ADMISSION, "if expanded > EPUB_MAX_EXPANDED_BYTES {", "if false {", "expanded-total-over"),
    ("the 511 B member-name cap is removed", ZIP_RULES, "usize::from(name_bytes) > EPUB_MAX_MEMBER_NAME_BYTES", "false", "name-len-512"),
    ("ZIP64 EOCD sentinels are not refused", ZIP_CONTAINER, "d.entries == u16::MAX || d.size == u32::MAX || d.offset == u32::MAX", "false", "zip64-sentinel"),
    ("the 192-anchor policy cap is removed", PAGE_CACHE, "if self.anchors < self.profile.max_anchors() {", "if true {", "chapter-anchors-over"),
    ("the source renderer regains the cache page cap", "phased-sink/src/lib.rs",
     "let resolving = self.mode == Target::TocResolve;",
     "if page.index >= 4096 { return false; }\n        let resolving = self.mode == Target::TocResolve;", "chapter-pages-over-4096"),
    ("the cache page cap blocks the source fallback", "phased-reader/src/phase_session.rs",
     "if request.cache_only {", "if request.cache_only || request.target >= 4096 {", "chapter-pages-over-4096"),
    ("the 4,096-page policy cap is removed", PAGE_CACHE, "if self.count >= self.profile.max_pages() {", "if false {", "chapter-pages-over-4096"),
    ("the 6-chunk sync writer index cap is raised", PHASE_STORAGE, SYNC_SIX_CHUNKS, SYNC_SEVEN_CHUNKS, "chapter-pages-over-768"),
    ("mapping denial loses generic Contents", READER,
     "let optional = optional.and_then(|bound| bound.then_some(()).ok_or(Status::OutOfMemory));",
     "let optional = optional.map(|_| ());", "toc-mapping-over"),
    ("Contents fallback loses its capacity notice", READER,
     "navigation.set_contents_fallback(contents_fallback(authored, optional));", "", "toc-nav-over-arena"),
    ("malformed Contents loses its explanation", READER,
     "else { ContentsFallback::Invalid }", "else { ContentsFallback::None }", "toc-malformed"),
    ("generic Contents fallback returns invalid/empty menu", READER, "Err(status) => (project(arena, &[]), status),", "Err(status) => (false, status),", "toc-nav-over-arena"),
    ("the 64 MiB source-file cap is removed", PHASED, "if bytes > admission::EXTERNAL_MAX_BYTES {", "if false {", "file-size-over"),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def compile_native_objects(run, workspace, out_dir):
    """Compile pinned FreeInk Expat + libunibreak objects for host linking."""
    sdk = ROOT / "vendor/freeink-sdk"
    book = sdk / "libs/book/FreeInkBook"
    includes = [
        arg
        for p in (
            book / "include",
            sdk / "libs/font/FreeInkFont/include",
            book / "third_party/expat",
            book / "third_party/miniz",
            book / "third_party/libunibreak",
        )
        for arg in ("-I", str(p))
    ]
    flags = ["-O2", "-DFREEINK_BOOK_SMALL=1"]
    objects = []
    for name in ("expat_xmlparse", "expat_xmltok", "expat_xmlrole", "unibreak_impl", "bridge"):
        src = workspace / "target-expat/native/expat_bridge.c" if name == "bridge" else book / "src/vendor" / f"{name}.c"
        obj = out_dir / f"{name}.o"
        run.run([run.tools["clang++"], "--no-default-config", "-x", "c", "-std=c99", *flags, *includes, "-c", str(src), "-o", str(obj)], ROOT)
        objects.append(obj)
    return objects


def test_binary(run, workspace, target):
    wire = run.cargo(
        workspace,
        target,
        "test",
        "-p",
        "newt-phased-reader",
        "--test",
        "large_epub_corpus",
        "--no-run",
        "--message-format=json",
    )
    outputs = [
        row["executable"]
        for row in map(json.loads, wire.splitlines())
        if row.get("reason") == "compiler-artifact"
        and row.get("executable")
        and row["target"]["name"] == "large_epub_corpus"
    ]
    if len(outputs) != 1:
        raise RuntimeError("one compiled large_epub_corpus test executable required")
    return outputs[0]


def _opt_bool(value):
    return "-" if value is None else ("1" if value else "0")


def _opt_int(value):
    return "-" if value is None else str(value)


def write_manifest(path: Path, records: list[dict]) -> None:
    lines = []
    for r in records:
        pr = r.get("expected_pages_range")
        lines.append("\t".join((
            r["name"], r["path"], r["expected_admit"], r["expected_open"], r["check_mode"],
            _opt_bool(r.get("expected_menu_valid")),
            _opt_bool(r.get("expected_authored")),
            _opt_int(r.get("expected_menu_count")),
            _opt_int(r.get("expected_anchors")),
            _opt_bool(r.get("expected_sync6")),
            _opt_bool(r.get("expected_small32")),
            "-" if pr is None else f"{pr[0]}..{pr[1]}",
            r.get("expected_fallback") or "-",
        )))
    path.write_text("\n".join(lines) + "\n")


def check_determinism(records: list[dict]) -> None:
    import hashlib
    for row in records:
        case = corpus.CASES_BY_NAME[row["name"]]
        if case.subset != "fast":
            continue
        digest = hashlib.sha256(corpus.build_epub(case.spec)).hexdigest()
        if digest != row["sha256"]:
            raise AssertionError(f"non-deterministic EPUB output for {case.name}")


def rustflags(objects: list[Path], *, warnings: bool) -> str:
    flags = (["-Dwarnings"] if warnings else []) + ["-Ccodegen-units=1"]
    for obj in objects:
        flags += ["-C", f"link-arg={obj}"]
    return shlex.join(flags)


def run_clean_cases(binary, run, workspace, support, manifest):
    run.env["NEWT_LARGE_EPUB_MANIFEST"] = str(manifest)
    out = run.run([binary, "--nocapture"], workspace)
    support.require("test result: ok. 1 passed; 0 failed; 0 ignored" in out, "corpus runner failed")
    return out


def prepare_control_baselines(binary, run, workspace, epubs_dir, output, support, by_name, verified, controls, *, tag="baseline"):
    """Ensure every control target case passes on unchanged code before mutating."""
    needed = []
    for row in controls:
        name = row[4]
        if name not in verified and name not in needed:
            needed.append(name)
    if not needed:
        return
    to_generate = [corpus.CASES_BY_NAME[name] for name in needed if name not in by_name]
    if to_generate:
        for rec in corpus.write_corpus(epubs_dir, to_generate):
            by_name[rec["name"]] = rec
    baseline_manifest = output / f"ctrl-{tag}.tsv"
    write_manifest(baseline_manifest, [by_name[name] for name in needed])
    run_clean_cases(binary, run, workspace, support, baseline_manifest)
    verified.update(needed)


def verify_control_baseline_guard(binary, run, workspace, epubs_dir, output, support, by_name):
    """Self-test: a broken control expectation must fail on unchanged code before any mutation."""
    bad_name = "chapter-pages-over-768"
    bad_by_name = {bad_name: {**by_name[bad_name], "expected_pages_range": (1, 1)}}
    bad_verified = set()
    number = len(run.commands)
    try:
        prepare_control_baselines(
            binary, run, workspace, epubs_dir, output, support,
            bad_by_name, bad_verified, [("self-test", "", "", "", bad_name)],
            tag="baseline-selftest",
        )
    except RuntimeError:
        stdout = (run.output / f"{number:03d}.stdout").read_text()
        stderr = (run.output / f"{number:03d}.stderr").read_text()
        combined = stdout + "\n" + stderr
        support.require(
            "test result: FAILED." in stdout and f"{bad_name}:" in combined and bad_name not in bad_verified,
            f"baseline self-test must fail cleanly on {bad_name}",
        )
    else:
        raise RuntimeError(f"baseline self-test passed with broken expectation on {bad_name}")
    print(f"PASS self-test: clean baseline rejects broken expectation on {bad_name}", flush=True)


def run_control(run, workspace, target, support, manifest, verified, name, source, good, bad, failing_case):
    support.require(failing_case in verified, f'control "{name}": {failing_case} has no clean baseline')
    path = workspace / source
    text = path.read_text()
    support.require(text.count(good) == 1, f'control "{name}": anchor not in rust/{source} exactly once')
    path.write_text(text.replace(good, bad))
    try:
        binary = test_binary(run, workspace, target)
        number = len(run.commands)
        run.env["NEWT_LARGE_EPUB_MANIFEST"] = str(manifest)
        try:
            run.run([binary, "--nocapture"], workspace)
        except RuntimeError:
            stdout = (run.output / f"{number:03d}.stdout").read_text()
            stderr = (run.output / f"{number:03d}.stderr").read_text()
            combined = stdout + "\n" + stderr
            support.require(
                "test result: FAILED." in stdout and f"{failing_case}:" in combined,
                f'control "{name}": must fail on {failing_case}',
            )
        else:
            raise RuntimeError(f'control "{name}": test passed with limit broken')
    finally:
        path.write_text(text)
    print(f"PASS control: fails on {failing_case} when {name}", flush=True)


def _probe(binary, run, workspace, out_dir, name, spec, mode="menu"):
    out_dir.mkdir(parents=True, exist_ok=True)
    p = out_dir / f"{name}.epub"
    p.write_bytes(corpus.build_epub(spec))
    manifest = out_dir / f"{name}.tsv"
    write_manifest(manifest, [{"name": name, "path": str(p), "expected_admit": "*", "expected_open": "*", "check_mode": mode}])
    run.env["NEWT_LARGE_EPUB_MANIFEST"] = str(manifest)
    out = run.run([binary, "--nocapture"], workspace)
    for line in out.splitlines():
        if line.startswith("CASE "):
            return dict(tok.split("=", 1) for tok in line.split()[2:])
    raise RuntimeError(f"missing CASE output for {name}")


def _bsearch_max(lo, hi, predicate):
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if predicate(mid):
            lo = mid
        else:
            hi = mid - 1
    return lo


def _is_authored(row):
    return row["menu_valid"] == "1" and row["authored"] == "1"


def run_sweep(binary, run, workspace, out_dir):
    """Binary-search exact thresholds where 48 KiB PhaseStorage banks bind."""
    print("\n=== SWEEP: exact 48 KiB PhaseStorage limits vs 1,024 ZIP entries ===", flush=True)
    max_ch_open = _bsearch_max(1, 1020, lambda n: _probe(
        binary, run, workspace, out_dir, f"ch-open-{n}", corpus.BookSpec(chapters=n), "admit")["open"] == "Ok")
    at_ch = _probe(binary, run, workspace, out_dir, f"ch-open-at-{max_ch_open}", corpus.BookSpec(chapters=max_ch_open), "menu")
    over_ch = _probe(binary, run, workspace, out_dir, f"ch-open-over-{max_ch_open + 1}", corpus.BookSpec(chapters=max_ch_open + 1), "admit")
    print(f"standard chapters (OEBPS/c0000.xhtml + nav): max open_phased Ok = {max_ch_open} (at: {at_ch}; +1: {over_ch})", flush=True)

    max_ch_authored = _bsearch_max(1, max_ch_open, lambda n: _is_authored(
        _probe(binary, run, workspace, out_dir, f"ch-toc-{n}", corpus.BookSpec(chapters=n), "menu")))
    print(f"standard chapters: max authored TOC = {max_ch_authored} (open_phased max = {max_ch_open})", flush=True)

    max_manifest = _bsearch_max(1, 1018, lambda n: _probe(
        binary, run, workspace, out_dir, f"mf-{n}", corpus.BookSpec(chapters=2, extra_manifest_items=n, toc_entries=2), "admit")["open"] == "Ok")
    at_mf = _probe(binary, run, workspace, out_dir, f"mf-at-{max_manifest}", corpus.BookSpec(chapters=2, extra_manifest_items=max_manifest, toc_entries=2), "admit")
    over_mf = _probe(binary, run, workspace, out_dir, f"mf-over-{max_manifest + 1}", corpus.BookSpec(chapters=2, extra_manifest_items=max_manifest + 1, toc_entries=2), "admit")
    print(f"extra manifest items (spine=2, isolating shared 48 KiB arena): max = {max_manifest} (total entries={max_manifest + 6}; at: {at_mf}; +1: {over_mf})", flush=True)

    max_toc = _bsearch_max(1, 1024, lambda n: _is_authored(
        _probe(binary, run, workspace, out_dir, f"toc-{n}", corpus.BookSpec(chapters=2, toc_entries=n), "menu")))
    at_toc = _probe(binary, run, workspace, out_dir, f"toc-at-{max_toc}", corpus.BookSpec(chapters=2, toc_entries=max_toc), "menu")
    over_toc = _probe(binary, run, workspace, out_dir, f"toc-over-{max_toc + 1}", corpus.BookSpec(chapters=2, toc_entries=max_toc + 1), "menu")
    print(f"nav TOC entries (chapters=2): max authored TOC = {max_toc} (at: {at_toc}; +1: {over_toc})", flush=True)

    max_extra_zip = _bsearch_max(1, 1018, lambda n: _probe(
        binary, run, workspace, out_dir, f"ez-{n}", corpus.BookSpec(chapters=2, extra_entries=n, toc_entries=2), "admit")["open"] == "Ok")
    print(f"extra ZIP entries (e/0000, chapters=2): max extra = {max_extra_zip} (total entries={max_extra_zip + 6})", flush=True)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full", action="store_true", help="Run the full corpus (includes 64 MiB and >4,096-page cases)")
    parser.add_argument("--measure", action="store_true", help="Print per-case timings, arena high-water, and cache sizes")
    parser.add_argument("--sweep", action="store_true", help="Also run chapter/entry count sweep against 48 KiB banks")
    parser.add_argument("--no-controls", action="store_true", help="Skip negative controls")
    args = parser.parse_args(argv)

    started = time.perf_counter()
    subset = "full" if args.full else "fast"
    cases = corpus.select_cases(subset=subset)

    support = load("large_epub_support", ROOT / "rust/tests/inflate/run_support.py")
    support.install_signal_handlers()
    scratch = ROOT / ".build/large-epub-corpus"
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="run-", dir=scratch) as directory:
        output = Path(directory)
        workspace = output / "rust"
        shutil.copytree(ROOT / "rust", workspace, ignore=shutil.ignore_patterns("target", "__pycache__"))
        (output / "logs").mkdir()
        (output / "objs").mkdir()
        epubs_dir = output / "epubs"
        gen_t0 = time.perf_counter()
        records = corpus.write_corpus(epubs_dir, cases)
        check_determinism(records)
        gen_s = time.perf_counter() - gen_t0
        manifest = output / "manifest.tsv"
        write_manifest(manifest, records)

        run = support.Run(output / "logs", support.verify_tools(), time.monotonic() + 600)
        run.env["CARGO_BUILD_JOBS"] = os.environ.get("CARGO_BUILD_JOBS", "6")
        objects = compile_native_objects(run, workspace, output / "objs")
        run.env["RUSTFLAGS"] = rustflags(objects, warnings=True)
        run.env["NEWT_LARGE_EPUB_MANIFEST"] = str(manifest)
        target = output / "target"
        try:
            binary = test_binary(run, workspace, target)
            exec_t0 = time.perf_counter()
            out = run_clean_cases(binary, run, workspace, support, manifest)
            exec_s = time.perf_counter() - exec_t0
            if args.measure:
                for line in out.splitlines():
                    if line.startswith(("SIZES ", "CASE ")):
                        print(line)
            if args.sweep:
                run_sweep(binary, run, workspace, output / "sweep")
            if not args.no_controls and negative_controls(__file__):
                by_name = {r["name"]: r for r in records}
                verified = set(by_name)
                prepare_control_baselines(binary, run, workspace, epubs_dir, output, support, by_name, verified, CONTROLS)
                verify_control_baseline_guard(binary, run, workspace, epubs_dir, output, support, by_name)
                run.env["RUSTFLAGS"] = rustflags(objects, warnings=False)
                for row in CONTROLS:
                    case_name = row[4]
                    case_manifest = output / f"ctrl-{case_name}.tsv"
                    write_manifest(case_manifest, [by_name[case_name]])
                    run_control(run, workspace, target, support, case_manifest, verified, *row)
        except RuntimeError:
            for log in sorted((output / "logs").glob("*.std*"))[-2:]:
                print(log.read_text(), file=sys.stderr, end="")
            raise
    total_s = time.perf_counter() - started
    print(f"PASS large-EPUB corpus ({subset}, {len(cases)} cases; gen {gen_s:.2f}s, run {exec_s:.2f}s, total {total_s:.2f}s)", flush=True)


if __name__ == "__main__":
    main()
