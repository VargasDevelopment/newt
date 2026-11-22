// The install page's state machine, without DOM or Web Serial: it reads the
// device, asks for consent, writes the spare slot, verifies it, selects it and
// restarts. The browser wiring (main.ts) and the tests both drive it.
import { md5Hex } from './digest.ts';
import { type FileProblem, type Firmware, slotImageIntact } from './image.ts';
import { knownLayout, TABLE_BYTES, TABLE_OFFSET } from './partitions.ts';
import { applied, guardWrite, HEAD_BYTES, OTADATA_BYTES, plan, type Plan, Refused, type RefusalReason } from './slots.ts';

export const FLASH_BYTES = 16 * 1024 * 1024;
// Newt confirms its new slot at its first finished screen. Closing the port
// earlier can reset the X3 before that, and the bootloader then returns to the
// old firmware (tools/bench_device.py CONFIRM_SECONDS).
export const CONFIRM_SECONDS = 30;

// One open connection to an X3 in its ROM download mode (esp.ts).
export interface Device {
  readonly chip: string; // 'ESP32-C3'
  readonly flashBytes: number;
  read(offset: number, length: number): Promise<Uint8Array>;
  write(offset: number, data: Uint8Array, progress: (fraction: number) => void): Promise<void>;
  md5(offset: number, length: number): Promise<string>; // computed by the chip
  reset(): Promise<void>; // one hard reset into the selected firmware
  close(): Promise<void>;
}

export interface Dependencies<Port> {
  // Shows the browser's port list; null when the user picked nothing. Must be
  // called before any await in a click handler (Web Serial needs the click).
  requestPort(): Promise<Port | null>;
  open(port: Port): Promise<Device>;
  newt: Promise<Firmware | null>; // the page's own newt.bin, checked against newt.manifest
  wait(milliseconds: number): Promise<void>;
}

// What the page knows about the connected X3 and what it will do.
export interface Survey {
  readonly firmware: Firmware;
  readonly plan: Plan;
}

export type Problem =
  | { readonly code: 'not-found' } // no X3 picked
  | { readonly code: 'no-connection' }
  | { readonly code: 'not-x3' }
  | { readonly code: 'newt-file' } // the page's newt.bin is missing or does not match its manifest
  | { readonly code: 'bad-file'; readonly problem: FileProblem }
  | { readonly code: 'refused'; readonly reason: RefusalReason }
  | { readonly code: 'write-stopped'; readonly survey: Survey } // the X3 is unchanged
  | { readonly code: 'check-failed'; readonly survey: Survey } // the X3 is unchanged
  | { readonly code: 'not-selected'; readonly survey: Survey } // readback shows the old selection: unchanged
  | { readonly code: 'unconfirmed'; readonly survey: Survey }; // either firmware may start; both are whole

export type Step = 'write' | 'check' | 'select' | 'restart';

export type Screen =
  | { readonly kind: 'start' }
  | { readonly kind: 'unsupported' }
  | { readonly kind: 'choose'; readonly firmware?: Firmware } // "Install another firmware"
  | { readonly kind: 'connecting' }
  | { readonly kind: 'checking' } // reading the firmware that stays
  | { readonly kind: 'summary'; readonly survey: Survey }
  | { readonly kind: 'consent'; readonly survey: Survey }
  | { readonly kind: 'already'; readonly survey: Survey }
  | { readonly kind: 'writing'; readonly survey: Survey; readonly step: Step; readonly fraction: number }
  | { readonly kind: 'starting'; readonly survey: Survey; readonly secondsLeft: number }
  | { readonly kind: 'done'; readonly survey: Survey; readonly restarted: boolean }
  | { readonly kind: 'error'; readonly problem: Problem };

// Everything the page must know before it offers a write. The firmware that
// stays is read whole and checked as the bootloader checks it: when it fails,
// the bootloader starts the other slot instead, so the "spare" would be the
// firmware that really runs.
async function survey(device: Device, firmware: Firmware, checking: () => void): Promise<Survey | Problem> {
  if (device.chip !== 'ESP32-C3' || device.flashBytes !== FLASH_BYTES) return { code: 'not-x3' };
  const layout = knownLayout(await device.read(TABLE_OFFSET, TABLE_BYTES));
  const ota = await device.read(layout.otadata, OTADATA_BYTES);
  const heads = [await device.read(layout.slots[0].offset, HEAD_BYTES), await device.read(layout.slots[1].offset, HEAD_BYTES)] as const;
  const found = plan(layout, ota, heads);
  if (firmware.image.length > layout.slots[found.target].size) return { code: 'bad-file', problem: 'too-large' };
  checking();
  const kept = layout.slots[found.running];
  if (!(await slotImageIntact((offset, length) => device.read(offset, length), kept, heads[found.running])))
    throw new Refused('kept-damaged', `app${found.running}, the slot that boots, fails its whole-image check`);
  return { firmware, plan: found };
}

// Newt already runs, exactly this version and build: nothing to install.
function alreadyRunning({ firmware, plan }: Survey): boolean {
  const running = plan.images[plan.running];
  return firmware.build !== undefined && running.kind === 'newt' && running.newtVersion === firmware.version && running.newtBuild === firmware.build;
}

function same(a: Uint8Array, b: Uint8Array): boolean {
  return a.length === b.length && a.every((byte, i) => byte === b[i]);
}

export class Session<Port> {
  #screen: Screen;
  #device: Device | null = null;
  #busy = false;
  #firmware: Promise<Firmware | null> = Promise.resolve(null); // the last connect's, for retry()
  readonly #listeners = new Set<(screen: Screen) => void>();

  constructor(
    private readonly deps: Dependencies<Port>,
    serialSupported: boolean,
  ) {
    this.#screen = serialSupported ? { kind: 'start' } : { kind: 'unsupported' };
  }

  get screen(): Screen {
    return this.#screen;
  }

  subscribe(listener: (screen: Screen) => void): () => void {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  #show(screen: Screen): void {
    this.#screen = screen;
    for (const listener of this.#listeners) listener(screen);
  }

  // N1: "Install Newt". The port list opens first, while the click still counts.
  installNewt(): Promise<void> {
    if (this.#screen.kind !== 'start') return Promise.resolve();
    const port = this.deps.requestPort();
    return this.#connect(port, this.deps.newt);
  }

  // N2: "Install another firmware", then a checked file.
  chooseOther(): void {
    if (this.#screen.kind === 'start') this.#show({ kind: 'choose' });
  }

  chosen(checked: { ok: true; firmware: Firmware } | { ok: false; problem: FileProblem }): void {
    if (this.#screen.kind !== 'choose') return;
    this.#show(checked.ok ? { kind: 'choose', firmware: checked.firmware } : { kind: 'error', problem: { code: 'bad-file', problem: checked.problem } });
  }

  installChosen(): Promise<void> {
    const screen = this.#screen;
    if (screen.kind !== 'choose' || !screen.firmware) return Promise.resolve();
    return this.#connect(this.deps.requestPort(), Promise.resolve(screen.firmware));
  }

  async #connect(portRequest: Promise<Port | null>, firmwareRequest: Promise<Firmware | null>): Promise<void> {
    if (this.#busy) return;
    this.#busy = true;
    this.#firmware = firmwareRequest;
    try {
      this.#show({ kind: 'connecting' });
      let port: Port | null;
      try {
        port = await portRequest;
      } catch {
        port = null; // the browser refused or the list failed: the same help as picking nothing
      }
      if (port === null) return this.#show({ kind: 'error', problem: { code: 'not-found' } });
      const firmware = await firmwareRequest;
      if (!firmware) return this.#show({ kind: 'error', problem: { code: 'newt-file' } });
      try {
        this.#device = await this.deps.open(port);
      } catch {
        return this.#show({ kind: 'error', problem: { code: 'no-connection' } });
      }
      let found: Survey | Problem;
      try {
        found = await survey(this.#device, firmware, () => this.#show({ kind: 'checking' }));
      } catch (error) {
        found = error instanceof Refused ? { code: 'refused', reason: error.reason } : { code: 'no-connection' };
      }
      if ('code' in found) {
        await this.#release(true);
        return this.#show({ kind: 'error', problem: found });
      }
      if (alreadyRunning(found)) {
        await this.#release(true);
        return this.#show({ kind: 'already', survey: found });
      }
      this.#show({ kind: 'summary', survey: found });
    } finally {
      this.#busy = false;
    }
  }

  next(): void {
    if (this.#screen.kind === 'summary') this.#show({ kind: 'consent', survey: this.#screen.survey });
  }

  // Back to the start, from anywhere but a write. Nothing has been written:
  // the X3 restarts into the firmware it ran.
  async cancel(): Promise<void> {
    if (this.#busy) return;
    const kind = this.#screen.kind;
    if (kind === 'writing' || kind === 'starting') return;
    await this.#release(true);
    this.#show({ kind: 'start' });
  }

  // The consented install: the spare slot, its on-chip MD5, one otadata
  // sector with readback, one reset, then the confirmation window.
  async install(): Promise<void> {
    const screen = this.#screen;
    const device = this.#device;
    if (screen.kind !== 'consent' || !device || this.#busy) return;
    this.#busy = true;
    try {
      const { survey: found } = screen;
      const { plan: chosen, firmware } = found;
      const slot = chosen.layout.slots[chosen.target];
      const show = (step: Step, fraction: number) => this.#show({ kind: 'writing', survey: found, step, fraction });

      show('write', 0);
      try {
        guardWrite(chosen, slot.offset, firmware.image.length);
        await device.write(slot.offset, firmware.image, (fraction) => show('write', fraction));
      } catch {
        return await this.#fail({ code: 'write-stopped', survey: found });
      }

      show('check', 1);
      let written: string | null = null;
      try {
        written = await device.md5(slot.offset, firmware.image.length);
      } catch {
        // Treated as a mismatch: the slot is never selected unverified.
      }
      if (written !== md5Hex(firmware.image)) return await this.#fail({ code: 'check-failed', survey: found });

      show('select', 1);
      const outcome = await this.#select(device, chosen);
      if (outcome !== 'selected') return await this.#fail({ code: outcome, survey: found });

      show('restart', 1);
      let restarted = true;
      try {
        await device.reset();
      } catch {
        restarted = false;
      }
      for (let left = CONFIRM_SECONDS; left > 0; --left) {
        this.#show({ kind: 'starting', survey: found, secondsLeft: left });
        await this.deps.wait(1000);
      }
      await this.#release(false);
      this.#show({ kind: 'done', survey: found, restarted });
    } finally {
      this.#busy = false;
    }
  }

  // Writes the selection sector, then reads otadata back (twice at most) to
  // learn what the next boot does, whether or not the write reported success.
  async #select(device: Device, chosen: Plan): Promise<'selected' | 'not-selected' | 'unconfirmed'> {
    const { offset, sector } = chosen.selection;
    try {
      guardWrite(chosen, offset, sector.length);
      await device.write(offset, sector, () => {});
    } catch {
      // The readback decides.
    }
    for (let attempt = 0; attempt < 2; ++attempt) {
      const after = await device.read(chosen.layout.otadata, OTADATA_BYTES).catch(() => null);
      if (!after) continue;
      if (same(after, applied(chosen))) return 'selected';
      return same(after, chosen.ota) ? 'not-selected' : 'unconfirmed';
    }
    return 'unconfirmed';
  }

  // The X3 restarts at once. After an unconfirmed selection the new firmware
  // may be starting, so the port stays open through its confirmation window.
  async #fail(problem: Problem): Promise<void> {
    const hold = problem.code === 'unconfirmed';
    const device = this.#device;
    if (hold && device) await device.reset().catch(() => {});
    this.#show({ kind: 'error', problem });
    if (hold) await this.deps.wait(CONFIRM_SECONDS * 1000);
    await this.#release(!hold);
  }

  async #release(reset: boolean): Promise<void> {
    const device = this.#device;
    this.#device = null;
    if (!device) return;
    if (reset) await device.reset().catch(() => {});
    await device.close().catch(() => {});
  }

  // "Try again" after no X3 was picked or reached: the port list again, for
  // the same firmware, while this click still counts.
  retry(): Promise<void> {
    const screen = this.#screen;
    if (this.#busy || screen.kind !== 'error' || (screen.problem.code !== 'not-found' && screen.problem.code !== 'no-connection'))
      return Promise.resolve();
    return this.#connect(this.deps.requestPort(), this.#firmware);
  }

  // From an error or the end: the start screen again.
  restart(): void {
    const kind = this.#screen.kind;
    if (kind === 'error' || kind === 'done' || kind === 'already') this.#show({ kind: 'start' });
  }
}
