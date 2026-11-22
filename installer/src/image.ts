// Firmware images: the whole-image check the bootloader relies on (ported
// from x3_slots.image_intact / appImageIntact), the checks a chosen file must
// pass, and Newt's own release pair, newt.bin and newt.manifest.
import { sha256Hex, toHex } from './digest.ts';
import { X3_TABLE } from './partitions.ts';
import { classify, HEAD_BYTES, type SlotImage, UNKNOWN_FIRMWARE } from './slots.ts';

const APP_MAGIC = 0xe9;
const ESP32C3_CHIP_ID = 5;
const MMU_PAGE_BYTES = 0x10000;

// As the bootloader's verify_segment_header (segmentPlaced in
// app_slot_identity.h): a length that is a multiple of 4, and a segment the
// cache maps (DROM or IROM) at the same offset in its 64 KiB MMU page as its
// data. Both slots start on a page, so the offset in the slot counts.
function segmentPlaced(load: number, dataAt: number, length: number): boolean {
  const mapped = (load >= 0x3c000000 && load < 0x3c800000) || (load >= 0x42000000 && load < 0x42800000);
  return length % 4 === 0 && (!mapped || load % MMU_PAGE_BYTES === dataAt % MMU_PAGE_BYTES);
}

interface Walk {
  readonly digestAt: number; // ends the checksum's 16-byte block
  readonly checksum: number;
  readonly end: number; // past the appended SHA-256, or digestAt without one
}

// The bytes a walk needs next: its read reached past `data` but stays inside the slot.
interface Short {
  readonly need: number;
}

// The digest offset, XOR checksum and end of an ESP32-C3 image with 1 to 16
// segments inside `slotBytes`, its SHA-256 appended when header byte 23 is 1
// (0: none; anything else is refused); Short when `data` ends before the walk
// does; else null.
function walk(data: Uint8Array, slotBytes: number): Walk | Short | null {
  const read = (at: number, count: number): Uint8Array | Short | null =>
    at + count > slotBytes ? null : at + count > data.length ? { need: at + count } : data.subarray(at, at + count);
  const header = read(0, 24);
  if (!(header instanceof Uint8Array)) return header;
  const view = new DataView(header.buffer, header.byteOffset, header.byteLength);
  const segments = header[1]!;
  if (header[0] !== APP_MAGIC || segments === 0 || segments > 16 || view.getUint16(12, true) !== ESP32C3_CHIP_ID || header[23]! > 1)
    return null;
  let at = 24;
  let checksum = 0xef;
  for (let i = 0; i < segments; ++i) {
    const entry = read(at, 8);
    if (!(entry instanceof Uint8Array)) return entry;
    at += 8;
    const fields = new DataView(entry.buffer, entry.byteOffset, 8);
    const length = fields.getUint32(4, true);
    if (!segmentPlaced(fields.getUint32(0, true), at, length)) return null;
    const segment = read(at, length);
    if (!(segment instanceof Uint8Array)) return segment;
    for (const byte of segment) checksum ^= byte;
    at += length;
  }
  const digestAt = (Math.floor(at / 16) + 1) * 16;
  const end = digestAt + (header[23] === 1 ? 32 : 0);
  return end > slotBytes ? null : { digestAt, checksum, end };
}

// A complete walk of `data` alone: reads past it fail, as the firmware's
// failed flash reads do (x3_slots._walk).
function walked(data: Uint8Array, slotBytes: number): Walk | null {
  const result = walk(data, slotBytes);
  return result && 'end' in result ? result : null;
}

// The structure walk checks, the XOR checksum as the last byte of its 16-byte
// block and, when appended, the SHA-256 of every byte before the digest.
export async function imageIntact(data: Uint8Array, slotBytes: number): Promise<boolean> {
  const found = walked(data, slotBytes);
  if (!found || found.end > data.length || data[found.digestAt - 1] !== found.checksum) return false;
  if (found.end === found.digestAt) return true;
  const digest = await sha256Hex(data.subarray(0, found.digestAt));
  return digest === toHex(data.subarray(found.digestAt, found.digestAt + 32));
}

// The bytes the image at the start of `data` occupies, through its appended
// SHA-256 when it has one, or null when its structure is not complete inside `data`.
export function imageLength(data: Uint8Array): number | null {
  const found = walked(data, data.length);
  return found ? found.end : null;
}

// Whether a chosen file holds bytes other than erased padding (0xff) after the
// image at its start (x3_slots.extra_data). The page writes the image alone,
// so padding is dropped; anything else is not the image its digest describes.
export function extraData(data: Uint8Array): boolean {
  const length = imageLength(data);
  return length !== null && !data.subarray(length).every((byte) => byte === 0xff);
}

// Whether the app at the start of a slot is whole, as the bootloader checks it
// before starting it: header, segments, checksum and any appended SHA-256. Reads
// only the image's own bytes, starting from its already read `head`: one read
// per segment header and per segment, so the cable carries the image's size
// (3.3 MB for Newt 0.1.0-alpha.11, 6.3 MB for CrossInk v1.6.1).
export async function slotImageIntact(
  read: (offset: number, length: number) => Promise<Uint8Array>,
  slot: { readonly offset: number; readonly size: number },
  head: Uint8Array,
): Promise<boolean> {
  let data = head;
  for (;;) {
    const found = walk(data, slot.size);
    if (!found) return false;
    if ('end' in found && found.end <= data.length) return imageIntact(data.subarray(0, found.end), slot.size);
    const end = 'need' in found ? found.need : found.end;
    const more = await read(slot.offset + data.length, end - data.length);
    if (more.length !== end - data.length) return false;
    const joined = new Uint8Array(end);
    joined.set(data);
    joined.set(more, data.length);
    data = joined;
  }
}

// The largest image any known table's slot takes. The device's own slot size
// is checked again once its table is read.
export const SLOT_BYTES = Math.min(...X3_TABLE.filter((p) => p.type === 0).map((p) => p.size));

export interface Firmware {
  readonly name: string; // what the page calls it: "Newt 0.1.0-alpha.11", "CrossPoint 2.0.0"
  readonly image: Uint8Array; // exactly the app image, through its digest
  readonly identity: SlotImage;
  readonly version?: string; // Newt's manifest version and build
  readonly build?: string;
}

// The name a slot or file goes by: Newt by its own mark, another firmware by
// the slot rule's name table (CrossInk, CrossPoint). Stock Xteink firmware
// reports "arduino-lib-builder", so anything unnamed is not guessed at.
export function firmwareName(image: SlotImage): string {
  const versioned = (name: string, version: string) => (version ? `${name} ${version}` : name);
  switch (image.kind) {
    case 'newt':
      return versioned('Newt', image.newtVersion);
    case 'other':
      return image.name === UNKNOWN_FIRMWARE ? 'Another firmware' : versioned(image.name, image.version);
    case 'empty':
      return 'Empty';
    case 'invalid':
      return 'Unfinished or damaged data';
  }
}

export type FileProblem = 'not-firmware' | 'damaged' | 'too-large' | 'extra-data';
export type Checked = { readonly ok: true; readonly firmware: Firmware } | { readonly ok: false; readonly problem: FileProblem };

// A chosen .bin: one ESP32-C3 app image (with an app descriptor, so never a
// bootloader or a merged flash image), intact, and small enough for a slot.
// Only erased padding may follow it; the page writes the image alone.
export async function checkFile(data: Uint8Array): Promise<Checked> {
  const length = imageLength(data);
  if (length === null || data.length < HEAD_BYTES) return { ok: false, problem: 'not-firmware' };
  const identity = classify(data.subarray(0, HEAD_BYTES));
  if (identity.kind === 'empty' || identity.kind === 'invalid') return { ok: false, problem: 'not-firmware' };
  if (length > SLOT_BYTES) return { ok: false, problem: 'too-large' };
  if (extraData(data)) return { ok: false, problem: 'extra-data' };
  const image = data.subarray(0, length);
  if (!(await imageIntact(image, length))) return { ok: false, problem: 'damaged' };
  return { ok: true, firmware: { name: firmwareName(identity), image, identity } };
}

export interface NewtManifest {
  readonly version: string;
  readonly build: string;
  readonly size: number;
  readonly sha256: string;
}

// newt.manifest exactly as tools/update_publish.py writes it.
export function parseManifest(text: string): NewtManifest | null {
  const match =
    /^format=newt-update-1\nkind=newt\nversion=([0-9A-Za-z.+-]{1,23})\nbuild=([0-9a-f]{12})\nsize=([1-9][0-9]{0,8})\nsha256=([0-9a-f]{64})\n$/.exec(
      text,
    );
  if (!match) return null;
  const [, version, build, size, sha256] = match;
  return { version: version!, build: build!, size: Number(size), sha256: sha256! };
}

// Newt's release pair: the image must be the manifest's bytes, an intact Newt
// image whose identity block names the manifest's version and build.
export async function checkNewt(manifestText: string, data: Uint8Array): Promise<Firmware | null> {
  const manifest = parseManifest(manifestText);
  if (!manifest || data.length !== manifest.size || data.length > SLOT_BYTES) return null;
  if ((await sha256Hex(data)) !== manifest.sha256) return null;
  if (imageLength(data) !== data.length || !(await imageIntact(data, data.length))) return null;
  const identity = classify(data.subarray(0, HEAD_BYTES));
  if (identity.kind !== 'newt' || identity.newtVersion !== manifest.version || identity.newtBuild !== manifest.build)
    return null;
  return { name: firmwareName(identity), image: data, identity, version: manifest.version, build: manifest.build };
}
