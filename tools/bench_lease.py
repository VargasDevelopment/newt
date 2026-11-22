"""Owner-controlled unit identity and one durable safety boundary per USB unit."""
import contextlib
import fcntl
import json
import os
from pathlib import Path
import re
import stat
import subprocess

from bench_candidate import json_bytes, require

ROOT = Path(__file__).resolve().parents[1]
UDEV_RULE = Path('/etc/udev/rules.d/99-newt-x3.rules')
MAC = r'(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}'


def load_identity(rule_path=None):
    """Validate the default or explicitly selected owner rule; never fall back."""
    path = UDEV_RULE if rule_path is None else Path(rule_path)
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NOFOLLOW)) as stream:
        info = os.fstat(stream.fileno())
        require(info.st_uid == 0 and stat.S_ISREG(info.st_mode) and not info.st_mode & 0o022,
                'Bench identity requires a root-owned, non-writable udev rule')
        lines = [line.split('#', 1)[0].strip() for line in stream if line.split('#', 1)[0].strip()]
    require(len(lines) == 1, 'Bench udev rule must contain exactly one rule line')
    line = lines[0]
    vendors = re.findall(r'ATTRS\{idVendor\}\s*==\s*"([^"]*)"', line)
    serials = re.findall(r'ATTRS\{serial\}\s*==\s*"([^"]*)"', line)
    require(vendors == ['303a'] and line.count('ATTRS{idVendor}') == 1 and
            len(serials) == 1 and line.count('ATTRS{serial}') == 1 and
            re.fullmatch(MAC, serials[0]) is not None,
            'Bench udev rule needs exactly one Espressif vendor and one MAC serial')
    mac = serials[0].lower()
    return {'mac': mac, 'port': '/dev/serial/by-id/usb-Espressif_USB_JTAG_serial_debug_unit_' +
            mac.upper() + '-if00'}


def port_serial(port):
    """Read only the opened port's USB ancestry; never enumerate serial devices."""
    target = Path(port).resolve(strict=True)
    require(target.parent == Path('/dev') and target.name.startswith('ttyACM'), 'Unexpected USB serial port type')
    node = (Path('/sys/class/tty') / target.name / 'device').resolve(strict=True)
    for parent in (node, *node.parents):
        serial = parent / 'serial'
        if serial.is_file():
            value = serial.read_text().strip()
            require(re.fullmatch(MAC, value) is not None and
                    (parent / 'idVendor').read_text().strip() == '303a' and
                    (parent / 'idProduct').read_text().strip() == '1001',
                    'Expected an Espressif USB-Serial/JTAG descriptor')
            return value.lower()
    raise ValueError('USB serial descriptor unavailable')


def usb_identity(identity):
    require(port_serial(identity['port']) == identity['mac'], 'USB descriptor is not the owner-selected unit')


def shared_state_root():
    environment = {name: value for name, value in os.environ.items() if not name.startswith('GIT_')}
    common = subprocess.check_output(['git', 'rev-parse', '--git-common-dir'],
                                     cwd=ROOT, env=environment, text=True).strip()
    return (ROOT / common).resolve() / 'newt-bench'


def save(path, value):
    temporary = path.with_suffix('.pending')
    require(not temporary.exists(), f'Stale {temporary.name}; inspect it before continuing')
    with temporary.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    temporary.rename(path)
    sync_directory(path.parent)


def sync_directory(path):
    descriptor = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


class UnitLease:
    def __init__(self, mac, state_root=None):
        require(re.fullmatch(MAC, mac) is not None, 'Invalid USB MAC serial')
        root = shared_state_root() if state_root is None else Path(state_root)
        self.directory = root / mac.lower().replace(':', '')
        self.stop = self.directory / 'STOP'
        self.journal = self.directory / 'write.json'
        self.context = {}
        self.write_started = False
        self.outside_intact = False

    def check_clear(self):
        require(not self.stop.exists(), f'Bench stopped; inspect {self.stop} before any device access')
        pending = self.journal.with_suffix('.pending')
        if pending.exists() or self.journal.exists():
            try:
                record = json_bytes((pending if pending.exists() else self.journal).read_bytes())
                require(isinstance(record, dict) and record.get('phase') in ('writing', 'verified') and
                        record.get('outside_md5') in ('unknown', 'intact') and
                        all(isinstance(record.get(key), str) and record[key] for key in
                            ('run_dir', 'session_dir', 'backup_path')) and
                        (record['phase'] != 'verified' or record['outside_md5'] == 'intact'),
                        'Invalid shared write record')
            except (ValueError, OSError):
                record = {'journal_path': str(self.journal), 'phase': 'unknown'}
            if pending.exists() or record.get('phase') != 'verified':
                self.context = record
                self.outside_intact = not pending.exists() and record.get('outside_md5') == 'intact'
                self.record_stop('Interrupted or unconfirmed write; inspect the saved outside-flash proof')
                raise ValueError(f'Bench stopped; unconfirmed shared write attempt requires recovery review: {self.stop}')

    def set_context(self, run, session):
        self.context = {'run_dir': str(Path(run).resolve()), 'session_dir': str(Path(session).resolve()),
                        'backup_path': str((Path(session) / 'backup.bin').resolve())}

    def begin_write(self):
        # Persist uncertainty before the first erase/write call can reach the unit.
        save(self.journal, self.context | {'phase': 'writing', 'outside_md5': 'unknown'})
        self.write_started = True

    def outside_verified(self):
        self.outside_intact = True
        save(self.journal, self.context | {'phase': 'writing', 'outside_md5': 'intact'})

    def finish(self):
        require(self.write_started and self.outside_intact, 'Write proof incomplete')
        # A successful attempt is durable independently of session receipts or cleanup.
        save(self.journal, self.context | {'phase': 'verified', 'outside_md5': 'intact'})
        self.write_started = False

    def record_stop(self, error):
        # No tool clears STOP. Preserve the first failure and its recovery breadcrumbs.
        if not self.stop.exists():
            with self.stop.open('x') as stream:
                json.dump(self.context | {'error': str(error),
                    'outside_md5': 'intact' if self.outside_intact else 'unknown',
                    'decision': 'Lead' if self.outside_intact else 'owner'}, stream, indent=2)
                stream.write('\n')
                stream.flush()
                os.fsync(stream.fileno())
            sync_directory(self.directory)

    @contextlib.contextmanager
    def hold(self):
        self.directory.mkdir(parents=True, exist_ok=True, mode=0o700)
        sync_directory(self.directory.parent.parent)
        sync_directory(self.directory.parent)
        # This inode survives sessions, worktrees and temp cleanup; never unlink it.
        with (self.directory / 'lock').open('a') as lock:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except BlockingIOError as error:
                raise ValueError('bench is leased by another live tool') from error
            self.check_clear()
            try:
                yield self
            except BaseException as error:
                if self.write_started:
                    self.record_stop(error)
                raise


@contextlib.contextmanager
def lease_unit(mac, state_root=None):
    with UnitLease(mac, state_root).hold() as unit:
        yield unit


def lease_port(port):
    return lease_unit(port_serial(port))
