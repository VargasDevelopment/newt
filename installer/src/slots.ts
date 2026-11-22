// The X3 app-slot rule, ported from tools/x3_slots.py and
// rust/core/src/slot_installer/identity.rs (ota_select). tests/installer_test.py
// exports the Python rule's answers on tests/x3_slots_test.py's vectors, and
// test/slots.test.ts holds this copy to them.
//
// Selection follows the ESP-IDF bootloader: a record counts when its sequence
// is set, its CRC matches and its state is not invalid or aborted; the highest
// sequence boots slot (seq - 1) % 2, unless it is still pending verification,
// which the rollback bootloader aborts in favor of the other record.
//
// The page differs from Newt's own tools in one way: it installs beside ANY
// running firmware, so it always writes the slot that does not boot, and it
// handles blank otadata (a unit whose first slot has never been switched).

export const SECTOR = 4096;
export const OTADATA_BYTES = 2 * SECTOR;
export const HEAD_BYTES = 0x200; // image header, first segment header, app descriptor, Newt mark

const APP_MAGIC = 0xe9;
const DESC_AT = 0x20;
const DESC_MAGIC = 0xabcd5432;
const NEWT_MARK_AT = DESC_AT + 0x100;
const NEWT_MARK = Uint8Array.from([...'NEWT-X3-APP\0'].map((c) => c.charCodeAt(0)).concat([1, 0, 0, 0]));
const NEWT_IDENTITY_AT = NEWT_MARK_AT + NEWT_MARK.length;
const NEWT_VERSION_BYTES = 24;
const NEWT_BUILD_BYTES = 16;
// Another firmware's name as Newt's Settings shows it ("Restart into <name>"),
// by its descriptor's exact project: x3_slots.FIRMWARE_NAMES and
// kFirmwareTitles. Stock reports the same project Newt's older builds did.
const FIRMWARE_NAMES: ReadonlyMap<string, string> = new Map([
  ['CrossInk', 'CrossInk'],
  ['crosspoint-reader', 'CrossPoint'],
]);
export const UNKNOWN_FIRMWARE = 'the other firmware';

export const OtaState = { New: 0, PendingVerify: 1, Valid: 2, Invalid: 3, Aborted: 4, Undefined: 0xffffffff } as const;

export type RefusalReason =
  | 'unknown-layout' // not a partition table this page knows
  | 'no-selection' // no usable otadata record, and otadata is not blank
  | 'changing' // a firmware change is pending or unconfirmed
  | 'boot-slot-not-app' // the slot that boots does not hold an app
  | 'kept-damaged' // the app that boots fails its whole-image check: the bootloader may start the other
  | 'sequence-exhausted'; // no sequence above the current one selects the spare

// The device bytes do not establish a safe install; nothing is written.
export class Refused extends Error {
  constructor(
    readonly reason: RefusalReason,
    message: string,
  ) {
    super(message);
    this.name = 'Refused';
  }
}

export interface OtaRecord {
  readonly seq: number;
  readonly state: number;
  readonly crc: number;
  readonly usable: boolean;
}

// CRC-32 over the sequence's 4 little-endian bytes, as the bootloader
// computes it (zlib.crc32(seq, 0xffffffff) in x3_slots.py).
export function seqCrc(seq: number): number {
  let crc = 0;
  for (let byte = 0; byte < 4; ++byte) {
    crc ^= (seq >>> (8 * byte)) & 0xff;
    for (let bit = 0; bit < 8; ++bit) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return ~crc >>> 0;
}

function view(bytes: Uint8Array): DataView {
  return new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
}

export function readRecords(ota: Uint8Array): readonly [OtaRecord, OtaRecord] {
  if (ota.length !== OTADATA_BYTES) throw new Error('Incomplete OTA selection read');
  const record = (at: number): OtaRecord => {
    const data = view(ota);
    const seq = data.getUint32(at, true);
    const state = data.getUint32(at + 24, true);
    const crc = data.getUint32(at + 28, true);
    const usable =
      seq !== 0 && seq !== 0xffffffff && crc === seqCrc(seq) && state !== OtaState.Invalid && state !== OtaState.Aborted;
    return { seq, state, crc, usable };
  };
  return [record(0), record(SECTOR)];
}

// Index (0/1) of the record the next boot follows. A tie goes to the first
// record. One still pending verification was never confirmed by its app: the
// bootloader aborts it and follows the other record, which must be usable and
// settled; otherwise no safe choice exists.
export function bootRecord(records: readonly [OtaRecord, OtaRecord]): 0 | 1 {
  const [first, second] = records;
  if (!first.usable && !second.usable) throw new Refused('no-selection', 'No valid OTA selection record');
  const followed = !second.usable || (first.usable && first.seq >= second.seq) ? 0 : 1;
  if (records[followed].state !== OtaState.PendingVerify) return followed;
  const fallback = followed === 0 ? 1 : 0;
  const other = records[fallback];
  if (!other.usable || other.state === OtaState.PendingVerify)
    throw new Refused('changing', 'A pending OTA selection has no confirmed fallback');
  return fallback;
}

export function slotOfSeq(seq: number): 0 | 1 {
  return (seq - 1) % 2 === 0 ? 0 : 1;
}

export function bootSlot(records: readonly [OtaRecord, OtaRecord]): 0 | 1 {
  return slotOfSeq(records[bootRecord(records)].seq);
}

// One otadata record as ESP-IDF writes it, alone in its 4 KiB sector.
export function recordSector(seq: number, state: number = OtaState.New): Uint8Array {
  const sector = new Uint8Array(SECTOR).fill(0xff);
  const data = view(sector);
  data.setUint32(0, seq, true);
  data.setUint32(24, state, true);
  data.setUint32(28, seqCrc(seq), true);
  return sector;
}

// otaSelect: the record that makes the next boot select `slot`: the smallest
// higher sequence for it, written into the record the next boot does not
// follow (an older one, or a pending one the bootloader would abort), so the
// record the device falls back to is never touched. 'selected' when the next
// boot already selects `slot`. 0xffffffff marks an erased record, so a
// sequence that would reach it is refused rather than written.
export function selectionRecord(
  records: readonly [OtaRecord, OtaRecord],
  slot: 0 | 1,
): 'selected' | { readonly into: 0 | 1; readonly sector: Uint8Array } {
  const current = bootRecord(records);
  let seq = records[current].seq;
  if (slotOfSeq(seq) === slot) return 'selected';
  do ++seq;
  while (slotOfSeq(seq) !== slot);
  if (seq >= 0xffffffff) throw new Refused('sequence-exhausted', `No OTA sequence above ${records[current].seq} selects app${slot}`);
  return { into: current === 0 ? 1 : 0, sector: recordSector(seq) };
}

export type SlotKind = 'empty' | 'invalid' | 'other' | 'newt';

export interface SlotImage {
  readonly kind: SlotKind;
  readonly name: string; // another firmware's name for people; empty for any other kind
  readonly project: string;
  readonly version: string; // the ESP-IDF descriptor's version text
  readonly newtVersion: string; // Newt's identity block; empty when absent or unterminated
  readonly newtBuild: string;
}

function text(bytes: Uint8Array): string {
  const end = bytes.indexOf(0);
  // ASCII, as x3_slots.text decodes it ('replace' for anything else).
  return Array.from(end < 0 ? bytes : bytes.subarray(0, end), (byte) => (byte < 0x80 ? String.fromCharCode(byte) : '\ufffd')).join('');
}

// Newt by its fixed mark; any other valid app is another firmware.
export function classify(head: Uint8Array): SlotImage {
  if (head.length < HEAD_BYTES) throw new Error('Incomplete slot header read');
  const none = { name: '', project: '', version: '', newtVersion: '', newtBuild: '' };
  if (head.subarray(0, HEAD_BYTES).every((byte) => byte === 0xff)) return { kind: 'empty', ...none };
  if (head[0] !== APP_MAGIC || view(head).getUint32(DESC_AT, true) !== DESC_MAGIC) return { kind: 'invalid', ...none };
  const project = text(head.subarray(DESC_AT + 48, DESC_AT + 80));
  const version = text(head.subarray(DESC_AT + 16, DESC_AT + 48));
  const newt = NEWT_MARK.every((byte, i) => head[NEWT_MARK_AT + i] === byte);
  if (!newt) return { kind: 'other', ...none, name: FIRMWARE_NAMES.get(project) ?? UNKNOWN_FIRMWARE, project, version };
  // As readNewtIdentity: both fields must end in NUL. Older Newt images carry other bytes here.
  const versionField = head.subarray(NEWT_IDENTITY_AT, NEWT_IDENTITY_AT + NEWT_VERSION_BYTES);
  const buildField = head.subarray(NEWT_IDENTITY_AT + NEWT_VERSION_BYTES, NEWT_IDENTITY_AT + NEWT_VERSION_BYTES + NEWT_BUILD_BYTES);
  const identified = versionField.at(-1) === 0 && buildField.at(-1) === 0;
  return {
    kind: 'newt',
    name: '',
    project,
    version,
    newtVersion: identified ? text(versionField) : '',
    newtBuild: identified ? text(buildField) : '',
  };
}

export interface Slot {
  readonly offset: number;
  readonly size: number;
}

export interface SlotLayout {
  readonly otadata: number;
  readonly slots: readonly [Slot, Slot];
}

export interface Plan {
  readonly layout: SlotLayout;
  readonly ota: Uint8Array; // the otadata bytes this plan was made from
  readonly images: readonly [SlotImage, SlotImage];
  readonly running: 0 | 1; // the slot that boots now; it stays
  readonly target: 0 | 1; // the spare slot: the only app slot the page writes
  // The one otadata sector written after the image is verified.
  readonly selection: { readonly offset: number; readonly sector: Uint8Array };
}

// What the page writes on this device: the image into the slot that does not
// boot, then one record selecting it, in the sector the next boot does not
// follow. Refused when the device does not establish which slot boots.
//
// The slot that boots must hold an app, and its record must be the newest and
// confirmed (valid, or undefined without rollback), as x3_slots.running_newt
// requires: otherwise a change is still in progress and the "spare" slot could
// be the one the device is about to start.
//
// Blank otadata (both sectors erased) boots app0, the first OTA slot, as the
// bootloader does without a factory app. The first record ESP-IDF writes then
// goes into sector 0 with sequence slot + 1; a failed first boot aborts it and
// the bootloader returns to app0.
export function plan(layout: SlotLayout, ota: Uint8Array, heads: readonly [Uint8Array, Uint8Array]): Plan {
  const images = [classify(heads[0]), classify(heads[1])] as const;
  const blank = ota.length === OTADATA_BYTES && ota.every((byte) => byte === 0xff);
  let running: 0 | 1;
  let selection: Plan['selection'];
  if (blank) {
    running = 0;
    selection = { offset: layout.otadata, sector: recordSector(2) };
  } else {
    const records = readRecords(ota);
    const followed = bootRecord(records);
    const current = records[followed];
    running = slotOfSeq(current.seq);
    const newest = Math.max(...records.filter((record) => record.usable).map((record) => record.seq));
    if (current.seq !== newest) throw new Refused('changing', 'A newer selection is still pending');
    if (current.state !== OtaState.Valid && current.state !== OtaState.Undefined)
      throw new Refused('changing', `The selection of app${running} is not confirmed`);
    const record = selectionRecord(records, running === 0 ? 1 : 0);
    if (record === 'selected') throw new Error('unreachable: the running slot is selected');
    selection = { offset: layout.otadata + record.into * SECTOR, sector: record.sector };
  }
  const kind = images[running].kind;
  if (kind === 'empty' || kind === 'invalid')
    throw new Refused('boot-slot-not-app', `app${running}, the slot that boots, holds no readable app`);
  return { layout, ota, images, running, target: running === 0 ? 1 : 0, selection };
}

// The otadata bytes after the plan's one sector write.
export function applied(plan: Plan): Uint8Array {
  const after = new Uint8Array(plan.ota);
  after.set(plan.selection.sector, plan.selection.offset - plan.layout.otadata);
  return after;
}

// The only writes the page makes: the spare slot, from its start, within its
// size; and the plan's one otadata sector. Everything else throws, whatever
// the caller computed.
export function guardWrite(plan: Plan, offset: number, length: number): void {
  const target = plan.layout.slots[plan.target];
  const app = offset === target.offset && length > 0 && length <= target.size;
  const selection = offset === plan.selection.offset && length === SECTOR;
  if (!app && !selection) throw new Error(`Refusing to write ${length} bytes at 0x${offset.toString(16)}`);
}
