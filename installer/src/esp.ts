// The Device the session drives, on esptool-js over Web Serial. Opening the
// port resets the X3 into its ROM download mode; nothing here writes until the
// session asks, and the session's guard decides every address.
import { ESPLoader, ESPRESSIF_VID, Transport, USB_JTAG_SERIAL_PID } from 'esptool-js';
import type { Device } from './session.ts';

// The X3's ESP32-C3 talks over its own USB-Serial/JTAG unit, the only port
// the browser's list offers.
export const X3_PORT_FILTERS = [{ usbVendorId: ESPRESSIF_VID, usbProductId: USB_JTAG_SERIAL_PID }];

const SIZES: Record<string, number> = { '4MB': 4 << 20, '8MB': 8 << 20, '16MB': 16 << 20, '32MB': 32 << 20 };

export async function openX3(port: SerialPort): Promise<Device> {
  const transport = new Transport(port, false);
  // USB-Serial/JTAG ignores the baud rate. One rate means esptool-js never
  // reopens the port, which would reset the X3 again.
  const loader = new ESPLoader({ transport, baudrate: 115200, romBaudrate: 115200 });
  try {
    await loader.main('default_reset');
    const flashSize = await loader.detectFlashSize();
    return {
      chip: loader.chip.CHIP_NAME,
      flashBytes: (flashSize && SIZES[flashSize]) || 0,
      read: (offset, length) => loader.readFlash(offset, length),
      write: (offset, data, progress) =>
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
      md5: async (offset, length) => (await loader.flashMd5sum(offset, length)).toLowerCase(),
      reset: () => loader.after('hard_reset'),
      close: () => transport.disconnect(),
    };
  } catch (error) {
    await transport.disconnect().catch(() => {});
    throw error;
  }
}
