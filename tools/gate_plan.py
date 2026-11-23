"""The complete host gate's phases, in their serial order.

Order is the serial gate's order exactly; tools/gate_runner.py may overlap
phases only where nothing below orders them. A phase names, in `after`, each
phase whose output it reads, and in `uses`, each fixed path it shares with
another phase: phases sharing a path never overlap and keep this order.
"""
from pathlib import Path
import sys

from gate_runner import Phase, label
from host_common import ROOT

from rust_gate_families import (ADAPTER_FAMILIES, FOUNDATION_CAMPAIGNS, FULL_POSITIVE_SOURCES,
                           owner as rust_owner, positive_owner as rust_positive_owner)

PYTHON = sys.executable

# Arguments that only add a test's negative controls (tools/gate_tier.py).
CONTROL_FLAGS = frozenset(("--negative-controls", "--negative-control", "--baseline-controls",
                           "--uninstrumented-control"))

# The one native unit binary: the SD bus header the Rust image's board shim links.
UNIT_TESTS = (("sd_shared_bus_test.cpp", "sd-shared-bus-tests"),)

# The quick development pre-gate (tools/check.py --suite quick): tooling and
# policy tests, build boundaries, generator checks, the native unit binary and
# the preview contracts, with their prerequisites, in the complete gate's order.
# The Rust families run only in the complete gate.
QUICK = ("bench_flash_test.py --negative-controls", "bench_flash_layouts_test.py --negative-controls", "bench_handshake_test.py", "x3_slots_test.py", "installer_test.py --negative-controls", "clean_export_test.py", "history_fixtures_test.py", "repo_files_test.py", "crt_bundle_test.py", "update_publish_test.py", "release_publish_test.py --negative-controls", "nightly_test.py --negative-controls", "release_tag_test.py", "nvs_namespaces_test.py", "legacy_nvs_test.py", "gate_scratch_test.py", "native_fixture_roots_test.py", "host_scratch_inputs_test.py", "trusted_host_test.py", "source_fixture_test.py", "code_health_test.py",
         "code_health_resources_test.py", "code_health_capacity_test.py", "code_health_lent_test.py", "code_health_procedure_cache_test.py",
         "python_runtime_test.py", "reading_acceptance_test.py", "reading_latency_test.py",
         "reading_latency_ready_test.py", "host_objects_test.py", "host_object_reuse_test.py",
         "page_entry_fatfs_test.py", "software_gate_shadow_test.py", "ci_toolchain_test.py",
         "ci_workflow_test.py", "public_ci_test.py", "docs_lane_test.py", "ci_persist_test.py", "gate_runner_test.py", "gate_tier_test.py", "gate_performance_test.py", "host_wide_jobs_test.py", "gate_stop_test.py", "procedure_closure_test.py",
         "code_health.py", "native_sleep_guard_test.py",
         "native_reader_stack_test.py", "native_rollback_hook_test.py", "native_timer_task_test.py", "native_flash_writers_test.py", "native_crash_output_test.py", "wifi_link_test.py", "next_boot_test.py", "heap_clear_test.py", "build_scope_test.py", "rust_app_build_test.py",
         "freeink_patch_destination_test.py", "toolchain_share_test.py",
         "native_output_test.py", "make-reader-fixture.py --check", "make-builtin-app.py --check", "newt_package_test.py", "newt_delivery_test.py",
         "library_preview_test.py", "system_ui_contract_test.py", "system_ui_rule_test.py",
         *(name for _, name in UNIT_TESTS), "gate_previews.py")


def plan(profile: str, fresh_objects: bool, controls=None) -> list[Phase]:
    """controls(script) says whether that test's negative controls run; None
    runs every one (the full tier). Without them a test drops its control
    flags, and a full_only phase does not run: one that only adds evidence
    about its test, its negative controls, an exhaustive matrix or an explicitly
    full-tier positive suite."""
    phases, names = [], {}  # names: each complete-plan name to its name here, or None

    def run(*arguments, bytecode=False, after=(), uses=(), alone=False, full_only=False, control_owner=None):
        arguments = tuple(map(str, arguments))
        complete = label((PYTHON, *(() if bytecode else ("-B",)), *arguments))
        script = control_owner or next((Path(argument).name for argument in arguments if argument.endswith(".py")), "")
        if controls is not None and not controls(script):
            if full_only:
                names[complete] = None
                return
            arguments = tuple(argument for argument in arguments if argument not in CONTROL_FLAGS)
        command = (PYTHON, *(() if bytecode else ("-B",)), *arguments)
        names[complete] = label(command)
        after = tuple(names.get(name, name) for name in after if names.get(name, name) is not None)
        phases.append(Phase(names[complete], command, after=after, uses=tuple(uses), alone=alone))

    def test(name, *arguments, **options):
        run(ROOT / "tests" / name, *arguments, **options)

    def unit(source, name, *arguments, after=(), uses=()):
        binary = ROOT / ".build" / name
        phases.append(Phase(f"compile {name}", (PYTHON, "-B", str(ROOT / "tools/host_compile.py"),
                                                str(ROOT / "tests" / source), name),
                            uses=(binary, *uses)))
        phases.append(Phase(label([binary]), (str(binary), *map(str, arguments)),
                            after=(f"compile {name}", *after), uses=(binary, *uses)))

    # The Rust checks are the longest chains; the runner starts them first by
    # their recorded seconds (.build/gate-phase-seconds.json).
    test("bench_flash_test.py", "--negative-controls")
    test("bench_flash_layouts_test.py", "--negative-controls")
    test("bench_handshake_test.py")
    test("x3_slots_test.py")
    test("installer_test.py", "--negative-controls")
    test("slot_vectors_test.py")
    test("app_store_test.py")
    test("update_publish_test.py")
    test("release_publish_test.py", "--negative-controls")
    test("nightly_test.py", "--negative-controls")
    test("release_tag_test.py")
    test("clean_export_test.py")
    test("history_fixtures_test.py")
    test("repo_files_test.py")
    test("crt_bundle_test.py")
    test("nvs_namespaces_test.py")
    test("gate_scratch_test.py")
    test("trusted_host_test.py")
    test("source_fixture_test.py")
    for script in ("code_health_test.py", "code_health_resources_test.py", "code_health_capacity_test.py", "code_health_lent_test.py",
                   "code_health_procedure_cache_test.py", "python_runtime_test.py", "reading_acceptance_test.py",
                   "reading_latency_test.py", "reading_latency_ready_test.py", "host_objects_test.py", "host_object_reuse_test.py",
                   "software_gate_shadow_test.py", "ci_toolchain_test.py", "ci_workflow_test.py", "public_ci_test.py", "docs_lane_test.py", "ci_persist_test.py", "native_fixture_roots_test.py", "host_scratch_inputs_test.py",
                   "gate_runner_test.py", "gate_tier_test.py", "gate_performance_test.py", "host_wide_jobs_test.py", "gate_stop_test.py", "procedure_closure_test.py"):
        test(script)
    run(ROOT / "tools/code_health.py")
    test("native_sleep_guard_test.py")
    test("native_reader_stack_test.py")
    test("native_rollback_hook_test.py")
    test("native_timer_task_test.py")
    test("native_flash_writers_test.py")
    test("native_crash_output_test.py")
    test("wifi_link_test.py")
    test("next_boot_test.py")
    test("heap_clear_test.py")
    test("host_sha256_test.py")
    test("page_entry_fatfs_test.py", uses=(ROOT / ".build/page-entry-fatfs",))
    test("dev_usb_serial_test.py", bytecode=True)
    test("build_scope_test.py", bytecode=True)
    test("rust_app_build_test.py")
    run(ROOT / "rust-health/rust_health.py")
    run(ROOT / "rust-health/rust_health_test.py")
    run(ROOT / "rust/tests/family_dispatch_test.py")
    # Rust checks run in parallel, one gate job each: each owns its receipt and
    # temporary folder (rust/check.py, #345) and averages about one CPU (two
    # cargo jobs at most). Landing and the target probes share a Cargo target.
    shared_target = (ROOT / ".build/rust/target",)
    test("legacy_nvs_test.py")
    run(ROOT / "rust/check.py", "--landing", uses=shared_target)
    # Landing's contract and source-privacy test, split out to fit the CPU budget (#337).
    run(ROOT / "rust/tests/landing_test.py")
    for campaign in FOUNDATION_CAMPAIGNS:
        run(ROOT / "rust/check.py", f"--foundation-controls={campaign}",
            full_only=True, control_owner=rust_owner('foundation'))
    for family in ADAPTER_FAMILIES:
        selected = controls is None or controls(rust_owner(family))
        if family in FULL_POSITIVE_SOURCES and not selected and not controls(rust_positive_owner(family)):
            continue
        run(ROOT / "rust/check.py", f"--family={family}", "--tier=fast",
            f"--control-family={family if selected else 'none'}")
    run(ROOT / "rust/check.py", "--target-probes", uses=shared_target)
    test("freeink_patch_destination_test.py")
    test("toolchain_share_test.py")
    test("native_output_test.py")
    for tool in ("make-reader-fixture.py", "make-builtin-app.py"):
        run(ROOT / "tools" / tool, "--check", bytecode=True)
    for script in ("newt_package_test.py", "newt_delivery_test.py", "home_refresh_regional_test.py"):
        test(script, bytecode=True)
    # The real firmware built for the host over C fakes (tools/rust_preview.py).
    preview = ROOT / ".build/newt-preview"
    phases.append(Phase("compile newt-preview", (PYTHON, "-B", str(ROOT / "tools/rust_preview.py")),
                        uses=(preview,)))
    test("library_preview_test.py", preview, bytecode=True, after=("compile newt-preview",))
    test("library_cover_prepare_test.py", *(("--portable-only",) if profile == "linux" else ()))
    test("system_ui_contract_test.py", preview, after=("compile newt-preview",))
    test("system_ui_rule_test.py", preview, after=("compile newt-preview",))
    test("preview_owners_test.py", preview, after=("compile newt-preview",))
    test("preview_update_test.py", preview, after=("compile newt-preview",))
    test("preview_reader_wake_test.py", preview, after=("compile newt-preview",))
    for source, name in UNIT_TESTS:
        unit(source, name)
    run(ROOT / "tools/gate_previews.py", preview, after=("compile newt-preview",))
    return phases


def quick(profile: str, fresh_objects: bool) -> list[Phase]:
    """The complete plan's QUICK phases and everything they run after."""
    phases = plan(profile, fresh_objects)
    by_name = {phase.name: phase for phase in phases}
    wanted = list(QUICK)
    missing = sorted(set(wanted) - set(by_name))
    if missing:
        raise ValueError(f"Quick phases missing from the complete gate: {missing}")
    chosen, pending = set(), wanted
    while pending:
        name = pending.pop()
        if name not in chosen:
            chosen.add(name)
            pending.extend(by_name[name].after)
    return [phase for phase in phases if phase.name in chosen]
