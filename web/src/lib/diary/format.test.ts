import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  DiaryFormatError,
  bytesToHex,
  decryptDiary,
  deriveDiaryKey,
  encryptDiary,
  hexToBytes,
  parseDiaryJson,
  parseHeader,
  sealPlaintextDiary
} from './format';

type Vector = {
  name: string;
  kind: 'encrypted' | 'decrypt' | 'plaintext';
  expect: 'ok' | 'fail';
  password?: string;
  salt_hex?: string;
  nonce_hex?: string;
  plaintext_utf8?: string;
  key_hex?: string;
  header_hex?: string;
  ciphertext_hex?: string;
  file_hex: string;
};

const document = JSON.parse(
  readFileSync(new URL('../../../../docs/test-vectors.json', import.meta.url), 'utf8')
) as { vectors: Vector[] };

describe('.diary v1 shared vectors', () => {
  for (const vector of document.vectors) {
    it(vector.name, async () => {
      const file = hexToBytes(vector.file_hex);
      if (vector.kind === 'encrypted') {
        const salt = hexToBytes(vector.salt_hex!);
        const nonce = hexToBytes(vector.nonce_hex!);
        const plaintext = new TextEncoder().encode(vector.plaintext_utf8!);
        const produced = await encryptDiary(plaintext, vector.password!, { salt, nonce });
        expect(bytesToHex(produced)).toBe(vector.file_hex);
        expect(bytesToHex(produced.slice(0, 57))).toBe(vector.header_hex);
        expect(bytesToHex(produced.slice(57))).toBe(vector.ciphertext_hex);
        const key = await deriveDiaryKey(vector.password!, salt);
        expect(bytesToHex(key)).toBe(vector.key_hex);
        key.fill(0);
        const opened = await decryptDiary(produced, vector.password!);
        expect(new TextDecoder().decode(opened)).toBe(vector.plaintext_utf8);
      } else if (vector.kind === 'decrypt') {
        if (vector.expect === 'ok') {
          const opened = await decryptDiary(file, vector.password ?? '');
          expect(new TextDecoder().decode(opened)).toBe(vector.plaintext_utf8);
        } else {
          await expect(decryptDiary(file, vector.password ?? '')).rejects.toBeInstanceOf(
            DiaryFormatError
          );
        }
      } else {
        const opened = await decryptDiary(file, '');
        expect(new TextDecoder().decode(opened)).toBe(vector.plaintext_utf8);
      }
    });
  }

  it('rejects version and KDF cap tampering before NDF work', async () => {
    const base = hexToBytes(document.vectors[0].file_hex);
    const highVersion = base.slice();
    highVersion[8] = 2;
    expect(() => parseHeader(highVersion)).toThrowError(/版本/);

    const highMemory = base.slice();
    new DataView(highMemory.buffer).setUint32(12, 262_145, true);
    expect(() => parseHeader(highMemory)).toThrowError(/上限/);

    const badFlags = base.slice();
    badFlags[10] = 0;
    expect(() => parseHeader(badFlags)).toThrowError(/ENCRYPTED/);

    expect(() => parseHeader(base.slice(0, -1))).toThrowError();
    expect(() => parseHeader(base.slice(0, 57))).toThrowError();
  });

  it('creates unencrypted files with zeroed KDF fields', async () => {
    const file = sealPlaintextDiary('hello 世界');
    const header = parseHeader(file);
    expect(header.kdf).toBe(0);
    expect(header.flags).toBe(0);
    expect(header.mCost).toBe(0);
    expect(header.salt.every((byte) => byte === 0)).toBe(true);
    expect(new TextDecoder().decode(await decryptDiary(file, ''))).toBe('hello 世界');
  });

  it('parses JSON payloads and rejects malformed ones', () => {
    const payload = parseDiaryJson(new TextEncoder().encode(document.vectors[0].plaintext_utf8!));
    expect(payload.format).toBe('diary');
    expect(payload.entries[0].content_md).toContain('代码');

    expect(() => parseDiaryJson(new TextEncoder().encode('not json'))).toThrowError(/JSON/);
    expect(() =>
      parseDiaryJson(
        new TextEncoder().encode(
          JSON.stringify({ format: 'diary', version: 2, tags: [], entries: [] })
        )
      )
    ).toThrowError(/结构/);
  });
});
