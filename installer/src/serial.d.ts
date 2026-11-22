// The part of Web Serial (https://wicg.github.io/serial/) the page uses.
// TypeScript's DOM library does not include it yet; esptool-js names these
// global types too.
interface SerialPortInfo {
  readonly usbVendorId?: number;
  readonly usbProductId?: number;
}

interface SerialPort extends EventTarget {
  getInfo(): SerialPortInfo;
}

interface Serial extends EventTarget {
  requestPort(options?: { filters?: SerialPortInfo[] }): Promise<SerialPort>;
}

interface Navigator {
  readonly serial?: Serial;
}
