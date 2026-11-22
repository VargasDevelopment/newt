#!/usr/bin/env python3
"""Identity-locked X3 install, restore and slot selection, with backup and on-chip MD5 checks.

tools/x3_slots.py decides from the device which slot may be written: beside
another firmware, Newt's slot; without one, the slot that does not boot.
Every write goes to that slot or to one OTA selection sector. install-other
and restore-other write only the slot that does not boot, beside the running
Newt, and select nothing.
"""
import argparse
import contextlib
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import sys
import time

from bench_candidate import FLASH, Candidate, json_bytes, other_image, partition_layout, require, sha, write_span
from bench_device import Device
from bench_lease import lease_unit, load_identity, save
import x3_slots
from x3_slots import HEAD, OTADATA, OTADATA_SIZE, SLOTS, SLOT_SIZE
OTHER_RECORD = 'other.json'  # install-other's record of what it wrote, for restore-other

SESSION_SECONDS = 4 * 60 * 60


class Steps:
    def __init__(self, directory):
        self.directory, self.results = directory, []

    def __call__(self, name, action):
        started = time.monotonic()
        status = 'FAIL'
        try:
            with (self.directory / (name + '.log')).open('x') as log:
                with contextlib.redirect_stdout(log), contextlib.redirect_stderr(log):
                    result = action()
            status = 'PASS'
            return result
        finally:
            elapsed = time.monotonic() - started
            self.results.append({'step': name, 'status': status, 'seconds': round(elapsed, 3)})
            print(f'{status} {name}: {elapsed:.2f}s', flush=True)


def verify_md5(device, offset, data):
    require(device.md5(offset, len(data)) == hashlib.md5(data).hexdigest(),
            f'On-chip MD5 mismatch at {offset:#x}, {len(data):#x} bytes')


def host_boot_id():
    return Path('/proc/sys/kernel/random/boot_id').read_text().strip()


def read_layout(device, repair=False):
    """Fresh device reads of the selection records and both slot heads."""
    ota = device.read(OTADATA, OTADATA_SIZE)
    heads = [device.read(offset, HEAD) for offset in SLOTS]
    return ota, x3_slots.decide(ota, heads, repair)


def candidate_slot(device, layout, image):
    """boot-newt without another firmware: both slots may hold Newt, so select the
    one whose on-chip MD5 is the candidate's, the slot that boots first."""
    expected = hashlib.md5(image).hexdigest()
    for slot in (layout.boot, layout.newt):
        if layout.images[slot].kind == 'newt' and device.md5(SLOTS[slot], len(image)) == expected:
            return slot
    raise ValueError('Neither Newt slot holds this candidate; install it instead')


def backup_session(device, session, identity, offset, length, table, step, now=time.time):
    backup_path, receipt_path = session / 'backup.bin', session / 'session.json'
    boot = host_boot_id()
    if receipt_path.exists() and backup_path.exists():
        receipt = json_bytes(receipt_path.read_bytes())
        require(receipt['mac'] == identity['mac'] and receipt['host_boot'] == boot and
                0 <= now() - receipt['created'] <= SESSION_SECONDS,
                'Session identity/age differs; start a new session directory')
        backup = backup_path.read_bytes()
        require(len(backup) == FLASH and sha(backup) == receipt['backup_sha256'], 'Session backup corrupt')
        # Without another firmware each install moves the writable slot to the other one.
        require(receipt['newt'] == offset,
                'The writable slot differs from the session; start a new session directory')
        span = receipt['span']
        require(write_span(offset, span) == span and length <= span,
                'Image exceeds session erase span; start a new session directory')
        require(table is None or backup[0x8000:0x9000] == table, 'Session partition layout differs')
        return backup, span
    # Missing either half never authorizes a fast install. Keep old evidence.
    for path in (backup_path, receipt_path):
        if path.exists():
            path.rename(session / (path.name + '.orphan-' + str(time.time_ns())))
    backup = step('full-backup', lambda: device.read(0, FLASH))
    require(len(backup) == FLASH, 'Full backup is incomplete')
    with backup_path.open('xb') as stream:
        stream.write(backup)
        stream.flush()
        os.fsync(stream.fileno())
    step('confirm-backup-md5', lambda: verify_md5(device, 0, backup))
    require(table is None or backup[0x8000:0x9000] == table, 'Device partition table differs from candidate')
    partition_layout(backup[0x8000:0x9000])
    span = write_span(offset, length)
    save(receipt_path, {'mac': identity['mac'], 'host_boot': boot, 'created': now(),
                        'backup_sha256': sha(backup), 'newt': offset, 'span': span})
    return backup, span


def install(device, session, identity, candidate, step, unit, action='install', repair=False):
    """No automatic retry/reset after failure, especially after a write starts.

    install/restore write the candidate into the writable slot, then select
    it with a fresh new record, so its first boot returns to a verified
    image unless the candidate confirms itself (x3_slots.install_selection).
    boot-newt proves the installed candidate; boot-other proves the other
    firmware against the session backup. Both write only the safe selection sector,
    or nothing when the slot is already selected. `repair` rewrites only an
    unreadable slot beside the running Newt.
    """
    require(not (session / 'session.pending').exists(), 'Stale session.pending; inspect before backup')
    step('usb-mac-security-flash', device.connect)
    ota, layout = step('slot-identity', lambda: read_layout(device, repair))
    print('Slots: ' + json.dumps(layout.report(ota), sort_keys=True), flush=True)
    image = candidate.image
    require(x3_slots.classify(image[:HEAD]).kind == 'newt', 'Candidate is not a Newt image')
    boot_only = action in ('boot-newt', 'boot-other')
    target = layout.other if action == 'boot-other' else layout.newt
    require(target is not None, 'No slot holds another firmware; boot-other has nothing to select')
    if action == 'boot-newt' and layout.other is None:
        target = step('newt-slot-md5', lambda: candidate_slot(device, layout, image))
    elif action == 'boot-newt':
        require(layout.images[layout.newt].kind == 'newt', 'Newt slot does not hold a Newt image; install instead')
        step('newt-slot-md5', lambda: verify_md5(device, layout.newt_offset, image))
    elif not boot_only:
        x3_slots.require_newt_write(layout, layout.newt_offset, len(image))
    backup, span = backup_session(device, session, identity, layout.newt_offset, len(image), candidate.table, step)
    start, end = layout.newt_offset, layout.newt_offset + span
    payload = image + b'\xff' * (span - len(image))
    write_span(start, len(payload))
    fallback, selection = (None, x3_slots.selection_write(layout, ota, target)) if boot_only else \
        x3_slots.install_selection(layout, ota)
    expected = ota
    for record in (fallback, selection):
        if record:
            expected = x3_slots.applied(expected, record)
    require(x3_slots.boot_slot(expected) == target, 'Selection does not boot requested app')
    returns_to = None
    if not boot_only:
        # The image a failed first boot returns to: never the written slot,
        # and its whole image must verify in the backup. before-outside-md5
        # then proves the device still holds those bytes (its full slot lies
        # outside the written span).
        returns_to = x3_slots.fallback_slot(expected)
        require(returns_to != layout.newt, 'A failed first boot would return to the written slot')
        kept = SLOTS[returns_to]
        step('fallback-image', lambda: require(
            x3_slots.image_intact(backup[kept:kept + SLOT_SIZE], SLOT_SIZE),
            f'app{returns_to} ({layout.images[returns_to].kind}), the image a failed first boot returns to, '
            'fails the image check; nothing written'))
    if action == 'boot-other':
        other = backup[layout.other_offset:layout.other_offset + SLOT_SIZE]
        require(x3_slots.classify(other[:HEAD]) == layout.images[layout.other],
                'The other firmware\'s descriptor differs from session backup')
        step('other-slot-md5', lambda: verify_md5(device, layout.other_offset, other))
    # A prior install may have changed the writable slot since this session's
    # backup. Boot-only actions preserve its current bytes as well as the other slot's.
    newt_before = step('before-newt-md5', lambda: device.md5(start, span)) if boot_only else None

    def outside():
        # Everything but the written span and the selection records equals the
        # session backup: bootloader, table, NVS, the other app slot (the
        # other firmware or the running Newt) and the rest.
        for low, high in ((0, OTADATA), (OTADATA + OTADATA_SIZE, start), (end, FLASH)):
            if low < high:
                verify_md5(device, low, backup[low:high])
        if boot_only:
            require(device.md5(start, span) == newt_before, 'Boot-only action changed Newt app bytes')

    # Esptool's RAM stub keeps the application stopped throughout the proof.
    step('before-outside-md5', outside)

    def write(offset, data):
        if not unit.write_started:
            unit.begin_write()
        device.write(layout, offset, data)

    # Beside another firmware, the running Newt's slot is rewritten in place:
    # that firmware is selected and proven first, so no app byte changes
    # while the next boot could still start this slot's old record.
    if fallback:
        step('select-other-fallback', lambda: write(*fallback))
        step('fallback-md5', lambda: verify_md5(device, OTADATA, x3_slots.applied(ota, fallback)))
    # The app is proven before anything selects it.
    if not boot_only:
        step(f'app{layout.newt}-write', lambda: write(start, payload))
        step('after-outside-md5', outside)
        unit.outside_verified()
        step('written-range-md5', lambda: verify_md5(device, start, payload))
    if selection:
        step('select-other' if action == 'boot-other' else 'select-newt', lambda: write(*selection))
        step('after-select-outside-md5', outside)
        unit.outside_verified()
    elif boot_only:
        print(f'app{target} already selected; nothing written', flush=True)
    step('selection-md5', lambda: verify_md5(device, OTADATA, expected))
    # The port stays open through the reset and the handshake: reopening it
    # resets the X3 again, and a second boot before Newt confirms the new
    # selection makes the rollback bootloader abort it (docs/UPDATES.md, "Slots").
    step('reset', device.reset)
    written = ([fallback[0]] if fallback else []) + ([] if boot_only else [start]) + \
        ([selection[0]] if selection else [])
    result = {'slots': layout.report(ota), 'selection_after': layout.report(expected)['selection'],
              'written': [hex(offset) for offset in written]}
    if returns_to is not None:
        result['first_boot_fallback'] = f'app{returns_to} ({layout.images[returns_to].kind})'
    if action != 'boot-other':
        result['boot'] = step('boot-hello-status-diag', lambda: device.hello(candidate.build))
    else:
        step('confirm-window', device.confirm_window)
        result['boot'] = {'hello': 'not available: another firmware; boot identity unconfirmed'}
    return result


def installed_other(source, identity):
    """restore-other's payload: the slot install-other wrote, as the install
    session's backup holds it from before that write."""
    receipt = json_bytes((source / 'session.json').read_bytes())
    require(receipt['mac'] == identity['mac'], 'The install session belongs to another unit')
    backup = (source / 'backup.bin').read_bytes()
    require(len(backup) == FLASH and sha(backup) == receipt['backup_sha256'], 'Install session backup corrupt')
    record = json_bytes((source / OTHER_RECORD).read_bytes())
    require(record['backup_sha256'] == receipt['backup_sha256'], 'install-other record is from another backup')
    start = SLOTS[record['slot']]
    payload = backup[start:start + write_span(start, record['span'])]
    require(x3_slots.classify(payload[:HEAD]).kind in ('newt', 'empty'),
            'The backed-up slot holds neither Newt nor nothing; not restored')
    return record, payload


def install_other(device, session, identity, image, step, unit, restore=None):
    """install-other: `image`, another firmware, into the slot that does not
    boot, beside the running, selected and confirmed Newt, replacing only an
    older Newt or nothing. restore-other (`restore`, from installed_other):
    that slot's bytes from before the install, over exactly the image
    install-other wrote. Neither writes the selection, bootloader, table, NVS
    or the Newt that boots; boot-other and boot-newt select."""
    require(not (session / 'session.pending').exists(), 'Stale session.pending; inspect before backup')
    step('usb-mac-security-flash', device.connect)
    ota = step('selection', lambda: device.read(OTADATA, OTADATA_SIZE))
    heads = [step(f'app{i}-head', lambda offset=offset: device.read(offset, HEAD)) for i, offset in enumerate(SLOTS)]
    images = [x3_slots.classify(head) for head in heads]
    target, boot = x3_slots.running_newt(ota, images)
    start = SLOTS[target]
    print(f'Slots: app{boot} boots Newt; app{target} holds {images[target].kind}', flush=True)
    if restore is None:
        require(images[target].kind in ('newt', 'empty'),
                f'app{target} holds {images[target].kind}; install-other replaces only an older Newt or nothing')
        record = {'slot': target, 'length': len(image), 'md5': hashlib.md5(image).hexdigest(),
                  'sha256': sha(image), 'replaced': vars(images[target])}
    else:
        record, payload = restore
        require(record['slot'] == target, f'app{record["slot"]}, the slot install-other wrote, now boots')
        step('installed-md5', lambda: require(record.get('phase') == 'writing' or
                                              device.md5(start, record['length']) == record['md5'],
                                              f'app{target} no longer holds the image install-other wrote'))
    backup, span = backup_session(device, session, identity, start, len(payload) if restore else len(image), None, step)
    # A descriptor alone cannot prove that this Newt survives an interrupted spare write.
    kept = SLOTS[boot]
    step('surviving-image', lambda: require(
        x3_slots.image_intact(backup[kept:kept + SLOT_SIZE], SLOT_SIZE),
        f'app{boot}, the surviving Newt, fails the image check; nothing written'))
    if restore is None:
        payload = image + b'\xff' * (span - len(image))
        record |= {'span': span, 'backup_sha256': sha(backup)}
        save(session / OTHER_RECORD, record)
    end = start + len(payload)

    def outside():
        # Everything but the written span equals the session backup,
        # selection records and the running Newt included.
        for low, high in ((0, start), (end, FLASH)):
            verify_md5(device, low, backup[low:high])

    step('before-outside-md5', outside)
    unit.begin_write()
    # Only this install's unfinished write authorizes restoring nonmatching spare bytes.
    if restore is None:
        save(session / OTHER_RECORD, record | {'phase': 'writing'})
    step(f'app{target}-write', lambda: device.write_spare(boot, start, payload))
    step('after-outside-md5', outside)
    unit.outside_verified()
    step('written-range-md5', lambda: verify_md5(device, start, payload))
    if restore is None:
        save(session / OTHER_RECORD, record | {'phase': 'verified'})
    step('reset', device.reset)
    step('confirm-window', device.confirm_window)
    return {'boot': f'app{boot}', 'written': hex(start), 'span': hex(len(payload)),
            'slot_after': vars(x3_slots.classify(payload[:HEAD])), 'replaced': vars(images[target])}


def report_slots(device, step):
    """Read-only: partition table, selection records and both slot heads, then reset."""
    step('usb-mac-security-flash', device.connect)
    partition_layout(step('partition-table', lambda: device.read(0x8000, 0x1000)))
    ota = step('selection', lambda: device.read(OTADATA, OTADATA_SIZE))
    heads = [step(f'app{i}-head', lambda offset=offset: device.read(offset, HEAD)) for i, offset in enumerate(SLOTS)]
    result = {'images': [{'slot': f'app{i}', **vars(x3_slots.classify(head))} for i, head in enumerate(heads)],
              'selection': [vars(record) for record in x3_slots.selections(ota)]}
    try:
        result['decision'] = x3_slots.decide(ota, heads).report(ota)
    except x3_slots.Refused as refusal:
        result['refused'] = str(refusal)
    step('reset', device.reset)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'restore', 'boot-newt', 'boot-other', 'slots',
                                           'install-other', 'restore-other'),
                        help='install/restore write Newt\'s slot; boot-newt/boot-other only select; slots only reads; '
                             'install-other/restore-other write only the slot that does not boot')
    parser.add_argument('candidate', type=Path, nargs='?', help='Newt candidate directory, or install-other\'s image')
    parser.add_argument('--manifest-sha256')
    parser.add_argument('--image-sha256', help='install-other: the pinned SHA-256 of the image')
    parser.add_argument('--from', dest='source', type=Path, help='restore-other: the install-other session')
    parser.add_argument('--session', type=Path, required=True, help='Private directory reused for at most four hours')
    parser.add_argument('--identity-rule', type=Path,
                        help='Owner-made root-owned udev rule for this unit; default is the bench rule')
    parser.add_argument('--repair-invalid', action='store_true',
                        help='install/restore into an unreadable slot beside the running, confirmed Newt')
    args = parser.parse_args()
    needs = {'slots': (), 'install-other': ('candidate', 'image_sha256'), 'restore-other': ('source',)}
    need = needs.get(args.action, ('candidate', 'manifest_sha256'))
    if any((getattr(args, name) is not None) != (name in need)
           for name in ('candidate', 'manifest_sha256', 'image_sha256', 'source')):
        parser.error('slots takes nothing; install-other an image and --image-sha256; restore-other --from; '
                     'every other action a candidate and --manifest-sha256')
    if args.repair_invalid and args.action not in ('install', 'restore'):
        parser.error('--repair-invalid applies only to install and restore')
    os.umask(0o077)
    started = time.monotonic()
    identity = load_identity(args.identity_rule)
    candidate = image = restore = None
    if args.manifest_sha256 is not None:
        candidate = Candidate(args.candidate, args.manifest_sha256)  # Hold checked bytes in memory through the write.
        print('PASS owner udev identity, manifest, clean image, gate, resources, app checksum/digest and layout', flush=True)
    elif args.image_sha256 is not None:
        image = other_image(args.candidate, args.image_sha256)
        print('PASS owner udev identity, image pin, ESP32-C3 app checksum/digest and size', flush=True)
    elif args.source is not None:
        restore = installed_other(args.source, identity)
        print('PASS owner udev identity, install session backup and record', flush=True)
    # Reusing a restore session could make its stock backup an unusable undo image.
    args.session.mkdir(parents=True, exist_ok=args.action != 'install-other', mode=0o700)
    run = args.session / datetime.now(timezone.utc).strftime('run-%Y%m%dT%H%M%S.%fZ')
    run.mkdir(mode=0o700)
    step = Steps(run)
    receipt = {'action': args.action, 'status': 'FAIL', 'write_attempted': False,
               'identity': identity, 'identity_rule': str(args.identity_rule) if args.identity_rule else 'default bench rule'}
    if candidate:
        receipt |= {'manifest_sha256': args.manifest_sha256, 'build': candidate.build}
    elif image:
        receipt['image_sha256'] = args.image_sha256
    elif restore:
        receipt['from'] = str(args.source)
    device = unit = None
    try:
        with lease_unit(identity['mac']) as unit:
            unit.set_context(run, args.session)
            try:
                device = Device(identity)
                if candidate:
                    receipt['result'] = install(device, args.session, identity, candidate, step, unit,
                                                args.action, args.repair_invalid)
                elif image or restore:
                    receipt['result'] = install_other(device, args.session, identity, image, step, unit, restore)
                else:
                    receipt['result'] = report_slots(device, step)
                device.close()  # Keep the lease through port closure and receipt publication.
                receipt['status'] = 'PASS'
                receipt['steps'] = step.results
                receipt['write_attempted'] = unit.write_started
                receipt['seconds'] = round(time.monotonic() - started, 3)
                save(run / 'receipt.json', receipt)
                if unit.write_started:
                    unit.finish()
            finally:
                if device is not None:
                    device.close()
    except BaseException as error:
        receipt['status'] = 'FAIL'
        receipt['error'] = str(error) or type(error).__name__
        receipt['steps'] = step.results
        receipt['write_attempted'] = bool(unit and unit.write_started)
        receipt['seconds'] = round(time.monotonic() - started, 3)
        # Failure recording must not conceal the original error or weaken shared STOP.
        try:
            save(run / 'receipt.json', receipt)
        except (OSError, ValueError) as recording_error:
            print('Failure receipt unconfirmed: ' + str(recording_error), file=sys.stderr)
        print('FAIL: no automatic retry or reset. Inspect ' + str(run), file=sys.stderr, flush=True)
        raise
    finally:
        if receipt.get('result') and args.action == 'slots':
            print(json.dumps(receipt['result'], indent=2, sort_keys=True), flush=True)
        print(f"{receipt['status']} total: {time.monotonic() - started:.2f}s; receipt {run / 'receipt.json'}", flush=True)


if __name__ == '__main__':
    try:
        main()
    except (Exception, KeyboardInterrupt) as error:
        print('FAIL: ' + str(error), file=sys.stderr)
        sys.exit(1)
