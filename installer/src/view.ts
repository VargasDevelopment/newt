// Every screen's words and layout. The page (main.ts) and the screenshot
// gallery (gallery.ts) both render through here, so they cannot drift.
import { firmwareName, type FileProblem } from './image.ts';
import type { Problem, Screen, Survey } from './session.ts';
import { UNKNOWN_FIRMWARE } from './slots.ts';
// Cut from the approved brand source by tools/make-install-wordmark.py.
import wordmark from './newt-wordmark.png';

export interface Actions {
  installNewt(): void;
  chooseOther(): void;
  pickFile(file: File): void;
  installChosen(): void;
  next(): void;
  install(): void;
  cancel(): void;
  restart(): void;
  retry(): void;
}

// What the start screen knows about the page's own newt.bin.
export type NewtOffer = { readonly status: 'loading' } | { readonly status: 'ready'; readonly name: string } | { readonly status: 'failed' };

export const NEWT_BIN_URL = 'firmware/newt.bin';
const CROSSPOINT_URL = 'https://github.com/crosspoint-reader/crosspoint-reader/releases';
const CROSSINK_URL = 'https://github.com/uxjulia/CrossInk/releases';
const REPO_URL = 'https://github.com/VargasDevelopment/newt';
const GUIDE_URL = `${REPO_URL}/blob/main/docs/INSTALL.md`;

type Child = Node | string | null | false;

function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  props: Partial<Record<'class' | 'href' | 'role' | 'aria-live' | 'download' | 'target' | 'rel' | 'src' | 'alt' | 'width' | 'height', string>> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  for (const [name, value] of Object.entries(props)) element.setAttribute(name, value);
  for (const child of children) if (child) element.append(child);
  return element;
}

function button(label: string, onClick: () => void, kind: 'primary' | 'secondary' = 'primary'): HTMLButtonElement {
  const element = h('button', { class: `button ${kind}` }, label);
  element.type = 'button';
  element.addEventListener('click', onClick);
  return element;
}

function link(label: string, href: string): HTMLAnchorElement {
  return h('a', { href, target: '_blank', rel: 'noopener' }, label);
}

function buttons(...children: Child[]): HTMLElement {
  return h('div', { class: 'buttons' }, ...children);
}

// The fallback for browsers without Web Serial and X3s that do not show up.
function cardPath(open: boolean): HTMLElement {
  const details = h(
    'details',
    { class: 'card-path' },
    h('summary', {}, 'Install from the card instead'),
    h('p', {}, 'If your X3 runs CrossPoint or CrossInk:'),
    h(
      'ol',
      {},
      h('li', {}, h('a', { href: NEWT_BIN_URL, download: 'newt.bin' }, 'Download newt.bin'), '.'),
      h('li', {}, "Copy it to your X3's card."),
      h('li', {}, 'On the X3, open Settings, then System, then SD Card Firmware Update, and pick newt.bin.'),
    ),
    h('p', { class: 'quiet' }, 'Your current firmware stays as the spare. On stock Xteink firmware, install CrossPoint first.'),
  );
  details.open = open;
  return details;
}

function slotRow(label: string, name: string, note?: string): HTMLElement {
  return h('div', { class: 'slot' }, h('div', { class: 'slot-label' }, label), h('div', { class: 'slot-name' }, name), note ? h('div', { class: 'slot-note' }, note) : null);
}

function runningName({ plan }: Survey): string {
  return firmwareName(plan.images[plan.running]);
}

function spareName({ plan }: Survey): string {
  return firmwareName(plan.images[plan.target]);
}

// A title, with any firmware name in it kept on one line ("0.1.0-alpha.11" never breaks at its hyphen).
function page(title: string | [before: string, name: string, after: string], ...children: Child[]): HTMLElement {
  const heading = typeof title === 'string' ? h('h1', {}, title) : h('h1', {}, title[0], h('span', { class: 'name' }, title[1]), title[2]);
  return h('section', { class: 'screen' }, heading, ...children);
}

const UNCHANGED = 'Nothing on your X3 changed.';

const FILE_PROBLEMS: Record<FileProblem, [title: string, body: string]> = {
  'not-firmware': ["That file isn't X3 firmware", 'Pick a .bin file made for the Xteink X3.'],
  'too-large': ['That file is too big', "It doesn't fit in the X3's spare."],
  damaged: ['That file is damaged', 'Download it again and pick the new copy.'],
  'extra-data': ['That file holds more than one app', 'Pick the firmware file itself, not a full flash image.'],
};

function problemPage(problem: Problem, actions: Actions): HTMLElement {
  const again = buttons(button('Start over', actions.restart));
  switch (problem.code) {
    case 'not-found':
      return page(
        'No X3 picked',
        h('p', {}, "If your X3 wasn't in the list:"),
        h('ol', {}, h('li', {}, 'Use a cable that carries data. Some cables only charge.'), h('li', {}, 'Wake the X3 and keep it on.'), h('li', {}, 'Try again.')),
        buttons(button('Try again', actions.retry), button('Start over', actions.restart, 'secondary')),
        cardPath(false),
      );
    case 'no-connection':
      return page("Couldn't talk to your X3", h('p', {}, 'Close other tabs or apps that use it, reconnect the cable, then try again.'), h('p', {}, UNCHANGED), buttons(button('Try again', actions.retry), button('Start over', actions.restart, 'secondary')));
    case 'not-x3':
      return page("This isn't an X3", h('p', {}, "The connected device isn't an Xteink X3, so nothing was written."), again);
    case 'newt-file':
      return page("Newt's file didn't load", h('p', {}, 'Reload this page and try again.'), h('p', {}, UNCHANGED), again);
    case 'bad-file': {
      const [title, body] = FILE_PROBLEMS[problem.problem];
      return page(title, h('p', {}, body), h('p', {}, UNCHANGED), again);
    }
    case 'refused':
      if (problem.reason === 'unknown-layout')
        return page(
          'This X3 is set up differently',
          h('p', {}, "Its storage layout isn't one this page knows, so it won't write anything."),
          h('p', {}, UNCHANGED),
          again,
          // Newt's Update and Restart into are unproven on this layout (docs/092-install-page.md).
          h('p', { class: 'quiet' }, 'Installing from the card is untested on this layout: nobody has tried it yet.'),
          cardPath(true),
        );
      if (problem.reason === 'changing')
        return page('Your X3 is in the middle of a change', h('p', {}, 'Restart it and wait for its home screen, then try again.'), h('p', {}, UNCHANGED), again);
      if (problem.reason === 'kept-damaged')
        return page(
          "Your X3's firmware didn't pass its check",
          h('p', {}, "The firmware set to start is damaged, so your X3 may really start the one in its spare. This page won't write anything."),
          h('p', {}, UNCHANGED),
          again,
        );
      if (problem.reason === 'sequence-exhausted')
        return page("This page can't switch your X3", h('p', {}, "Its record of which firmware starts can't count any higher, so this page won't write anything."), h('p', {}, UNCHANGED), again);
      return page(
        "Can't tell what your X3 runs",
        h('p', {}, "So this page won't write anything. Restart your X3, wait for its home screen, and try again."),
        h('p', {}, UNCHANGED),
        again,
        cardPath(false),
      );
    case 'write-stopped':
      return page('Writing stopped', h('p', {}, `Your X3 still starts ${runningName(problem.survey)}. Check the cable and try again.`), again);
    case 'check-failed':
      return page('The check failed', h('p', {}, `What was written didn't match, so it won't be used. Your X3 still starts ${runningName(problem.survey)}.`), again);
    case 'not-selected':
      return page(['', problem.survey.firmware.name, " wasn't set to start"], h('p', {}, `Your X3 still starts ${runningName(problem.survey)}. Try again.`), again);
    case 'unconfirmed':
      return page(
        "Couldn't confirm the last step",
        h('p', {}, `Your X3 will start either ${problem.survey.firmware.name} or ${runningName(problem.survey)}. Both are complete.`),
        h('p', {}, 'Keep the cable on for a minute. To be sure, run the install again.'),
        again,
      );
  }
}

const STEPS = { write: 'Writing', check: 'Checking what was written', select: 'Setting it to start', restart: 'Restarting your X3' } as const;

function screenPage(screen: Screen, actions: Actions, newt: NewtOffer): HTMLElement {
  switch (screen.kind) {
    case 'start':
      return page(
        'Install Newt',
        h('p', { class: 'lead' }, 'Newt is a reading firmware for the Xteink X3. It installs beside the firmware you have now, which stays on your X3.'),
        h(
          'ol',
          { class: 'steps' },
          h('li', {}, 'Connect your X3 to this computer with its cable, and wake it.'),
          h('li', {}, 'Click Install Newt, then pick your X3 from the list.'),
          h('li', {}, "Keep the cable on until Newt's Home appears."),
        ),
        buttons(button('Install Newt', actions.installNewt), button('Install another firmware', actions.chooseOther, 'secondary')),
        h('p', { class: 'quiet' }, newt.status === 'ready' ? newt.name : newt.status === 'loading' ? 'Loading Newt…' : ''),
        cardPath(false),
      );
    case 'unsupported':
      return page(
        "This browser can't install Newt",
        h('p', { class: 'lead' }, 'Open this page in Chrome or Edge on a computer. Safari and phone browsers cannot connect to the X3.'),
        cardPath(true),
      );
    case 'choose': {
      const input = h('input');
      input.type = 'file';
      input.accept = '.bin';
      input.className = 'file';
      input.addEventListener('change', () => {
        const file = input.files?.[0];
        if (file) actions.pickFile(file);
      });
      const chooser = h('label', { class: `button ${screen.firmware ? 'secondary' : 'primary'}` }, screen.firmware ? 'Choose another file' : 'Choose file', input);
      return page(
        'Install another firmware',
        h('p', { class: 'lead' }, 'Pick a firmware file (.bin) made for the Xteink X3. It goes in the spare; the firmware you run now stays.'),
        h('p', {}, 'Get one from its maker, such as ', link('CrossPoint', CROSSPOINT_URL), ' or ', link('CrossInk', CROSSINK_URL), '.'),
        screen.firmware ? slotRow('Chosen', screen.firmware.name, `${(screen.firmware.image.length / 1e6).toFixed(1)} MB, checked`) : null,
        buttons(screen.firmware ? button(`Install ${screen.firmware.name}`, actions.installChosen) : null, chooser, button('Back', actions.cancel, 'secondary')),
      );
    }
    case 'connecting':
      return page('Connecting…', h('p', { class: 'lead' }, 'Pick your X3 in the list. It shows as “USB JTAG/serial debug unit”.'), h('div', { class: 'spinner', role: 'status' }));
    case 'checking':
      return page('Checking your X3…', h('p', { class: 'lead' }, 'Reading the firmware it runs now, to make sure it stays whole. This can take a minute.'), h('div', { class: 'spinner', role: 'status' }));
    case 'summary':
      return page(
        'Found your X3',
        h('div', { class: 'slots' }, slotRow('Runs now', runningName(screen.survey)), slotRow('Spare', spareName(screen.survey))),
        h('p', { class: 'quiet' }, 'Only the spare changes.'),
        buttons(button('Continue', actions.next), button('Cancel', actions.cancel, 'secondary')),
      );
    case 'consent': {
      const spare = screen.survey.plan.images[screen.survey.plan.target];
      return page(
        ['Install ', screen.survey.firmware.name, '?'],
        h(
          'div',
          { class: 'slots' },
          slotRow('Stays', runningName(screen.survey), 'Your books, settings and card stay as they are.'),
          slotRow('Replaced', spare.kind === 'empty' ? 'Nothing: the spare is empty.' : spareName(screen.survey)),
        ),
        // An unnamed firmware may be the factory one, which its maker may not offer for download.
        spare.kind === 'other' && spare.name === UNKNOWN_FIRMWARE
          ? h('p', {}, "If this is your X3's original firmware, save a copy first. You may not be able to download it again.")
          : null,
        buttons(button('Install', actions.install), button('Cancel', actions.cancel, 'secondary')),
      );
    }
    case 'writing': {
      const bar = h('progress', { class: 'progress' });
      bar.max = 1;
      bar.value = screen.step === 'write' ? screen.fraction : 1;
      const percent = screen.step === 'write' ? ` ${Math.round(screen.fraction * 100)}%` : '';
      return page(
        ['Installing ', screen.survey.firmware.name, '…'],
        bar,
        h('p', { class: 'status', 'aria-live': 'polite' }, `${STEPS[screen.step]}…${percent}`),
        h('p', { class: 'warn' }, 'Keep the cable on and this page open.'),
      );
    }
    case 'starting':
      return page(
        ['Starting ', screen.survey.firmware.name, '…'],
        h('p', { class: 'lead' }, 'Your X3 is restarting. Keep the cable on until its home screen appears.'),
        h('p', { class: 'status', 'aria-live': 'polite' }, `Finishing in ${screen.secondsLeft} s`),
      );
    case 'done': {
      const { firmware, plan } = screen.survey;
      const isNewt = firmware.identity.kind === 'newt';
      const kept = plan.images[plan.running];
      return page(
        'Done',
        h('p', { class: 'lead' }, screen.restarted ? `${firmware.name} is installed. Unplug your X3 once its home screen is up.` : `${firmware.name} is installed. Restart your X3 to start it.`),
        isNewt ? h('p', {}, 'Copy EPUB books anywhere on the card.') : null,
        h('p', {}, `${runningName(screen.survey)} stays on your X3 as the spare.`),
        // Newt's Settings names the kept firmware the same way (N4).
        isNewt && kept.kind === 'other' ? h('p', {}, `To switch back, use Restart into ${kept.name} in Newt's Settings.`) : null,
        buttons(button('Start over', actions.restart, 'secondary')),
      );
    }
    case 'already':
      return page(['', screen.survey.firmware.name, ' is already installed'], h('p', { class: 'lead' }, 'Your X3 runs it now, so there is nothing to do.'), buttons(button('Start over', actions.restart, 'secondary')));
    case 'error':
      return problemPage(screen.problem, actions);
  }
}

export function render(root: HTMLElement, screen: Screen, actions: Actions, newt: NewtOffer): void {
  root.replaceChildren(
    h('header', { class: 'brand' }, h('img', { class: 'mark', src: wordmark, alt: 'Newt', width: '628', height: '210' }), h('span', { class: 'for' }, 'for Xteink X3')),
    screenPage(screen, actions, newt),
    h(
      'footer',
      {},
      'Books and settings are never erased. This page runs only in your browser. ',
      link('Install guide', GUIDE_URL),
      ' · ',
      link('Newt on GitHub', REPO_URL),
      ' · ',
      h('a', { href: 'NOTICES.txt' }, 'Notices'),
    ),
  );
}
