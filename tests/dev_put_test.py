#!/usr/bin/env python3
"""The development bridge's card write (#26).

rust/tests/dev_put/lease.rs drives newt_core::dev_put over a fake card that
stores every write at once and a scripted host. It pins the name and folder
rules, admission, and that the final name holds only the old file or the whole
verified new one, whatever ends the lease, while a marker a power loss left
refuses the folder and recovery deletes nothing that may share data. This runs it in a scratch copy of rust/. Each
negative control then breaks one rule there and expects the named test to
fail, never to fail to compile.
"""
import importlib.util
import json
import re
from pathlib import Path
import shutil
import sys
import tempfile
import time

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
from gate_tier import negative_controls

BINARY = 'dev_put'
TESTS = 49
# Recovery's one rule: a marker the next FILE finds refuses.
MARKED = '        if self.recovery_file(MARKER, "marker")? { return Err(Refused::Recovery("commit", 0)); }\n'
# (what breaks, its file under rust/core/src, its text there, the broken text, the test that must fail)
CONTROLS = (
    ('bulk read ignores over-length return', 'dev_put/lease.rs',
     '            if got > remaining { return Err(Refused::Abandoned("line")); }\n',
     '            let _ = remaining;\n',
     'overlength_bulk_read_cancels_without_writing'),
    ('fast path polls micros when bytes are ready', '../../salamander/board/dev_usb.rs',
     '        let mut started: Option<u32> = None;\n',
     '        let mut started: Option<u32> = Some(micros());\n',
     'receive_owner::packet_gaps_are_polled_but_a_stalled_sender_returns_promptly'),
    ('stale queue_empty skips queued bytes on new lease', '../../salamander/board/dev_usb.rs',
     'PutReceive { previous, queue_empty: 0, _owner: core::marker::PhantomData }',
     'PutReceive { previous, queue_empty: 1, _owner: core::marker::PhantomData }',
     'receive_owner::queued_bytes_precede_fifo_and_reset_across_leases'),
    ('a partial reply swallows the undelimited exit row', '../../firmware/app_runtime/card_write_observation.rs',
     'writeln!(output, "\\nPUT_STATS', 'writeln!(output, "PUT_STATS',
     'wire_deadlines::a_partial_timeout_reply_cannot_swallow_the_exit_row'),
    ('packet gaps immediately sleep', '../../salamander/board/dev_usb.rs',
     'const QUIET_US: u32 = 250;', 'const QUIET_US: u32 = 0;',
     'receive_owner::packet_gaps_are_polled_but_a_stalled_sender_returns_promptly'),
    ('empty packet polling exceeds its bound', '../../salamander/board/dev_usb.rs',
     'const QUIET_US: u32 = 250;', 'const QUIET_US: u32 = 1_000;',
     'receive_owner::packet_gaps_are_polled_but_a_stalled_sender_returns_promptly'),
    ('a transient disconnected observation silently drops the exit row', '../../firmware/app_runtime/card_write_observation.rs',
     'if !wire.connected() { wire.pause(); continue; }', 'if !wire.connected() { break; }',
     'wire_deadlines::stalled_sender_exits_and_its_diagnostic_output_is_bounded'),
    ('a stalled sender waits past its receive deadline', 'dev_put/lease.rs',
     'wrapping_sub(activity) >= IDLE_MS', 'wrapping_sub(activity) >= IDLE_MS * 2',
     'wire_deadlines::stalled_sender_exits_and_its_diagnostic_output_is_bounded'),
    ('exit diagnostics wait past their output deadline', '../../firmware/app_runtime/card_write_observation.rs',
     'wire.now().wrapping_sub(start) < 1_000', 'wire.now().wrapping_sub(start) < 2_000',
     'wire_deadlines::stalled_sender_exits_and_its_diagnostic_output_is_bounded'),
    ('DATA write observation omitted', 'dev_put/lease.rs', '        self.wire.observe(Observation::Writing);\n', '',
     'twenty_mib_is_committed_only_after_full_readback'),
    ('timeout loses its typed cause', 'dev_put/lease.rs', '"timeout" => ExitReason::Timeout,', '"timeout" => ExitReason::Syntax,',
     'a_late_interrupted_large_transfer_keeps_the_old_file_and_next_lease_works'),
    ('first exit overwritten by reply failure', '../../firmware/app_runtime/card_write_observation.rs',
     'self.reason.get_or_insert(reason);', 'self.reason = Some(reason);',
     'lease_counters_separate_receive_write_ack_and_latch_the_first_exit'),
    ('the SHA-256 is not compared', 'dev_put/lease.rs', "        if digest != sha256 { return Err(Refused::DigestMismatch); }\n", "",
     'a_digest_mismatch_leaves_no_final_file'),
    ('a `..` name is allowed', 'dev_put/path.rs', "pub fn name_allowed(name: &[u8]) -> bool {\n",
     "pub fn name_allowed(name: &[u8]) -> bool {\n    if name == b\"..\" { return true; }\n",
     'names_outside_the_safe_set_are_refused'),
    ('an absolute path passes validation', 'dev_put/path.rs',
     "relative.split(|&b| b == b'/').all(name_allowed)",
     "relative.split(|&b| b == b'/').filter(|c| !c.is_empty()).all(name_allowed)",
     'names_outside_the_safe_set_are_refused'),
    ('a parent of another kind is accepted', 'dev_put/lease.rs',
     'Entry::File | Entry::Other => Err(Refused::Target)', 'Entry::File | Entry::Other => Ok(())',
     'paths_cannot_follow_non_directory_parents'),
    ('raw reception ignores takeover and idle timeout', 'dev_put/lease.rs',
     '            if let Some(reason) = self.abandoned(activity) { return Err(Refused::Abandoned(reason)); }\n',
     '            if let Some(reason) = self.abandoned(activity) { if reason != "physical-input" { return Err(Refused::Abandoned(reason)); } }\n',
     'an_interrupted_raw_block_never_replaces_the_final_file'),
    ('the receive token forgets restoration', '../../salamander/board/dev_usb.rs',
     'unsafe { nsc_put_rx_end(self.previous) }', 'let _ = self.previous;',
     'receive_owner::every_lease_exit_restores_rx'),
    ('raw progress renews the deadline', 'dev_put/lease.rs',
     '        let activity = self.wire.now();\n        while received < decoded {\n',
     '        while received < decoded {\n            let activity = self.wire.now();\n',
     'a_raw_block_has_a_nonrenewing_deadline'),
    ('an open book does not refuse', 'dev_put.rs', "    if facts.reader_open { return Err(Busy::Reading); }\n", "",
     'admission_refuses_reading_opening_and_away_from_home'),
    ('bytes go straight to the final name', 'dev_put/lease.rs', "let fd = self.card.create(TEMP)",
     "let fd = self.card.create(path(&target))", 'physical_input_abandons_and_keeps_the_old_file'),
    ('recovery deletes a file or clears the marker while a marker stands', 'dev_put/commit.rs', MARKED,
     "        if self.recovery_file(MARKER, \"marker\")? { let _ = self.card.remove(OLD); "
     "let _ = self.card.remove(TEMP); let _ = self.card.remove(MARKER); return Ok(()); }\n",
     'commit::a_rename_a_power_loss_cut_short_is_never_unlinked'),
    ('a marker clears once no backup or temp shows (the target alone, or nothing)', 'dev_put/commit.rs', MARKED,
     "        if self.recovery_file(MARKER, \"marker\")? {\n"
     "            if !old && self.card.entry(TEMP) == Ok(Entry::Missing) { return self.card.remove(MARKER)"
     ".map_err(|code| Refused::Recovery(\"marker\", code)); }\n"
     "            return Err(Refused::Recovery(\"commit\", 0));\n        }\n",
     'commit::a_marker_left_by_any_commit_refuses_whatever_the_names_show'),
    ('a rename that fails after moving reads as a plain refusal', 'dev_put/commit.rs',
     "        if !unchanged { return Refused::Recovery(step, code); }\n", "",
     'commit::a_rename_that_fails_after_moving_keeps_the_marker'),
    ('the read-back ignores a takeover', 'dev_put/lease.rs',
     "            if let Some(reason) = self.interruption() { return Err(Refused::Abandoned(reason)); }\n", "",
     'commit::a_press_while_done_reads_back_stops_it_there'),
    ('the first rename ignores a takeover', 'dev_put/lease.rs',
     "        if let Some(reason) = self.interruption() {\n"
     "            // No rename ran, so the temp the exit removes is no other name's.\n"
     "            let _ = self.card.remove(MARKER);\n            return Err(Refused::Abandoned(reason));\n        }\n", "",
     'commit::a_press_just_before_the_first_rename_keeps_the_old_file'),
    ('an idle transfer waits past its bound', 'dev_put/lease.rs',
     'wrapping_sub(activity) >= IDLE_MS', 'wrapping_sub(activity) >= IDLE_MS * 2',
     'silence_times_out_and_removes_the_temp'),
    ('a failed restore reads as a plain refusal', 'dev_put/commit.rs',
     'is_err() { return Err(Refused::Recovery("rename", code)); }', 'is_err() { return Err(Refused::Card("rename", code)); }',
     'commit::a_failed_restore_keeps_the_backup_and_says_so'),
)


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def test_binary(run, workspace, target):
    """Compiles the test; a compiler error is a failure, never a control's kill."""
    wire = run.cargo(workspace, target, 'test', '-p', 'newt-core', '--test', BINARY, '--no-run',
                     '--message-format=json')
    outputs = [row['executable'] for row in map(json.loads, wire.splitlines())
               if row.get('reason') == 'compiler-artifact' and row.get('executable')
               and row['target']['name'] == BINARY]
    if len(outputs) != 1:
        raise RuntimeError(f'one compiled {BINARY} test executable required')
    return outputs[0]


def control(run, workspace, target, support, name, source, good, bad, test):
    path = workspace / 'core/src' / source
    text = path.read_text()
    support.require(text.count(good) == 1, f'control "{name}": its anchor is not in rust/core/src/{source} exactly once')
    path.write_text(text.replace(good, bad))
    try:
        binary = test_binary(run, workspace, target)
        number = len(run.commands)
        try:
            run.run([binary, test, '--exact'], workspace)
        except RuntimeError:
            stdout = (run.output / f'{number:03d}.stdout').read_text()
            support.require('test result: FAILED. 0 passed; 1 failed' in stdout and 'panicked at' in stdout,
                            f'control "{name}": {test} must fail on an assertion')
        else:
            raise RuntimeError(f'control "{name}": {test} passed with the rule broken')
    finally:
        path.write_text(text)
    print(f'PASS control: {test} fails when {name}', flush=True)


def native_receive(run, workspace, output, support):
    native = workspace / 'firmware/serial_console/native'
    fixture = workspace / 'tests/dev_put/native'
    source = native / 'serial_console.cpp'
    good = source.read_text()
    # Use the pinned HAL enum without its target-only MMIO/CPU headers. The
    # hosted toolchain installs this same SDK payload for every gate shard.
    hal = ROOT / '.toolchain/platformio/packages/framework-arduinoespressif32-libs/esp32c3/include/hal/esp32c3/include/hal/usb_serial_jtag_ll.h'
    text = hal.read_text()
    enums = re.findall(r'typedef enum \{[^}]+\} usb_serial_jtag_ll_intr_t;', text)
    support.require(len(enums) == 1, 'one pinned C3 USB interrupt enum required')
    (output / 'pinned-usb-irq.h').write_text(text[:text.index('#pragma once')] + '#pragma once\n' + enums[0] + '\n')
    def check(enabled):
        binary = output / f'native-rx-{enabled}'
        run.run([run.tools['clang++'], '--no-default-config', '-std=c++17',
                 f'-DNEWT_DEV_USB={enabled}', '-DARDUINO_USB_MODE=1', '-DARDUINO_USB_CDC_ON_BOOT=1', '-DCONFIG_IDF_TARGET_ESP32C3=1',
                 '-I' + str(output), '-I' + str(fixture), '-I' + str(native), source, fixture / 'receive.cpp',
                 '-fuse-ld=/usr/bin/x86_64-linux-gnu-ld.bfd', '-o', binary], workspace)
        return binary
    for enabled in (0, 1):
        run.run([check(enabled)], workspace)
    print('PASS native RX: ordinary SDK path, ordered drain, RX-only mask and pending-byte restoration', flush=True)
    if not negative_controls(__file__):
        return
    for name, before, after in [
        ('RX mask read outside critical section',
         '    portENTER_CRITICAL(&lock);\n    uint32_t previous = usb_serial_jtag_ll_get_intr_ena_status() & USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT;',
         '    uint32_t previous = usb_serial_jtag_ll_get_intr_ena_status() & USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT;\n    portENTER_CRITICAL(&lock);'),
        ('restore removed', 'if (previous) usb_serial_jtag_ll_ena_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT);', 'if (previous) { (void)previous; }'),
        ('queued bytes skipped', 'bool drained = queue_empty && *queue_empty;', 'bool drained = true;'),
        ('queue_empty set before queue drained', 'if (queued <= 0) {', 'if (true) {'),
        ('begin leaves RX unmasked',
         'usb_serial_jtag_ll_disable_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT);\n    portEXIT_CRITICAL(&lock);\n    return previous;',
         'portEXIT_CRITICAL(&lock);\n    return previous;'),
        ('bulk read ignores max_len bound',
         'const size_t rem = max_len - got;',
         'const size_t rem = 64;'),
        ('begin clears BUS_RESET',
         'usb_serial_jtag_ll_disable_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT);\n    portEXIT_CRITICAL(&lock);\n    return previous;',
         'usb_serial_jtag_ll_disable_intr_mask(USB_SERIAL_JTAG_INTR_SERIAL_OUT_RECV_PKT | USB_SERIAL_JTAG_INTR_BUS_RESET);\n    portEXIT_CRITICAL(&lock);\n    return previous;'),
        ('end clears BUS_RESET',
         '    portEXIT_CRITICAL(&lock);\n    // Do not clear RX raw status:',
         '    usb_serial_jtag_ll_disable_intr_mask(USB_SERIAL_JTAG_INTR_BUS_RESET);\n    portEXIT_CRITICAL(&lock);\n    // Do not clear RX raw status:'),
        ('ordinary reads use leased FIFO', 'int nsc_read_byte(void) { return nsc_port_read_byte(Serial); }',
         'int nsc_read_byte(void) { uint8_t b = 0; return nsc_put_rx_read(&b, 1, nullptr) == 1 ? b : -1; }'),
    ]:
        support.require(good.count(before) == 1, f'native control anchor: {name}')
        source.write_text(good.replace(before, after))
        try:
            binary = check(1)
            number = len(run.commands)
            try:
                run.run([binary], workspace)
            except RuntimeError:
                error = (run.output / f'{number:03d}.stderr').read_text()
                support.require('Assertion' in error or 'assertion' in error, f'{name}: must fail an assertion')
            else:
                raise RuntimeError(f'native RX control survived: {name}')
        finally:
            source.write_text(good)
        print(f'PASS native RX control: {name}', flush=True)


def main():
    support = load('dev_put_support', ROOT / 'rust/tests/inflate/run_support.py')
    support.install_signal_handlers()
    scratch = ROOT / '.build/dev-put'
    scratch.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='run-', dir=scratch) as directory:
        output = Path(directory)
        workspace = output / 'rust'
        shutil.copytree(ROOT / 'rust', workspace, ignore=shutil.ignore_patterns('target', '__pycache__'))
        (output / 'logs').mkdir()
        run = support.Run(output / 'logs', support.verify_tools(), time.monotonic() + 300)
        run.env['RUSTFLAGS'] = '-Dwarnings -Ccodegen-units=1'
        target = output / 'target'
        try:
            native_receive(run, workspace, output, support)
            result = run.run([test_binary(run, workspace, target)], workspace)
            support.require(f'test result: ok. {TESTS} passed; 0 failed; 0 ignored' in result,
                            f'all {TESTS} {BINARY} tests ran and passed')
            print('PASS the bridge card write keeps the final name old or whole and verified', flush=True)
            if negative_controls(__file__):
                # A broken rule can leave a value unused; only the test may kill it.
                run.env['RUSTFLAGS'] = '-Ccodegen-units=1'
                for row in CONTROLS:
                    control(run, workspace, target, support, *row)
        except RuntimeError:
            for log in sorted((output / 'logs').glob('*.std*'))[-2:]:
                print(log.read_text(), file=sys.stderr, end='')
            raise


if __name__ == '__main__':
    main()
