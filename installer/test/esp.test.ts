import { describe, expect, it } from 'vitest';
import { fromHex, md5Hex } from '../src/digest.ts';
import { type FlashReader, readFlash } from '../src/esp.ts';

// The flasher stub's READ_FLASH (esptool-legacy-flasher-stub handle_flash_read)
// behind macOS's tty input queue as Chrome configures it: 1024 bytes, each
// 0xFF stored twice (PARMRK), and whatever does not fit in one burst lost.
// The host drains the queue before it acks, so each burst starts empty.
const QUEUE = 1024;

function cost(payload: Uint8Array): number {
  return payload.reduce((sum, byte) => sum + (byte === 0xff || byte === 0xc0 || byte === 0xdb ? 2 : 1), 2);
}

class StubBehindMacTty implements FlashReader {
  readonly ESP_READ_FLASH = 0xd2;
  readonly #delivered: Uint8Array[] = [];
  #job = { offset: 0, length: 0, block: 0, inFlight: 0, sent: 0 };

  constructor(readonly flash: Uint8Array) {}

  async checkCommand(_what?: string, _op?: number | null, data = new Uint8Array(0)): Promise<number> {
    const view = new DataView(data.buffer, data.byteOffset);
    const [offset, length, block, inFlight] = [0, 1, 2, 3].map((i) => view.getUint32(4 * i, true)) as [number, number, number, number];
    this.#job = { offset, length, block, inFlight, sent: 0 };
    this.#burst(0);
    return 0;
  }

  readonly transport = {
    read: async (_timeout: number): Promise<Uint8Array> => {
      const frame = this.#delivered.shift();
      if (!frame) throw new Error('No serial data received.');
      return frame;
    },
    write: async (ack: Uint8Array): Promise<void> => this.#burst(new DataView(ack.buffer).getUint32(0, true)),
  };

  // Everything the stub sends before it waits for the next ack.
  #burst(acked: number): void {
    const job = this.#job;
    const frames: Uint8Array[] = [];
    while (job.sent < job.length && job.sent - acked < job.inFlight) {
      const n = Math.min(job.block, job.length - job.sent);
      frames.push(this.flash.slice(job.offset + job.sent, job.offset + job.sent + n));
      job.sent += n;
    }
    if (acked >= job.length) frames.push(fromHex(md5Hex(this.flash.slice(job.offset, job.offset + job.length))));
    let room = QUEUE;
    for (const frame of frames) {
      if (cost(frame) > room) return; // a frame without its end never arrives
      room -= cost(frame);
      this.#delivered.push(frame);
    }
  }
}

describe('readFlash over macOS Chrome', () => {
  const flash = new Uint8Array(0x10000).fill(0xff); // erased, as most of a table or otadata is
  flash.set(Uint8Array.from({ length: 0x1000 }, (_, i) => (i * 7) & 0xff), 0x8000);

  it('reads erased and mixed flash whole', async () => {
    const stub = new StubBehindMacTty(flash);
    expect(await readFlash(stub, 0xe000, 0x2000)).toEqual(flash.slice(0xe000, 0x10000));
    expect(await readFlash(stub, 0x7f00, 0xc00)).toEqual(flash.slice(0x7f00, 0x8b00));
  });

  it("loses esptool-js's 4 KiB packets of erased flash (the model's control)", async () => {
    const stub = new StubBehindMacTty(flash);
    const args = new DataView(new ArrayBuffer(16));
    [0xe000, 0x2000, 0x1000, 1024].forEach((word, i) => args.setUint32(4 * i, word, true));
    await stub.checkCommand('read flash', 0xd2, new Uint8Array(args.buffer));
    await expect(stub.transport.read(100_000)).rejects.toThrow('No serial data');
  });

  it('refuses data that does not match the stub digest', async () => {
    const stub = new StubBehindMacTty(flash);
    const read = stub.transport.read;
    let first = true;
    stub.transport.read = async (timeout) => {
      const frame = await read(timeout);
      if (first) frame.fill(0xaa, 0, 1); // flash holds 0x00 there
      first = false;
      return frame;
    };
    await expect(readFlash(stub, 0x8000, 0x100)).rejects.toThrow('MD5');
  });
});
