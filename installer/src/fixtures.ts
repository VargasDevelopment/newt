// Synthetic images and otadata for the tests and the screenshot gallery,
// shaped like tests/bench_flash_fixtures.py's app_bytes. Never shipped in the page.
import { sha256Hex, fromHex } from './digest.ts';
import { X3_TABLE, knownLayout, tableBytes } from './partitions.ts';
import { OTADATA_BYTES, SECTOR, seqCrc } from './slots.ts';

export const X3_LAYOUT = knownLayout(tableBytes(X3_TABLE));

export interface AppSpec {
  readonly project: string;
  readonly version: string;
  readonly newt?: { readonly version: string; readonly build: string };
  readonly extra?: number; // payload bytes, to make bigger images
}

const ascii = (text: string) => Uint8Array.from([...text].map((c) => c.charCodeAt(0)));

// One segment: the app descriptor, Newt's mark and identity when `newt`, then filler.
export async function appImage(spec: AppSpec): Promise<Uint8Array> {
  const descriptor = new Uint8Array(256);
  new DataView(descriptor.buffer).setUint32(0, 0xabcd5432, true);
  descriptor.set(ascii(spec.version), 16);
  descriptor.set(ascii(spec.project), 48);
  const mark = new Uint8Array(56);
  if (spec.newt) {
    mark.set(ascii('NEWT-X3-APP\0'));
    mark[12] = 1;
    mark.set(ascii(spec.newt.version), 16);
    mark.set(ascii(spec.newt.build), 40);
  }
  const filler = Uint8Array.from({ length: 255 + (spec.extra ?? 0) }, (_, i) => i & 0xff);
  // Segment lengths are a multiple of 4, as esptool writes them; the zeros keep the checksum.
  const unpadded = descriptor.length + mark.length + filler.length;
  const payload = new Uint8Array(unpadded + (-unpadded & 3));
  payload.set(descriptor);
  payload.set(mark, descriptor.length);
  payload.set(filler, descriptor.length + mark.length);
  const end = 24 + 8 + payload.length;
  const digestAt = (Math.floor(end / 16) + 1) * 16;
  const data = new Uint8Array(digestAt + 32);
  const view = new DataView(data.buffer);
  data.set([0xe9, 0x01, 0x02, 0x4f]);
  data[12] = 5; // ESP32-C3
  data[23] = 1; // an appended SHA-256
  view.setUint32(24, 0x3c000020, true);
  view.setUint32(28, payload.length, true);
  data.set(payload, 32);
  data[digestAt - 1] = payload.reduce((sum, byte) => sum ^ byte, 0xef);
  data.set(fromHex(await sha256Hex(data.subarray(0, digestAt))), digestAt);
  return data;
}

export type RecordSpec = readonly [seq: number, state: number] | null;

// Both otadata sectors: each record (seq, state), or erased.
export function otadata(first: RecordSpec, second: RecordSpec): Uint8Array {
  const data = new Uint8Array(OTADATA_BYTES).fill(0xff);
  const view = new DataView(data.buffer);
  [first, second].forEach((record, index) => {
    if (!record) return;
    const [seq, state] = record;
    view.setUint32(index * SECTOR, seq, true);
    view.setUint32(index * SECTOR + 24, state, true);
    view.setUint32(index * SECTOR + 28, seqCrc(seq), true);
  });
  return data;
}

export const CROSSPOINT: AppSpec = { project: 'crosspoint-reader', version: '2.0.0' };
export const CROSSINK: AppSpec = { project: 'CrossInk', version: 'v1.6.1' };
export const STOCK: AppSpec = { project: 'arduino-lib-builder', version: 'idf-master-106-gc9c8a06-dirty' };
export const NEWT: AppSpec = { project: 'arduino-lib-builder', version: '0ee595f', newt: { version: '0.1.0-alpha.11', build: '0ee595fb7ae0' } };
