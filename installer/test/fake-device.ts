// A 16 MiB X3 in memory behind the session's Device interface, with fault
// switches. It records every write, so tests can prove what was touched.
import { md5Hex } from '../src/digest.ts';
import { appImage, otadata, X3_LAYOUT, type AppSpec, type RecordSpec } from '../src/fixtures.ts';
import { TABLE_OFFSET, tableBytes, X3_TABLE } from '../src/partitions.ts';
import { type Device, FLASH_BYTES } from '../src/session.ts';

export interface Faults {
  write?: (offset: number) => boolean; // true: this write fails
  tearAt?: number; // a failing app write stops after this many bytes
  md5?: 'wrong' | 'throw';
  read?: (offset: number, count: number) => boolean; // true: this read fails
  selectionLands?: boolean; // the otadata write lands even though it reports failure
}

export class FakeX3 implements Device {
  readonly flash = new Uint8Array(FLASH_BYTES).fill(0xff);
  readonly writes: [offset: number, length: number][] = [];
  readonly events: string[] = [];
  chip = 'ESP32-C3';
  flashBytes = FLASH_BYTES;
  reads = 0;
  faults: Faults = {};

  async read(offset: number, length: number): Promise<Uint8Array> {
    this.reads++;
    if (this.faults.read?.(offset, this.reads)) throw new Error('read failed');
    return this.flash.slice(offset, offset + length);
  }

  async write(offset: number, data: Uint8Array, progress: (fraction: number) => void): Promise<void> {
    this.events.push(`write 0x${offset.toString(16)}`);
    this.writes.push([offset, data.length]);
    if (this.faults.write?.(offset)) {
      if (this.faults.tearAt !== undefined) this.flash.set(data.subarray(0, this.faults.tearAt), offset);
      if (this.faults.selectionLands) this.flash.set(data, offset);
      throw new Error('write failed');
    }
    // As the flasher stub does: whole sectors are erased, then programmed.
    this.flash.fill(0xff, offset, offset + Math.ceil(data.length / 4096) * 4096);
    this.flash.set(data, offset);
    progress(0.5);
    progress(1);
  }

  async md5(offset: number, length: number): Promise<string> {
    if (this.faults.md5 === 'throw') throw new Error('md5 failed');
    const digest = md5Hex(this.flash.subarray(offset, offset + length));
    return this.faults.md5 === 'wrong' ? digest.replace(/^./, (c) => (c === '0' ? '1' : '0')) : digest;
  }

  async reset(): Promise<void> {
    this.events.push('reset');
  }

  async close(): Promise<void> {
    this.events.push('close');
  }

  ota(): Uint8Array {
    return this.flash.slice(X3_LAYOUT.otadata, X3_LAYOUT.otadata + 0x2000);
  }
}

export async function x3(slots: readonly [AppSpec | null, AppSpec | null], records: readonly [RecordSpec, RecordSpec]): Promise<FakeX3> {
  const device = new FakeX3();
  device.flash.set(tableBytes(X3_TABLE), TABLE_OFFSET);
  device.flash.set(otadata(...records), X3_LAYOUT.otadata);
  for (const [index, spec] of slots.entries()) if (spec) device.flash.set(await appImage(spec), X3_LAYOUT.slots[index]!.offset);
  return device;
}
