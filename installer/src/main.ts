// The page: Web Serial and fetch wiring around the session and the view.
import './style.css';
import { openX3, X3_PORT_FILTERS } from './esp.ts';
import { checkFile, checkNewt, type Firmware } from './image.ts';
import { Session } from './session.ts';
import { NEWT_BIN_URL, type NewtOffer, render } from './view.ts';

const MANIFEST_URL = 'firmware/newt.manifest';

// The page's own copy of Newt (GitHub Release downloads send no CORS header),
// checked against its manifest before anything can write it.
async function loadNewt(): Promise<Firmware | null> {
  try {
    const [manifest, image] = await Promise.all([
      fetch(MANIFEST_URL, { cache: 'no-cache' }).then((response) => (response.ok ? response.text() : Promise.reject(new Error(response.statusText)))),
      fetch(NEWT_BIN_URL, { cache: 'no-cache' }).then((response) => (response.ok ? response.arrayBuffer() : Promise.reject(new Error(response.statusText)))),
    ]);
    return await checkNewt(manifest, new Uint8Array(image));
  } catch {
    return null;
  }
}

const serial = navigator.serial;
const newt = loadNewt();
let offer: NewtOffer = { status: 'loading' };

const session = new Session<SerialPort>(
  {
    requestPort: () => (serial ? serial.requestPort({ filters: X3_PORT_FILTERS }).catch(() => null) : Promise.resolve(null)),
    open: openX3,
    newt,
    wait: (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds)),
  },
  serial !== undefined && window.isSecureContext,
);

const root = document.getElementById('app');
if (!root) throw new Error('The page has no #app element');

const actions = {
  installNewt: () => void session.installNewt(),
  chooseOther: () => session.chooseOther(),
  pickFile: (file: File) =>
    void file
      .arrayBuffer()
      .then((data) => checkFile(new Uint8Array(data)))
      .catch(() => ({ ok: false, problem: 'not-firmware' }) as const)
      .then((checked) => session.chosen(checked)),
  installChosen: () => void session.installChosen(),
  next: () => session.next(),
  install: () => void session.install(),
  cancel: () => void session.cancel(),
  restart: () => session.restart(),
  retry: () => void session.retry(),
};

const show = () => render(root, session.screen, actions, offer);
session.subscribe(show);
void newt.then((firmware) => {
  offer = firmware ? { status: 'ready', name: firmware.name } : { status: 'failed' };
  show();
});
// Leaving mid-install would close the port before the new firmware confirms.
window.addEventListener('beforeunload', (event) => {
  const kind = session.screen.kind;
  if (kind === 'writing' || kind === 'starting') event.preventDefault();
});
show();
