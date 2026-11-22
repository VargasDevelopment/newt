import { describe, expect, it } from 'vitest';
import { md5Hex } from '../src/digest.ts';
import { appImage, CROSSINK, CROSSPOINT, NEWT, otadata, STOCK, X3_LAYOUT, type AppSpec } from '../src/fixtures.ts';
import { checkFile, type Firmware, firmwareName } from '../src/image.ts';
import { TABLE_OFFSET, tableBytes, X3_TABLE } from '../src/partitions.ts';
import { bootSlot, classify, HEAD_BYTES, OtaState, readRecords, SECTOR } from '../src/slots.ts';
import { CONFIRM_SECONDS, type Problem, type Screen, Session } from '../src/session.ts';
import { FakeX3, x3 } from './fake-device.ts';

const { Invalid, PendingVerify, Valid } = OtaState;
const isOtadata = (offset: number) => offset >= X3_LAYOUT.otadata && offset < X3_LAYOUT.otadata + 2 * SECTOR;
const NEXT_NEWT: AppSpec = { ...NEWT, version: 'abc1234', newt: { version: '0.1.0-alpha.12', build: 'abc1234abc12' } };

async function newtFirmware(spec: AppSpec = NEXT_NEWT): Promise<Firmware> {
  const image = await appImage({ ...spec, extra: 9000 }); // three sectors
  const identity = classify(image.subarray(0, HEAD_BYTES));
  return { name: firmwareName(identity), image, identity, version: spec.newt!.version, build: spec.newt!.build };
}

interface Harness {
  session: Session<'port'>;
  screens: Screen[];
  waits: number[];
  opened: number;
}

function harness(device: FakeX3, newt: Promise<Firmware | null>, options: { port?: 'port' | null; openFails?: boolean } = {}): Harness {
  const result: Harness = { session: undefined!, screens: [], waits: [], opened: 0 };
  result.session = new Session<'port'>(
    {
      requestPort: async () => (options.port === undefined ? 'port' : options.port),
      open: async () => {
        result.opened++;
        if (options.openFails) throw new Error('busy');
        return device;
      },
      newt,
      wait: async (milliseconds) => void result.waits.push(milliseconds),
    },
    true,
  );
  result.session.subscribe((screen) => result.screens.push(screen));
  return result;
}

// Everything outside the spare slot's image and the one selection sector.
function untouched(before: Uint8Array, after: Uint8Array, firmware: Firmware, target: 0 | 1, sector: number): boolean {
  const slot = X3_LAYOUT.slots[target].offset;
  const changed = (at: number) => (at >= slot && at < slot + Math.ceil(firmware.image.length / SECTOR) * SECTOR) || (at >= sector && at < sector + SECTOR);
  for (let at = 0; at < before.length; at += SECTOR)
    if (!changed(at) && !before.subarray(at, at + SECTOR).every((byte, i) => byte === after[at + i])) return false;
  return true;
}

async function toConsent(h: Harness, start: () => Promise<void> = () => h.session.installNewt()): Promise<void> {
  await start();
  expect(h.session.screen.kind).toBe('summary');
  h.session.next();
  expect(h.session.screen.kind).toBe('consent');
}

function problem(h: Harness): Problem | null {
  const screen = h.session.screen;
  return screen.kind === 'error' ? screen.problem : null;
}

describe('installing', () => {
  const starts: [string, [AppSpec | null, AppSpec | null], Parameters<typeof x3>[1], 0 | 1, 0 | 1][] = [
    ['CrossPoint in app0, empty spare', [CROSSPOINT, null], [[7, Valid], [6, Valid]], 0, 1],
    ['CrossInk in app1, an older Newt spare', [NEWT, CROSSINK], [[5, Valid], [6, Valid]], 1, 0],
    ['stock, never switched (blank otadata)', [STOCK, null], [null, null], 0, 1],
    ['Newt in app0, CrossInk spare', [NEWT, CROSSINK], [[3, Valid], null], 0, 1],
  ];

  it.each(starts)('from %s: the spare, its MD5, one selection sector, one reset', async (_name, slots, records, running, target) => {
    const device = await x3(slots, records);
    const before = device.flash.slice();
    const firmware = await newtFirmware();
    const h = harness(device, Promise.resolve(firmware));
    await toConsent(h);
    const consent = h.session.screen;
    if (consent.kind !== 'consent') throw new Error('not at consent');
    expect([consent.survey.plan.running, consent.survey.plan.target]).toEqual([running, target]);
    const sector = consent.survey.plan.selection.offset;

    await h.session.install();

    expect(h.session.screen).toMatchObject({ kind: 'done', restarted: true });
    expect(device.writes).toEqual([
      [X3_LAYOUT.slots[target].offset, firmware.image.length],
      [sector, SECTOR],
    ]);
    expect(device.events).toEqual([`write 0x${X3_LAYOUT.slots[target].offset.toString(16)}`, `write 0x${sector.toString(16)}`, 'reset', 'close']);
    expect(md5Hex(device.flash.subarray(X3_LAYOUT.slots[target].offset, X3_LAYOUT.slots[target].offset + firmware.image.length))).toBe(md5Hex(firmware.image));
    expect(bootSlot(readRecords(device.ota()))).toBe(target);
    expect(untouched(before, device.flash, firmware, target, sector)).toBe(true);
    // The port stays open through the confirmation window, counted down to the user.
    expect(h.waits).toEqual(new Array(CONFIRM_SECONDS).fill(1000));
    expect(h.screens.filter((s) => s.kind === 'starting').map((s) => s.kind === 'starting' && s.secondsLeft)).toEqual(
      Array.from({ length: CONFIRM_SECONDS }, (_, i) => CONFIRM_SECONDS - i),
    );
    expect(h.screens.map((s) => s.kind).filter((kind, i, all) => kind !== all[i - 1])).toEqual([
      'connecting',
      'checking',
      'summary',
      'consent',
      'writing',
      'starting',
      'done',
    ]);
  });

  it('opens the port list before waiting for anything', async () => {
    let requested = false;
    let release: (firmware: Firmware | null) => void = () => {};
    const session = new Session<'port'>(
      {
        requestPort: async () => {
          requested = true;
          return 'port';
        },
        open: async () => await x3([CROSSPOINT, null], [[7, Valid], null]),
        newt: new Promise((resolve) => (release = resolve)),
        wait: async () => {},
      },
      true,
    );
    const running = session.installNewt();
    expect(requested).toBe(true); // still inside the click
    release(await newtFirmware());
    await running;
    expect(session.screen.kind).toBe('summary');
  });

  it('tries the port list again after no X3 was picked, for the same firmware', async () => {
    const device = await x3([CROSSPOINT, null], [[7, Valid], null]);
    const ports: ('port' | null)[] = [null, 'port'];
    const session = new Session<'port'>(
      { requestPort: async () => ports.shift() ?? null, open: async () => device, newt: newtFirmware(), wait: async () => {} },
      true,
    );
    await session.installNewt();
    expect(session.screen).toMatchObject({ kind: 'error', problem: { code: 'not-found' } });
    await session.retry();
    expect(session.screen).toMatchObject({ kind: 'summary', survey: { firmware: { name: 'Newt 0.1.0-alpha.12' } } });
    await session.retry(); // only from those two errors
    expect(session.screen.kind).toBe('summary');
  });

  it('installs a chosen firmware the same way (N2)', async () => {
    const device = await x3([NEWT, CROSSINK], [[5, Valid], [6, Valid]]);
    const h = harness(device, Promise.resolve(null));
    h.session.chooseOther();
    const checked = await checkFile(await appImage(CROSSPOINT));
    h.session.chosen(checked);
    expect(h.session.screen).toMatchObject({ kind: 'choose', firmware: { name: 'CrossPoint 2.0.0' } });
    await toConsent(h, () => h.session.installChosen());
    await h.session.install();
    expect(h.session.screen.kind).toBe('done');
    expect(device.writes.map(([offset]) => offset)).toEqual([X3_LAYOUT.slots[0].offset, X3_LAYOUT.otadata]);
    expect(bootSlot(readRecords(device.ota()))).toBe(0);
  });

  it('refuses a bad chosen file before connecting', async () => {
    const h = harness(new FakeX3(), Promise.resolve(null));
    h.session.chooseOther();
    h.session.chosen({ ok: false, problem: 'damaged' });
    expect(problem(h)).toEqual({ code: 'bad-file', problem: 'damaged' });
    expect(h.opened).toBe(0);
  });

  it('writes nothing when Newt already runs, exactly this build', async () => {
    const device = await x3([NEXT_NEWT, CROSSINK], [[3, Valid], null]);
    const h = harness(device, newtFirmware());
    await h.session.installNewt();
    expect(h.session.screen.kind).toBe('already');
    expect(device.writes).toEqual([]);
    expect(device.events).toEqual(['reset', 'close']);
  });

  it('cancels at consent without writing', async () => {
    const device = await x3([CROSSPOINT, null], [[7, Valid], null]);
    const h = harness(device, newtFirmware());
    await toConsent(h);
    await h.session.cancel();
    expect(h.session.screen.kind).toBe('start');
    expect(device.writes).toEqual([]);
    expect(device.events).toEqual(['reset', 'close']);
  });

  it('ignores a second Install while one runs', async () => {
    const device = await x3([CROSSPOINT, null], [[7, Valid], null]);
    const h = harness(device, newtFirmware());
    await toConsent(h);
    await Promise.all([h.session.install(), h.session.install()]);
    expect(device.writes).toHaveLength(2);
  });
});

describe('refusing before any write', () => {
  async function refused(prepare: (device: FakeX3) => void, options: Parameters<typeof harness>[2] = {}, newt: Promise<Firmware | null> = newtFirmware()) {
    const device = await x3([CROSSPOINT, null], [[7, Valid], null]);
    prepare(device);
    const h = harness(device, newt, options);
    await h.session.installNewt();
    expect(device.writes).toEqual([]);
    return { problem: problem(h), device, h };
  }

  it('no X3 picked', async () => expect((await refused(() => {}, { port: null })).problem).toEqual({ code: 'not-found' }));

  it("the port won't open", async () => expect((await refused(() => {}, { openFails: true })).problem).toEqual({ code: 'no-connection' }));

  it("the page's newt.bin failed its manifest: the port is never opened", async () => {
    const { problem: found, h } = await refused(() => {}, {}, Promise.resolve(null));
    expect(found).toEqual({ code: 'newt-file' });
    expect(h.opened).toBe(0);
  });

  it.each([
    ['another chip', (d: FakeX3) => void (d.chip = 'ESP32')],
    ['other flash', (d: FakeX3) => void (d.flashBytes = 4 << 20)],
  ])('not an X3: %s', async (_name, prepare) => expect((await refused(prepare)).problem).toEqual({ code: 'not-x3' }));

  it('an unknown partition table', async () => {
    const { problem: found, device } = await refused((d) => {
      const table = X3_TABLE.map((p) => (p.label === 'app1' ? { ...p, offset: 0x790000 } : p));
      d.flash.set(tableBytes(table), TABLE_OFFSET);
    });
    expect(found).toEqual({ code: 'refused', reason: 'unknown-layout' });
    expect(device.events).toEqual(['reset', 'close']);
  });

  it('a change in progress', async () => {
    const { problem: found } = await refused((d) => d.flash.set(otadata([7, Valid], [8, PendingVerify]), X3_LAYOUT.otadata));
    expect(found).toEqual({ code: 'refused', reason: 'changing' });
  });

  it('records that select nothing', async () => {
    const { problem: found } = await refused((d) => d.flash.set(otadata([7, Invalid], null), X3_LAYOUT.otadata));
    expect(found).toEqual({ code: 'refused', reason: 'no-selection' });
  });

  it('a read that fails', async () => {
    const { problem: found } = await refused((d) => (d.faults.read = () => true));
    expect(found).toEqual({ code: 'no-connection' });
  });

  it('a start sequence with no number left for the spare', async () => {
    // app1 runs at 0xfffffffe; only the reserved 0xffffffff would select app0.
    const device = await x3([CROSSINK, CROSSPOINT], [[0xfffffffe, Valid], [0xfffffffd, Valid]]);
    const h = harness(device, newtFirmware());
    await h.session.installNewt();
    expect(problem(h)).toEqual({ code: 'refused', reason: 'sequence-exhausted' });
    expect(device.writes).toEqual([]);
  });
});

// otadata 7/6 selects app0. When app0 fails the bootloader's whole-image
// check, the bootloader starts app1 instead: app1 is no spare.
describe('the firmware that stays is checked whole', () => {
  const flip = (bytes: Uint8Array, at: number) => void (bytes[at] = bytes[at]! ^ 1);
  const KEPT = { ...CROSSPOINT, extra: 20000 };
  const damages: [string, (image: Uint8Array) => void][] = [
    ['a segment byte', (image) => flip(image, 0x1000)],
    ['its digest', (image) => flip(image, image.length - 1)],
    ['an erased tail', (image) => void image.fill(0xff, 0x2000)],
  ];

  it.each(damages)('refuses when the running image fails on %s, writing nothing', async (_name, damage) => {
    const device = await x3([KEPT, CROSSINK], [[7, Valid], [6, Valid]]);
    damage(device.flash.subarray(X3_LAYOUT.slots[0].offset, X3_LAYOUT.slots[0].offset + (await appImage(KEPT)).length));
    const h = harness(device, newtFirmware());
    await h.session.installNewt();
    expect(problem(h)).toEqual({ code: 'refused', reason: 'kept-damaged' });
    expect(device.writes).toEqual([]);
    expect(device.events).toEqual(['reset', 'close']);
  });

  it('reads the kept image once, and nothing of the spare past its head', async () => {
    const device = await x3([KEPT, CROSSINK], [[7, Valid], [6, Valid]]);
    const length = (await appImage(KEPT)).length;
    const read: [number, number][] = [];
    const original = device.read.bind(device);
    device.read = (offset, count) => (read.push([offset, count]), original(offset, count));
    const h = harness(device, newtFirmware());
    await h.session.installNewt();
    expect(h.session.screen.kind).toBe('summary');
    const inSlot = (slot: 0 | 1) => read.filter(([offset]) => offset >= X3_LAYOUT.slots[slot].offset && offset < X3_LAYOUT.slots[slot].offset + X3_LAYOUT.slots[slot].size);
    expect(inSlot(0).reduce((sum, [, count]) => sum + count, 0)).toBe(length);
    expect(inSlot(1)).toEqual([[X3_LAYOUT.slots[1].offset, HEAD_BYTES]]);
  });

  it('installs over a damaged spare', async () => {
    const device = await x3([KEPT, { ...CROSSINK, extra: 20000 }], [[7, Valid], [6, Valid]]);
    flip(device.flash, X3_LAYOUT.slots[1].offset + 0x1000);
    const h = harness(device, newtFirmware());
    await toConsent(h);
    await h.session.install();
    expect(h.session.screen.kind).toBe('done');
  });
});

describe('failures while installing', () => {
  async function failed(faults: FakeX3['faults']) {
    const device = await x3([CROSSPOINT, null], [[7, Valid], [6, Valid]]);
    const otaBefore = device.ota();
    const h = harness(device, newtFirmware());
    await toConsent(h);
    device.faults = faults;
    await h.session.install();
    return { device, h, otaBefore, found: problem(h) };
  }

  const slot1 = X3_LAYOUT.slots[1].offset;

  it('a write that stops: no selection, the old firmware still starts', async () => {
    const { device, found, otaBefore } = await failed({ write: (offset) => offset === slot1, tearAt: 100 });
    expect(found?.code).toBe('write-stopped');
    expect(device.writes.map(([offset]) => offset)).toEqual([slot1]);
    expect(device.ota()).toEqual(otaBefore);
    expect(device.events.slice(-2)).toEqual(['reset', 'close']);
  });

  it.each(['wrong', 'throw'] as const)('an MD5 that is %s: never selected', async (md5) => {
    const { device, found, otaBefore } = await failed({ md5 });
    expect(found?.code).toBe('check-failed');
    expect(device.writes.map(([offset]) => offset)).toEqual([slot1]);
    expect(device.ota()).toEqual(otaBefore);
  });

  it('a selection write that fails and did not land: unchanged, said so', async () => {
    const { found, device, otaBefore } = await failed({ write: isOtadata });
    expect(found?.code).toBe('not-selected');
    expect(device.writes).toHaveLength(2);
    expect(device.ota()).toEqual(otaBefore);
    expect(bootSlot(readRecords(device.ota()))).toBe(0);
  });

  it('a selection write that reports failure but landed: done', async () => {
    const { h, device } = await failed({ write: isOtadata, selectionLands: true });
    expect(device.writes).toHaveLength(2);
    expect(h.session.screen.kind).toBe('done');
    expect(bootSlot(readRecords(device.ota()))).toBe(1);
  });

  it('a selection that cannot be read back: unconfirmed, never "unsaved", and the port held', async () => {
    let reads = 0;
    const { found, h, device } = await failed({ read: (offset) => offset === X3_LAYOUT.otadata && ++reads > 0 });
    expect(found?.code).toBe('unconfirmed');
    expect(h.waits).toEqual([CONFIRM_SECONDS * 1000]);
    expect(device.events.slice(-2)).toEqual(['reset', 'close']);
    expect(reads).toBe(2); // bounded readback
  });

  it('a readback that shows neither the old nor the new records: unconfirmed', async () => {
    const device = await x3([CROSSPOINT, null], [[7, Valid], [6, Valid]]);
    const h = harness(device, newtFirmware());
    await toConsent(h);
    const write = device.write.bind(device);
    device.write = async (offset, data, progress) => {
      await write(offset, data, progress);
      if (offset === X3_LAYOUT.otadata + SECTOR) device.flash[offset + 4] = 0; // a torn record
    };
    await h.session.install();
    expect(problem(h)?.code).toBe('unconfirmed');
  });

  it('a reset that fails still finishes, and says to restart', async () => {
    const device = await x3([CROSSPOINT, null], [[7, Valid], null]);
    const h = harness(device, newtFirmware());
    await toConsent(h);
    device.reset = async () => {
      throw new Error('gone');
    };
    await h.session.install();
    expect(h.session.screen).toMatchObject({ kind: 'done', restarted: false });
  });
});

describe('without Web Serial', () => {
  it('starts on the unsupported screen and stays there', async () => {
    const session = new Session<'port'>({ requestPort: async () => 'port', open: async () => new FakeX3(), newt: Promise.resolve(null), wait: async () => {} }, false);
    expect(session.screen.kind).toBe('unsupported');
    await session.installNewt();
    session.chooseOther();
    expect(session.screen.kind).toBe('unsupported');
  });
});
