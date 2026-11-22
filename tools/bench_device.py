"""The only live transport for bench_flash; explicit identity-locked port only."""
import importlib.util
from pathlib import Path
import signal
import time

from bench_candidate import FLASH, SECTOR, require, write_span
from bench_lease import usb_identity
from x3_slots import OTADATA, SLOTS

# Reset to an answered HELLO measured 6.5 s on the bench X3 (2026-10-25 install).
# Newt now confirms its image only at its first finished screen (docs/UPDATES.md), and
# its DEV bridge reads but never answers HELLO before that. Both windows still
# cover that measurement, which included the first paint; the time to the
# confirmation itself is not yet measured on a device.
HELLO_BUDGET = 20
CONFIRM_SECONDS = 30


class Device:
    def __init__(self, identity):
        self.identity, self.esp = identity, None
        self.close_after = 0
        self.cancelled = None
        self.signal_handlers = {}

    def connect(self):
        import esptool
        require(esptool.__version__ == '5.1.2', 'Use the approved project esptool 5.1.2')
        usb_identity(self.identity)  # Must pass before the port is opened.
        self.esp = esptool.detect_chip(self.identity['port'], baud=460800, connect_attempts=1)
        require(self.esp.CHIP_NAME == 'ESP32-C3', 'Expected ESP32-C3')
        esptool.cmds.read_mac(self.esp)
        require(':'.join(f'{n:02x}' for n in self.esp.read_mac()) == self.identity['mac'], 'MAC differs')
        security = self.esp.get_security_info()
        esptool.cmds.get_security_info(self.esp)
        require(not security['parsed_flags']['SECURE_BOOT_EN'] and
                security['flash_crypt_cnt'].bit_count() % 2 == 0, 'Security enabled; no write')
        self.esp = self.esp.run_stub()
        esptool.cmds.attach_flash(self.esp)
        require(esptool.cmds.detect_flash_size(self.esp) == '16MB', 'Expected 16 MiB flash')
        self.esp.flash_set_parameters(FLASH)

    def read(self, offset, length):
        return self.esp.read_flash(offset, length)

    def md5(self, offset, length):
        return self.esp.flash_md5sum(offset, length)

    def write(self, layout, offset, image):
        """The layout's writable app slot or one OTA selection sector; never
        the other firmware's slot, and without one never the Newt that boots."""
        # Defense at the transport boundary as well (x3_slots decides the layout).
        require(offset != layout.other_offset, "Never write the other firmware's slot")
        if offset in SLOTS:
            require(offset == layout.newt_offset, 'Writes go only to the writable app slot')
            write_span(offset, len(image))
        else:
            require(offset in (OTADATA, OTADATA + SECTOR) and len(image) == SECTOR,
                    'Writes go only to Newt\'s slot or one OTA selection sector')
        self.flash(offset, image)

    def write_spare(self, boot, offset, image):
        """install-other and restore-other: only the slot that does not boot,
        beside the running Newt in `boot` (x3_slots.running_newt), and nothing else."""
        require(boot in (0, 1) and offset == SLOTS[1 - boot], 'Writes go only to the slot that does not boot')
        write_span(offset, len(image))
        self.flash(offset, image)

    def flash(self, offset, image):
        import esptool
        require(len(image) % SECTOR == 0, 'Payload must cover complete erase sectors')
        # Official esptool otherwise retries both blocks and whole writes.
        esptool.loader.WRITE_BLOCK_ATTEMPTS = 1
        self.esp.WRITE_FLASH_ATTEMPTS = 1
        esptool.cmds.write_flash(self.esp, [(offset, image)], no_progress=True)

    def reset(self):
        from esptool.reset import HardReset
        self.defer_cancellation()
        # Even a reset error may have reached the unit. Keep its handle and
        # lease for the confirmation window before a close can reset it again.
        self.close_after = time.monotonic() + CONFIRM_SECONDS
        try:
            # ESP32-C3's pinned hard_reset uses HardReset(port, False).
            # Call its sequence once: ResetStrategy.__call__ retries by closing
            # and reopening the port on OSError, too early for confirmation.
            HardReset(self.esp._port).reset()
        finally:
            self.close_after = time.monotonic() + CONFIRM_SECONDS

    def confirm_window(self):
        """Give the new app time to confirm its selection: Newt at its
        first finished screen, an Arduino-based firmware in initArduino. This
        is no proof that it did.

        Applied to read-only resets and DEV errors too. No port reopen or
        control-line operation is allowed in this window. A killed process or
        physical unplug cannot be protected by a host tool.
        """
        while (remaining := self.close_after - time.monotonic()) > 0:
            try:
                time.sleep(remaining)
            except KeyboardInterrupt as error:
                self.cancelled = self.cancelled or error

    def defer_cancellation(self):
        """Arm before the reset, covering HELLO, explicit waits and cleanup.

        Signals record intent until the held port is closed. Repeated Ctrl-C
        cannot skip a finally block or release the unit lease early. SIGKILL
        and physical unplug remain outside a Python process's control.
        """
        if not self.signal_handlers:
            for number in (signal.SIGINT, signal.SIGTERM):
                self.signal_handlers[number] = signal.signal(number, self.cancel)

    def cancel(self, number, frame):
        if self.cancelled is None:
            self.cancelled = KeyboardInterrupt() if number == signal.SIGINT else SystemExit(128 + number)

    def close(self):
        while self.esp is not None:
            try:
                self.confirm_window()
                self.esp._port.close()
                self.esp = None
            except KeyboardInterrupt as error:
                self.cancelled = self.cancelled or error
        # Restore only after closure, while main still owns the unit lease.
        for number, handler in self.signal_handlers.items():
            signal.signal(number, handler)
        self.signal_handlers.clear()
        cancelled, self.cancelled = self.cancelled, None
        if cancelled is not None:
            raise cancelled

    def hello(self, expected_build):
        """Readiness and handshake on the port still open from the reset.

        Opening another port resets the X3; between a new selection's first
        boot and the app's confirmation, that second reset makes the rollback
        bootloader abort the selection (docs/UPDATES.md, "Slots").
        """
        # bench_flash holds the shared unit lease through this client and closure.
        bridge = load_bridge()
        client = held_client(bridge, self.esp._port, timeout=5)
        try:
            print(ready(bridge, client, expected_build, budget=HELLO_BUDGET))
            if 'diag' in client.capabilities or 'diag2' in client.capabilities:
                print(client.request('DIAG'))
            return client.receipt() | client.firmware_rows.report()
        finally:
            client.close()


def load_bridge():
    spec = importlib.util.spec_from_file_location('bench_dev_usb', Path(__file__).with_name('dev-usb.py'))
    bridge = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(bridge)
    return bridge


def held_client(bridge, port, timeout):
    """dev-usb's serial client over a port its owner already holds open.

    It never opens the port (that resets the X3) and never closes it: the
    owner, esptool's Device, does.
    """
    class Held(bridge.SerialClient):
        def __init__(self):
            bridge.ProtocolClient.__init__(self, timeout)
            self.connection.update(transport='serial', port=port.port, opening_may_reset_device=False, held=True)
            self.port = port
            self.connection_event('held')
            port.reset_input_buffer()  # Discard host receive bytes, not a reset command.

        def close(self):
            self.connection['closed'] = True
            self.connection_event('released')
    return Held()


def ready(bridge, client, expected_build, budget, attempt=1.0, quiet=0.5):
    """Bounded readiness after a reset, then the strict handshake.

    A HELLO sent while the device still boots can be discarded, so resend it
    until the bridge answers or the budget is spent. Replies to earlier
    attempts may still arrive; drain them until the line is quiet so the
    strict handshake sees only its own replies.
    """
    deadline = time.monotonic() + budget
    while True:
        require(time.monotonic() < deadline, 'Bridge did not answer HELLO within the readiness budget '
                '(Newt answers HELLO only once its image is confirmed, at its first finished screen)')
        client.send('N1 HELLO', deadline)
        try:
            record = client.record(min(deadline, time.monotonic() + attempt))
            break
        except bridge.ProtocolTimeout:
            continue
    while True:
        require(record[2] == 0 and record[3].startswith('HELLO '),
                'Expected only handshake replies while waiting for readiness')
        require(time.monotonic() < deadline, 'Bridge output did not settle within the readiness budget')
        try:
            record = client.record(min(deadline, time.monotonic() + quiet))
        except bridge.ProtocolTimeout:
            break
    return client.hello(expected_build)
