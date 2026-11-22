import { describe, expect, it } from 'vitest';
import { sha256Hex } from '../src/digest.ts';
import { knownLayout, parseTable, type Partition, TABLE_BYTES, tableBytes, X3_TABLE } from '../src/partitions.ts';
import { Refused } from '../src/slots.ts';

function reason(bytes: Uint8Array): string | null {
  try {
    knownLayout(bytes);
    return null;
  } catch (error) {
    if (error instanceof Refused) return error.reason;
    throw error;
  }
}

describe('partition tables', () => {
  it("builds the firmware's partitions.bin byte for byte", async () => {
    // SHA-256 of .pio/regional-dev-usb/build/newt_x3/partitions.bin, generated
    // from firmware/x3_existing_partitions.csv; bench_flash checks the bench holds it.
    expect(await sha256Hex(tableBytes(X3_TABLE))).toBe('334c9553e2799107ed33eb9057f8661c6dd757c662a661edac6c73bb99ae7d5e');
  });

  it('takes slots and otadata from the X3 table', () => {
    expect(knownLayout(tableBytes(X3_TABLE))).toEqual({
      otadata: 0xe000,
      slots: [
        { offset: 0x10000, size: 0x770000 },
        { offset: 0x780000, size: 0x770000 },
      ],
    });
    expect(parseTable(tableBytes(X3_TABLE))).toEqual(X3_TABLE);
  });

  const changed = (index: number, change: Partial<Partition>) => X3_TABLE.map((p, i) => (i === index ? { ...p, ...change } : p));

  it.each([
    // The X4-shaped table some X3s may carry: known to exist, not yet proven with Newt.
    ['the X4 layout', tableBytes(changed(2, { size: 0x640000 }).map((p, i) => (i === 3 ? { ...p, offset: 0x650000, size: 0x640000 } : p)))],
    ['a moved app1', tableBytes(changed(3, { offset: 0x790000 }))],
    ['a renamed slot', tableBytes(changed(2, { label: 'ota_0' }))],
    ['a flagged partition', tableBytes(changed(4, { flags: 1 }))],
    ['an extra partition', tableBytes([...X3_TABLE, { type: 1, subtype: 0x81, offset: 0xfe0000, size: 0x1000, label: 'extra', flags: 0 }])],
    ['a missing partition', tableBytes(X3_TABLE.slice(0, 5))],
    ['a factory app', tableBytes(changed(4, { type: 0, subtype: 0, label: 'factory' }))],
  ])('refuses %s', (_name, bytes) => expect(reason(bytes)).toBe('unknown-layout'));

  it('refuses damaged or incomplete table bytes', () => {
    const good = tableBytes(X3_TABLE);
    const flip = (at: number) => {
      const bytes = new Uint8Array(good);
      bytes[at]! ^= 1;
      return bytes;
    };
    expect(reason(flip(0x44))).toBe('unknown-layout'); // an entry byte: the MD5 differs
    expect(reason(flip(0xd0))).toBe('unknown-layout'); // the MD5 itself
    expect(reason(flip(0xc4))).toBe('unknown-layout'); // the MD5 entry's filler
    expect(reason(flip(TABLE_BYTES - 1))).toBe('unknown-layout'); // data after the table
    expect(reason(good.subarray(0, TABLE_BYTES - 1))).toBe('unknown-layout');
    expect(reason(new Uint8Array(TABLE_BYTES).fill(0xff))).toBe('unknown-layout'); // erased
    const noMd5 = new Uint8Array(good);
    noMd5.fill(0xff, 0xc0);
    expect(reason(noMd5)).toBe('unknown-layout');
  });
});
