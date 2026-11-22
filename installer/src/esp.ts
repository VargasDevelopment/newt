// The Device the session drives, on esptool-js over Web Serial. Opening the
// port resets the X3 into its ROM download mode; nothing here writes until the
// session asks, and the session's guard decides every address.
import { ESPLoader, ESPRESSIF_VID, Transport, USB_JTAG_SERIAL_PID } from 'esptool-js';
import { md5Hex, toHex } from './digest.ts';
import type { Device } from './session.ts';

// The X3's ESP32-C3 talks over its own USB-Serial/JTAG unit, the only port
// the browser's list offers.
export const X3_PORT_FILTERS = [{ usbVendorId: ESPRESSIF_VID, usbProductId: USB_JTAG_SERIAL_PID }];

const SIZES: Record<string, number> = { '4MB': 4 << 20, '8MB': 8 << 20, '16MB': 16 << 20, '32MB': 32 << 20 };

// esptool-js 0.7.1's after('hard_reset') only releases RTS, which the connect
// sequence already released, so the chip stays in the flasher stub
// (espressif/esptool-js#279). This is esptool's HardReset: EN low (RTS) with
// IO9 high (DTR released), then released, so the X3 starts its firmware.
const HARD_RESET = 'D0|R1|W200|R0|W200';

// On macOS, Chrome reads the port with PARMRK set, so the kernel queues each
// 0xFF byte twice, and its 1024-byte input queue silently drops the rest of a
// burst. esptool-js streams 4 KiB packets; erased flash (the partition table,
// otadata) is mostly 0xFF, so bytes vanish and the stub then waits forever for
// an ack. The stub sends no more than one unacked block, so a 256-byte block
// fills at most half that queue, even when every byte is 0xFF.
export const READ_BLOCK = 256;
// Transport.read restarts this on every byte, so it bounds silence, not a whole
// packet: a stream that keeps trickling bytes can run longer. esptool-js waits 100 s.
const READ_TIMEOUT_MS = 3000;

// What readFlash needs of an ESPLoader, so a test can stand in for the stub.
export type FlashReader = Pick<ESPLoader, 'checkCommand' | 'ESP_READ_FLASH'> & {
  transport: Pick<Transport, 'read' | 'write'>;
};

// The flasher stub's READ_FLASH as esptool runs it, with one READ_BLOCK in
// flight: each packet must be whole, and the stub's closing MD5 must match.
export async function readFlash(loader: FlashReader, offset: number, length: number): Promise<Uint8Array> {
  const args = new DataView(new ArrayBuffer(16));
  [offset, length, READ_BLOCK, READ_BLOCK].forEach((word, i) => args.setUint32(4 * i, word, true));
  await loader.checkCommand('read flash', loader.ESP_READ_FLASH, new Uint8Array(args.buffer));
  const data = new Uint8Array(length);
  const ack = new DataView(new ArrayBuffer(4));
  for (let received = 0; received < length; ) {
    const packet = await loader.transport.read(READ_TIMEOUT_MS);
    const expected = Math.min(READ_BLOCK, length - received);
    if (packet.length !== expected)
      throw new Error(`read at 0x${(offset + received).toString(16)}: got ${packet.length} bytes, expected ${expected}`);
    data.set(packet, received);
    received += expected;
    ack.setUint32(0, received, true);
    await loader.transport.write(new Uint8Array(ack.buffer));
  }
  const digest = toHex(await loader.transport.read(READ_TIMEOUT_MS));
  if (digest !== md5Hex(data)) throw new Error(`read at 0x${offset.toString(16)}: the stub's MD5 ${digest} does not match the data`);
  return data;
}

// `debug` (the page's ?debug) logs esptool-js's own steps, every byte of the
// connect and the small reads, and each operation's outcome and time.
export async function openX3(port: SerialPort, debug = false): Promise<Device> {
  const transport = new Transport(port, debug);
  // USB-Serial/JTAG ignores the baud rate. One rate means esptool-js never
  // reopens the port, which would reset the X3 again.
  const loader = new ESPLoader({ transport, baudrate: 115200, romBaudrate: 115200, debugLogging: debug });
  const reset = () => loader.after('custom_reset', undefined, HARD_RESET);
  // Byte traces of big reads or writes would fill the log (and memory).
  const step = async <T>(name: string, bytes: number, run: () => Promise<T>): Promise<T> => {
    if (!debug) return run();
    transport.tracing = bytes <= 0x4000;
    const started = performance.now();
    try {
      const result = await run();
      console.log(`newt: ${name} ok in ${Math.round(performance.now() - started)} ms`);
      return result;
    } catch (error) {
      console.log(`newt: ${name} FAILED after ${Math.round(performance.now() - started)} ms: ${String(error)}`);
      throw error;
    }
  };
  try {
    await loader.main('default_reset');
    const flashSize = await loader.detectFlashSize();
    return {
      chip: loader.chip.CHIP_NAME,
      flashBytes: (flashSize && SIZES[flashSize]) || 0,
      read: (offset, length) => step(`read 0x${offset.toString(16)}+0x${length.toString(16)}`, length, () => readFlash(loader, offset, length)),
      write: (offset, data, progress) =>
        step(`write 0x${offset.toString(16)}+0x${data.length.toString(16)}`, data.length, () =>
          loader.writeFlash({
            fileArray: [{ data, address: offset }],
            // Never patch image headers or erase anything beyond the written sectors.
            flashMode: 'keep',
            flashFreq: 'keep',
            flashSize: 'keep',
            eraseAll: false,
            compress: true,
            reportProgress: (_file, written, total) => progress(total ? written / total : 1),
          }),
        ),
      md5: (offset, length) => step(`md5 0x${offset.toString(16)}+0x${length.toString(16)}`, 0, async () => (await loader.flashMd5sum(offset, length)).toLowerCase()),
      reset: () => step('reset', 0, reset),
      close: () => step('close', 0, () => transport.disconnect()),
    };
  } catch (error) {
    if (debug) console.log(`newt: connect FAILED: ${String(error)}`);
    // The connect may have stopped with the X3 in download mode: restart it.
    await reset().catch(() => {});
    await transport.disconnect().catch(() => {});
    throw error;
  }
}
