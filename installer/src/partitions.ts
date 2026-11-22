// The device's partition table, read from 0x8000, and the X3 tables this page
// knows. The page installs only on a known table and takes every address it
// writes from it; it never writes the table itself.
import { fromHex, md5Hex, toHex } from './digest.ts';
import { OTADATA_BYTES, Refused, type SlotLayout } from './slots.ts';

export const TABLE_OFFSET = 0x8000;
export const TABLE_BYTES = 0xc00; // ESP-IDF's largest table
const ENTRY_MAGIC = 0x50aa;
const MD5_MAGIC = 0xebeb;

export interface Partition {
  readonly type: number;
  readonly subtype: number;
  readonly offset: number;
  readonly size: number;
  readonly label: string;
  readonly flags: number;
}

const APP = 0;
const DATA = 1;
const OTA_0 = 0x10;
const OTA_1 = 0x11;
const OTA_DATA = 0;

// The X3 factory table (firmware/x3_existing_partitions.csv). Newt builds
// against it and bench_flash.py checks the bench unit holds exactly it.
// The X4-shaped table some X3s may carry (app1 at 0x650000) is not here:
// nobody has installed Newt on one yet (docs/092-install-page.md).
export const X3_TABLE: readonly Partition[] = [
  { type: DATA, subtype: 0x02, offset: 0x9000, size: 0x5000, label: 'nvs', flags: 0 },
  { type: DATA, subtype: OTA_DATA, offset: 0xe000, size: 0x2000, label: 'otadata', flags: 0 },
  { type: APP, subtype: OTA_0, offset: 0x10000, size: 0x770000, label: 'app0', flags: 0 },
  { type: APP, subtype: OTA_1, offset: 0x780000, size: 0x770000, label: 'app1', flags: 0 },
  { type: DATA, subtype: 0x82, offset: 0xef0000, size: 0x100000, label: 'spiffs', flags: 0 },
  { type: DATA, subtype: 0x03, offset: 0xff0000, size: 0x10000, label: 'coredump', flags: 0 },
];
const KNOWN_TABLES: readonly (readonly Partition[])[] = [X3_TABLE];

// The entries of a well-formed table: 32-byte entries, then the MD5 entry
// over them, then erased bytes. Anything else is null.
export function parseTable(bytes: Uint8Array): Partition[] | null {
  if (bytes.length !== TABLE_BYTES) return null;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const entries: Partition[] = [];
  for (let at = 0; at + 32 <= bytes.length; at += 32) {
    const magic = view.getUint16(at, true);
    if (magic === MD5_MAGIC) {
      const md5 = toHex(bytes.subarray(at + 16, at + 32));
      const rest = bytes.subarray(at + 32);
      const filler = bytes.subarray(at + 2, at + 16);
      return md5 === md5Hex(bytes.subarray(0, at)) && filler.every((b) => b === 0xff) && rest.every((b) => b === 0xff)
        ? entries
        : null;
    }
    if (magic !== ENTRY_MAGIC) return null;
    const labelBytes = bytes.subarray(at + 12, at + 28);
    const end = labelBytes.indexOf(0);
    entries.push({
      type: bytes[at + 2]!,
      subtype: bytes[at + 3]!,
      offset: view.getUint32(at + 4, true),
      size: view.getUint32(at + 8, true),
      label: String.fromCharCode(...(end < 0 ? labelBytes : labelBytes.subarray(0, end))),
      flags: view.getUint32(at + 28, true),
    });
  }
  return null;
}

function same(a: Partition, b: Partition): boolean {
  return (
    a.type === b.type &&
    a.subtype === b.subtype &&
    a.offset === b.offset &&
    a.size === b.size &&
    a.label === b.label &&
    a.flags === b.flags
  );
}

// The slots and otadata of a known table, or Refused('unknown-layout').
export function knownLayout(bytes: Uint8Array): SlotLayout {
  const entries = parseTable(bytes);
  const known = entries && KNOWN_TABLES.find((table) => table.length === entries.length && table.every((p, i) => same(p, entries[i]!)));
  if (!known) throw new Refused('unknown-layout', 'The partition table is not a known X3 table');
  const find = (type: number, subtype: number): Partition => {
    const found = known.find((p) => p.type === type && p.subtype === subtype);
    if (!found) throw new Error('A known table lacks an OTA partition');
    return found;
  };
  const otadata = find(DATA, OTA_DATA);
  if (otadata.size !== OTADATA_BYTES) throw new Error('A known table has an unexpected otadata size');
  const [app0, app1] = [find(APP, OTA_0), find(APP, OTA_1)];
  return { otadata: otadata.offset, slots: [{ offset: app0.offset, size: app0.size }, { offset: app1.offset, size: app1.size }] };
}

// A table's bytes as ESP-IDF's gen_esp32part writes them (tests and the gallery).
export function tableBytes(entries: readonly Partition[]): Uint8Array {
  const bytes = new Uint8Array(TABLE_BYTES).fill(0xff);
  const view = new DataView(bytes.buffer);
  entries.forEach((p, i) => {
    const at = 32 * i;
    view.setUint16(at, ENTRY_MAGIC, true);
    bytes[at + 2] = p.type;
    bytes[at + 3] = p.subtype;
    view.setUint32(at + 4, p.offset, true);
    view.setUint32(at + 8, p.size, true);
    bytes.fill(0, at + 12, at + 28);
    bytes.set([...p.label].map((c) => c.charCodeAt(0)), at + 12);
    view.setUint32(at + 28, p.flags, true);
  });
  const at = 32 * entries.length;
  view.setUint16(at, MD5_MAGIC, true);
  bytes.set(fromHex(md5Hex(bytes.subarray(0, at))), at + 16);
  return bytes;
}
