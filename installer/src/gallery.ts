// Every screen of the page from fixtures, through the page's own view, for
// screenshots and review. `?state=NAME` shows one; no parameter lists them.
import './style.css';
import { appImage, CROSSINK, CROSSPOINT, NEWT, otadata, STOCK, X3_LAYOUT, type AppSpec } from './fixtures.ts';
import { type Firmware, firmwareName } from './image.ts';
import type { Problem, Screen, Survey } from './session.ts';
import { classify, HEAD_BYTES, OtaState, plan } from './slots.ts';
import { type Actions, type NewtOffer, render } from './view.ts';

const NEWT_NAME = 'Newt 0.1.0-alpha.11';
const noop = () => {};
const actions: Actions = { installNewt: noop, chooseOther: noop, pickFile: noop, installChosen: noop, next: noop, install: noop, cancel: noop, restart: noop, retry: noop };

async function firmware(spec: AppSpec): Promise<Firmware> {
  const image = await appImage({ ...spec, extra: 3_300_000 });
  const identity = classify(image.subarray(0, HEAD_BYTES));
  return { name: firmwareName(identity), image, identity };
}

async function survey(running: AppSpec, spare: AppSpec | null, chosen: Firmware): Promise<Survey> {
  const head = async (spec: AppSpec | null) => (spec ? (await appImage(spec)).slice(0, HEAD_BYTES) : new Uint8Array(HEAD_BYTES).fill(0xff));
  const found = plan(X3_LAYOUT, otadata([7, OtaState.Valid], [6, OtaState.Valid]), [await head(running), await head(spare)]);
  return { firmware: chosen, plan: found };
}

async function screens(): Promise<[name: string, screen: Screen, offer?: NewtOffer][]> {
  const newt = await firmware(NEWT);
  const crossInk = await firmware(CROSSINK);
  const fresh = await survey(CROSSPOINT, null, newt);
  const replacing = await survey(CROSSINK, STOCK, newt);
  const error = (problem: Problem): Screen => ({ kind: 'error', problem });
  return [
    ['start', { kind: 'start' }],
    ['start-loading', { kind: 'start' }, { status: 'loading' }],
    ['unsupported', { kind: 'unsupported' }],
    ['choose', { kind: 'choose' }],
    ['choose-chosen', { kind: 'choose', firmware: crossInk }],
    ['connecting', { kind: 'connecting' }],
    ['checking', { kind: 'checking' }],
    ['summary', { kind: 'summary', survey: fresh }],
    ['consent', { kind: 'consent', survey: fresh }],
    ['consent-replaces', { kind: 'consent', survey: replacing }],
    ['writing', { kind: 'writing', survey: fresh, step: 'write', fraction: 0.42 }],
    ['writing-check', { kind: 'writing', survey: fresh, step: 'check', fraction: 1 }],
    ['starting', { kind: 'starting', survey: fresh, secondsLeft: 23 }],
    ['done', { kind: 'done', survey: fresh, restarted: true }],
    ['done-from-crossink', { kind: 'done', survey: replacing, restarted: true }],
    ['done-from-stock', { kind: 'done', survey: await survey(STOCK, null, newt), restarted: true }],
    ['done-other-firmware', { kind: 'done', survey: await survey(NEWT, CROSSINK, crossInk), restarted: true }],
    ['already', { kind: 'already', survey: fresh }],
    ['error-not-found', error({ code: 'not-found' })],
    ['error-no-connection', error({ code: 'no-connection' })],
    ['error-not-x3', error({ code: 'not-x3' })],
    ['error-newt-file', error({ code: 'newt-file' })],
    ['error-bad-file-not-firmware', error({ code: 'bad-file', problem: 'not-firmware' })],
    ['error-bad-file-damaged', error({ code: 'bad-file', problem: 'damaged' })],
    ['error-bad-file-too-large', error({ code: 'bad-file', problem: 'too-large' })],
    ['error-bad-file-extra-data', error({ code: 'bad-file', problem: 'extra-data' })],
    ['error-unknown-layout', error({ code: 'refused', reason: 'unknown-layout' })],
    ['error-changing', error({ code: 'refused', reason: 'changing' })],
    ['error-no-selection', error({ code: 'refused', reason: 'no-selection' })],
    ['error-boot-slot-not-app', error({ code: 'refused', reason: 'boot-slot-not-app' })],
    ['error-kept-damaged', error({ code: 'refused', reason: 'kept-damaged' })],
    ['error-sequence-exhausted', error({ code: 'refused', reason: 'sequence-exhausted' })],
    ['error-write-stopped', error({ code: 'write-stopped', survey: fresh })],
    ['error-check-failed', error({ code: 'check-failed', survey: fresh })],
    ['error-not-selected', error({ code: 'not-selected', survey: fresh })],
    ['error-unconfirmed', error({ code: 'unconfirmed', survey: fresh })],
  ];
}

async function main(): Promise<void> {
  const root = document.getElementById('app');
  if (!root) return;
  const wanted = new URLSearchParams(location.search).get('state');
  const all = await screens();
  const ready: NewtOffer = { status: 'ready', name: NEWT_NAME };
  if (wanted) {
    const found = all.find(([name]) => name === wanted);
    if (!found) throw new Error(`No screen named ${wanted}`);
    render(root, found[1], actions, found[2] ?? ready);
  } else {
    root.replaceChildren(
      ...all.map(([name, screen, offer]) => {
        const frame = document.createElement('div');
        frame.id = name;
        render(frame, screen, actions, offer ?? ready);
        return frame;
      }),
    );
  }
  document.body.dataset.ready = String(all.length);
}

void main();
