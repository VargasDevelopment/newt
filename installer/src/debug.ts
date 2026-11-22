// The page's ?debug mode: everything esptool-js and the page print, with
// times, kept so the visitor can save it as one file and send it to us. Off by
// default; nothing leaves the browser unless the visitor sends the file.
export const debugEnabled = new URLSearchParams(location.search).has('debug');

export function startDebugLog(): void {
  const lines: string[] = [];
  const started = performance.now();
  const keep = (level: string, args: readonly unknown[]) =>
    lines.push(`${((performance.now() - started) / 1000).toFixed(3)} ${level} ${args.map(String).join(' ')}`);
  for (const level of ['log', 'info', 'warn', 'error', 'debug'] as const) {
    const original = console[level].bind(console);
    console[level] = (...args: unknown[]) => {
      keep(level, args);
      original(...args);
    };
  }
  window.addEventListener('error', (event) => keep('uncaught', [event.message]));
  window.addEventListener('unhandledrejection', (event) => keep('unhandled', [event.reason]));
  console.log(`newt: debug log ${new Date().toISOString()} ${location.href} | ${navigator.userAgent}`);

  const save = document.createElement('button');
  save.type = 'button';
  save.className = 'button secondary debug-save';
  save.textContent = 'Save debug log';
  save.addEventListener('click', () => {
    const url = URL.createObjectURL(new Blob([lines.join('\n') + '\n'], { type: 'text/plain' }));
    const link = Object.assign(document.createElement('a'), { href: url, download: 'newt-install-log.txt' });
    link.click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  });
  document.body.append(save);
}
