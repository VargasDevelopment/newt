import { describe, expect, it } from 'vitest';
import { fromHex, sha256Hex } from '../src/digest.ts';
import { appImage, CROSSINK, CROSSPOINT, NEWT, STOCK } from '../src/fixtures.ts';
import {
  checkFile,
  checkNewt,
  extraData,
  firmwareName,
  imageIntact,
  imageLength,
  parseManifest,
  SLOT_BYTES,
  slotImageIntact,
} from '../src/image.ts';
import { classify, HEAD_BYTES } from '../src/slots.ts';
import vectors from './x3-slot-vectors.json';

describe("tools/x3_slots.py's image vectors", () => {
  it.each(vectors.images)('$name: intact $intact, length $length', async ({ hex, slotBytes, intact, length, extraData: extra }) => {
    const data = fromHex(hex);
    expect(await imageIntact(data, slotBytes)).toBe(intact);
    expect(imageLength(data)).toBe(length);
    expect(extraData(data)).toBe(extra);
  });

  // #368: a chosen file installs as its image alone when only erased padding
  // follows it; any other byte after it is extra data.
  it.each([
    ['erased padding', true],
    ['extra data', false],
  ])('a chosen file with %s installs: %s', async (name, installs) => {
    const vector = vectors.images.find((image) => image.name === name);
    if (!vector) throw new Error(`no ${name} vector`);
    const checked = await checkFile(fromHex(vector.hex));
    expect(checked.ok ? checked.firmware.image.length : checked.problem).toBe(installs ? vector.length : 'extra-data');
  });

  // The kept slot's check, read from the device piece by piece, agrees with
  // the whole-buffer rule, and reads nothing past the image.
  it.each(vectors.images)('$name, read from a slot: intact $intact', async ({ hex, slotBytes, intact, length }) => {
    const data = fromHex(hex);
    let furthest = 0;
    const read = async (offset: number, count: number) => {
      furthest = Math.max(furthest, offset + count);
      return data.slice(offset, offset + count);
    };
    expect(await slotImageIntact(read, { offset: 0, size: slotBytes }, data.subarray(0, HEAD_BYTES))).toBe(intact);
    if (length !== null && intact) expect(furthest).toBeLessThanOrEqual(length);
  });
});

describe('names', () => {
  it.each([
    [CROSSPOINT, 'CrossPoint 2.0.0'],
    [CROSSINK, 'CrossInk v1.6.1'],
    [NEWT, 'Newt 0.1.0-alpha.11'],
    // Stock reports the same project as Newt; nothing guesses at it.
    [STOCK, 'Another firmware'],
    [{ project: 'papyrix', version: '1.0' }, 'Another firmware'],
  ])('%o is %s', async (spec, name) => expect(firmwareName(classify((await appImage(spec)).subarray(0, HEAD_BYTES)))).toBe(name));
});

describe('a chosen file', () => {
  it('passes as exactly its image, erased padding dropped', async () => {
    const image = await appImage(CROSSINK);
    const padded = new Uint8Array(image.length + 4096).fill(0xff);
    padded.set(image);
    for (const file of [image, padded]) {
      const checked = await checkFile(file);
      expect(checked.ok && checked.firmware.name).toBe('CrossInk v1.6.1');
      expect(checked.ok && checked.firmware.image).toEqual(image);
    }
  });

  it('refuses what is not one intact app image that fits', async () => {
    const image = await appImage(CROSSPOINT);
    const problem = async (data: Uint8Array) => {
      const checked = await checkFile(data);
      return checked.ok ? null : checked.problem;
    };
    const flipped = new Uint8Array(image);
    flipped[0x100]! ^= 1;
    expect(await problem(flipped)).toBe('damaged');
    const trailing = new Uint8Array(image.length + 16).fill(0xff);
    trailing.set(image);
    trailing[image.length + 3] = 0;
    expect(await problem(trailing)).toBe('extra-data');
    // A bootloader or merged flash image has no app descriptor at 0x20.
    const noDescriptor = new Uint8Array(image);
    noDescriptor.fill(0, 0x20, 0x24);
    expect(await problem(noDescriptor)).toBe('not-firmware');
    expect(await problem(new Uint8Array(4096).fill(0xff))).toBe('not-firmware');
    expect(await problem(new TextEncoder().encode('<html>not found</html>'))).toBe('not-firmware');
    const esp32 = new Uint8Array(image);
    esp32[12] = 0; // another chip
    expect(await problem(esp32)).toBe('not-firmware');
    expect(await problem(await appImage({ ...CROSSPOINT, extra: SLOT_BYTES }))).toBe('too-large');
  });
});

async function manifestFor(image: Uint8Array, version = '0.1.0-alpha.11', build = '0ee595fb7ae0'): Promise<string> {
  return `format=newt-update-1\nkind=newt\nversion=${version}\nbuild=${build}\nsize=${image.length}\nsha256=${await sha256Hex(image)}\n`;
}

describe("Newt's release pair", () => {
  it('parses only the exact manifest', () => {
    const good = `format=newt-update-1\nkind=newt\nversion=0.1.0-alpha.11\nbuild=0ee595fb7ae0\nsize=3296384\nsha256=${'a'.repeat(64)}\n`;
    expect(parseManifest(good)).toEqual({ version: '0.1.0-alpha.11', build: '0ee595fb7ae0', size: 3296384, sha256: 'a'.repeat(64) });
    for (const bad of [good.replace('kind=newt', 'kind=crossink'), good.trimEnd(), good.replace('size=3', 'size=03'), `${good}extra=1\n`, good.replace('\n', '\r\n')])
      expect(parseManifest(bad)).toBeNull();
  });

  it('accepts the bytes the manifest names, and nothing else', async () => {
    const image = await appImage(NEWT);
    expect((await checkNewt(await manifestFor(image), image))?.name).toBe('Newt 0.1.0-alpha.11');
    const other = await appImage({ ...NEWT, extra: 16 });
    expect(await checkNewt(await manifestFor(image), other)).toBeNull(); // size and digest differ
    expect(await checkNewt(await manifestFor(image, '0.1.0-alpha.12'), image)).toBeNull(); // identity differs
    expect(await checkNewt(await manifestFor(image, undefined, 'aaaaaaaaaaaa'), image)).toBeNull();
    const crossInk = await appImage(CROSSINK);
    expect(await checkNewt(await manifestFor(crossInk), crossInk)).toBeNull(); // not Newt
    expect(await checkNewt('', image)).toBeNull();
  });
});
