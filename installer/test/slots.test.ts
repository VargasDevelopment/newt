import { describe, expect, it } from 'vitest';
import { fromHex, toHex } from '../src/digest.ts';
import { appImage, CROSSINK, CROSSPOINT, NEWT, otadata, STOCK, X3_LAYOUT } from '../src/fixtures.ts';
import {
  applied,
  bootRecord,
  bootSlot,
  classify,
  guardWrite,
  HEAD_BYTES,
  OTADATA_BYTES,
  OtaState,
  plan,
  readRecords,
  Refused,
  SECTOR,
  selectionRecord,
  slotOfSeq,
} from '../src/slots.ts';
import vectors from './x3-slot-vectors.json';

const { New, PendingVerify, Valid, Invalid, Aborted, Undefined } = OtaState;

// One sector per record, as tests/installer_test.py exported them.
function selectionBytes(records: readonly string[]): Uint8Array {
  const ota = new Uint8Array(OTADATA_BYTES).fill(0xff);
  records.forEach((record, index) => ota.set(fromHex(record), index * SECTOR));
  return ota;
}

function refusal(run: () => unknown): string | null {
  try {
    run();
    return null;
  } catch (error) {
    if (error instanceof Refused) return error.reason;
    throw error;
  }
}

describe("tools/x3_slots.py's vectors", () => {
  it.each(vectors.heads)('classifies $name as $kind', ({ hex, kind, project, shown }) => {
    const image = classify(fromHex(hex));
    expect(image.kind).toBe(kind);
    expect(image.name).toBe(shown);
    if (kind !== 'empty' && kind !== 'invalid') expect(image.project).toBe(project);
  });

  it.each(vectors.selections)('follows the same record and writes the same selection for $name', ({ records, boot, select }) => {
    const parsed = readRecords(selectionBytes(records));
    if (boot === null) {
      expect(refusal(() => bootRecord(parsed))).not.toBeNull();
    } else {
      expect(bootRecord(parsed)).toBe(boot.record);
      expect(bootSlot(parsed)).toBe(boot.slot);
    }
    for (const slot of [0, 1] as const) {
      const expected = select[slot];
      let actual: string | { into: number; record: string };
      try {
        const record = selectionRecord(parsed, slot);
        actual = record === 'selected' ? record : { into: record.into, record: toHex(record.sector.subarray(0, 32)) };
        if (record !== 'selected') expect(record.sector.subarray(32).every((byte) => byte === 0xff)).toBe(true);
      } catch (error) {
        if (!(error instanceof Refused)) throw error;
        actual = error.reason === 'sequence-exhausted' ? 'exhausted' : 'unknown';
      }
      expect(actual).toEqual(expected);
    }
  });
});

async function heads(...specs: (Parameters<typeof appImage>[0] | null)[]): Promise<[Uint8Array, Uint8Array]> {
  const head = async (spec: Parameters<typeof appImage>[0] | null) =>
    spec ? (await appImage(spec)).slice(0, HEAD_BYTES) : new Uint8Array(HEAD_BYTES).fill(0xff);
  return [await head(specs[0] ?? null), await head(specs[1] ?? null)];
}

// Records whose newest, confirmed sequence boots `slot`.
const confirmed = (slot: 0 | 1) => (slot === 1 ? otadata([5, Valid], [6, Valid]) : otadata([7, Valid], [6, Valid]));

describe('the install plan', () => {
  it.each([0, 1] as const)('writes only the spare beside any firmware running from app%i', async (running) => {
    for (const spec of [CROSSPOINT, CROSSINK, STOCK, NEWT]) {
      const pair: [typeof spec | null, typeof spec | null] = [null, null];
      pair[running] = spec;
      const ota = confirmed(running);
      const found = plan(X3_LAYOUT, ota, await heads(...pair));
      expect([found.running, found.target]).toEqual([running, 1 - running]);
      // Both records are set: the one written is the one the next boot does not follow.
      const followed = bootRecord(readRecords(ota));
      expect(found.selection.offset).toBe(X3_LAYOUT.otadata + (1 - followed) * SECTOR);
      const after = applied(found);
      expect(bootSlot(readRecords(after))).toBe(found.target);
      expect(readRecords(after)[followed]).toEqual(readRecords(ota)[followed]);
      expect(readRecords(after)[1 - followed]!.state).toBe(New);
    }
  });

  it('selects with the smallest higher sequence when only one record is set', async () => {
    const found = plan(X3_LAYOUT, otadata(null, [4, Valid]), await heads(null, CROSSPOINT));
    expect([found.running, found.target, found.selection.offset]).toEqual([1, 0, X3_LAYOUT.otadata]);
    expect(readRecords(applied(found))[0]!.seq).toBe(5);
  });

  it('on blank otadata, app0 runs and seq 2 in sector 0 selects app1', async () => {
    const blank = new Uint8Array(OTADATA_BYTES).fill(0xff);
    const found = plan(X3_LAYOUT, blank, await heads(STOCK, null));
    expect([found.running, found.target, found.selection.offset]).toEqual([0, 1, X3_LAYOUT.otadata]);
    const records = readRecords(applied(found));
    // seq 1 would boot app0 again (the xteink-flasher bug the research names).
    expect([records[0].seq, slotOfSeq(records[0].seq), records[0].state, bootSlot(records)]).toEqual([2, 1, New, 1]);
    expect(records[1].usable).toBe(false);
    // A first boot that never confirms is aborted: no record counts, and the bootloader starts app0.
    const aborted = new Uint8Array(applied(found));
    new DataView(aborted.buffer).setUint32(24, Aborted, true);
    expect(refusal(() => bootRecord(readRecords(aborted)))).toBe('no-selection');
  });

  it('refuses blank otadata when app0 holds no app', async () => {
    const blank = new Uint8Array(OTADATA_BYTES).fill(0xff);
    expect(refusal(() => plan(X3_LAYOUT, blank, [new Uint8Array(HEAD_BYTES).fill(0xff), new Uint8Array(HEAD_BYTES)]))).toBe('boot-slot-not-app');
  });

  it('refuses otadata that is neither blank nor selects a slot', async () => {
    const pair = await heads(CROSSPOINT, CROSSINK);
    const junk = new Uint8Array(OTADATA_BYTES).fill(0xff);
    junk[100] = 0;
    for (const ota of [junk, otadata([5, Invalid], [6, Aborted]), otadata([0, Valid], null)])
      expect(refusal(() => plan(X3_LAYOUT, ota, pair))).toBe('no-selection');
  });

  it('refuses while a change is pending or unconfirmed, and accepts undefined (no rollback)', async () => {
    const pair = await heads(CROSSPOINT, CROSSINK);
    for (const ota of [
      otadata([5, Valid], [6, PendingVerify]), // a newer selection still on its first boot
      otadata([5, New], null), // selected, never booted
      otadata([5, Valid], [6, New]),
      otadata(null, [8, PendingVerify]), // no confirmed fallback
      otadata([7, PendingVerify], [8, PendingVerify]),
    ])
      expect(refusal(() => plan(X3_LAYOUT, ota, pair))).toBe('changing');
    expect(plan(X3_LAYOUT, otadata([5, Undefined], null), pair).running).toBe(0);
    // A pending record behind the followed one is older: allowed, and it is the one rewritten.
    const found = plan(X3_LAYOUT, otadata([7, Valid], [6, PendingVerify]), pair);
    expect([found.running, found.selection.offset]).toEqual([0, X3_LAYOUT.otadata + SECTOR]);
  });

  it('refuses when the slot that boots holds no readable app', async () => {
    const [crossPoint] = await heads(CROSSPOINT);
    const invalid = new Uint8Array(HEAD_BYTES);
    expect(refusal(() => plan(X3_LAYOUT, confirmed(1), [crossPoint, new Uint8Array(HEAD_BYTES).fill(0xff)]))).toBe('boot-slot-not-app');
    expect(refusal(() => plan(X3_LAYOUT, confirmed(1), [crossPoint, invalid]))).toBe('boot-slot-not-app');
  });

  it("on every shared selection and slot pair, never writes the running slot or the followed record", () => {
    const pairs = vectors.heads.flatMap((a) => vectors.heads.map((b) => [fromHex(a.hex), fromHex(b.hex)] as const));
    for (const selection of vectors.selections) {
      const ota = selectionBytes(selection.records);
      for (const pair of pairs) {
        let found;
        try {
          found = plan(X3_LAYOUT, ota, pair);
        } catch (error) {
          if (error instanceof Refused) continue;
          throw error;
        }
        // Blank otadata ('none') has no record to follow: sector 0 takes the first one.
        const blank = selection.name === 'none';
        const followed = blank ? 1 : bootRecord(readRecords(ota));
        expect(found.target).not.toBe(found.running);
        expect(found.running).toBe(blank ? 0 : bootSlot(readRecords(ota)));
        expect(found.selection.offset).toBe(X3_LAYOUT.otadata + (1 - followed) * SECTOR);
        expect(bootSlot(readRecords(applied(found)))).toBe(found.target);
        expect(['newt', 'other']).toContain(found.images[found.running].kind);
      }
    }
  });
});

describe('sequence exhaustion', () => {
  const sectors = (first: number, second: number) => otadata([first, Valid], [second, Valid]);

  // 0xffffffff marks an erased record: the bootloader ignores it, so the
  // device would keep starting the old slot after a "selected" readback.
  it.each([
    [0xfffffffe, 0xfffffffd], // app1 runs; app0 would need 0xffffffff
    [0xfffffffd, 0xfffffffe],
  ])('refuses when only the reserved sequence selects the spare (%i, %i)', async (first, second) => {
    const pair = await heads(CROSSPOINT, CROSSINK);
    expect(refusal(() => plan(X3_LAYOUT, sectors(first, second), pair))).toBe('sequence-exhausted');
  });

  it('still selects with the last usable sequence', async () => {
    const found = plan(X3_LAYOUT, sectors(0xfffffffd, 0xfffffffc), await heads(CROSSPOINT, CROSSINK));
    expect(found.running).toBe(0);
    expect(readRecords(applied(found)).map((record) => record.seq)).toEqual([0xfffffffd, 0xfffffffe]);
    expect(bootSlot(readRecords(applied(found)))).toBe(1);
  });
});

describe('the write guard', () => {
  it('allows the spare slot and the one selection sector, nothing else', async () => {
    const found = plan(X3_LAYOUT, confirmed(0), await heads(CROSSPOINT, null));
    const spare = X3_LAYOUT.slots[1];
    guardWrite(found, spare.offset, 1);
    guardWrite(found, spare.offset, spare.size);
    guardWrite(found, found.selection.offset, SECTOR);
    for (const [offset, length] of [
      [X3_LAYOUT.slots[0].offset, 4096], // the running firmware
      [0, 4096], // bootloader
      [0x8000, 4096], // partition table
      [0x9000, 4096], // NVS
      [found.selection.offset === X3_LAYOUT.otadata ? X3_LAYOUT.otadata + SECTOR : X3_LAYOUT.otadata, SECTOR], // the followed record
      [spare.offset, spare.size + 1],
      [spare.offset, 0],
      [spare.offset + SECTOR, 4096],
      [found.selection.offset, 2 * SECTOR],
    ] as const)
      expect(() => guardWrite(found, offset, length), `0x${offset.toString(16)}+${length}`).toThrow();
  });
});
