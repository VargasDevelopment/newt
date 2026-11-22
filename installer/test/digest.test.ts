import { describe, expect, it } from 'vitest';
import { md5Hex, sha256Hex } from '../src/digest.ts';

const ascii = (text: string) => Uint8Array.from([...text].map((c) => c.charCodeAt(0)));

describe('digests', () => {
  // RFC 1321's test suite.
  it.each([
    ['', 'd41d8cd98f00b204e9800998ecf8427e'],
    ['a', '0cc175b9c0f1b6a831c399e269772661'],
    ['abc', '900150983cd24fb0d6963f7d28e17f72'],
    ['message digest', 'f96b697d7cb7938d525a2f31aaf161d0'],
    ['abcdefghijklmnopqrstuvwxyz', 'c3fcd3d76192e4007dfb496cca67e13b'],
    ['ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789', 'd174ab98d277d9f5a5611c2c9f419d9f'],
    ['1234567890'.repeat(8), '57edf4a22be3c955ac49da2e2107b67a'],
  ])('MD5 of %j', (text, digest) => expect(md5Hex(ascii(text))).toBe(digest));

  it('MD5 across block boundaries matches the padding rule', () => {
    // 55, 56 and 64 bytes straddle the length field's block.
    expect(md5Hex(new Uint8Array(55))).toBe('c9ea3314b91c9fd4e38f9432064fd1f2');
    expect(md5Hex(new Uint8Array(56))).toBe('e3c4dd21a9171fd39d208efa09bf7883');
    expect(md5Hex(new Uint8Array(64))).toBe('3b5d3c7d207e37dceeedd301e35e2e58');
  });

  it('SHA-256', async () => {
    expect(await sha256Hex(ascii('abc'))).toBe('ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad');
  });
});
