// Digests and hex. MD5 (RFC 1321): The ESP32 flasher stub reports a written range's MD5, and
// ESP-IDF partition tables end in one; browsers' Web Crypto has no MD5.
const SHIFTS = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21] as const;
const CONSTANTS = Array.from({ length: 64 }, (_, i) => Math.floor(Math.abs(Math.sin(i + 1)) * 2 ** 32) >>> 0);

export function md5Hex(data: Uint8Array): string {
  const padded = new Uint8Array((((data.length + 8) >> 6) + 1) << 6);
  padded.set(data);
  padded[data.length] = 0x80;
  const view = new DataView(padded.buffer);
  const bits = data.length * 8;
  view.setUint32(padded.length - 8, bits >>> 0, true);
  view.setUint32(padded.length - 4, Math.floor(bits / 2 ** 32), true);
  const state = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
  const words = new Array<number>(16);
  for (let block = 0; block < padded.length; block += 64) {
    for (let i = 0; i < 16; ++i) words[i] = view.getUint32(block + 4 * i, true);
    let [a, b, c, d] = state as [number, number, number, number];
    for (let i = 0; i < 64; ++i) {
      const round = i >> 4;
      const f = round === 0 ? (b & c) | (~b & d) : round === 1 ? (d & b) | (~d & c) : round === 2 ? b ^ c ^ d : c ^ (b | ~d);
      const g = round === 0 ? i : round === 1 ? (5 * i + 1) & 15 : round === 2 ? (3 * i + 5) & 15 : (7 * i) & 15;
      const sum = (a + f + CONSTANTS[i]! + words[g]!) | 0;
      const shift = SHIFTS[(round << 2) | (i & 3)]!;
      [a, d, c] = [d, c, b];
      b = (b + ((sum << shift) | (sum >>> (32 - shift)))) | 0;
    }
    state[0] = (state[0]! + a) | 0;
    state[1] = (state[1]! + b) | 0;
    state[2] = (state[2]! + c) | 0;
    state[3] = (state[3]! + d) | 0;
  }
  const out = new Uint8Array(16);
  const outView = new DataView(out.buffer);
  state.forEach((word, i) => outView.setUint32(4 * i, word >>> 0, true));
  return toHex(out);
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export function fromHex(text: string): Uint8Array {
  return Uint8Array.from(text.match(/../g) ?? [], (pair) => Number.parseInt(pair, 16));
}

export async function sha256Hex(data: Uint8Array): Promise<string> {
  return toHex(new Uint8Array(await crypto.subtle.digest('SHA-256', new Uint8Array(data))));
}
