import { argon2id } from 'hash-wasm';
import { diaryPayloadSchema, type DiaryPayload } from './schema';

export const MAGIC = new Uint8Array([0x44, 0x49, 0x41, 0x52, 0x59, 0x1a, 0x01, 0x00]);
export const VERSION = 1;
export const HEADER_LEN = 57;
export const FLAG_ENCRYPTED = 0b0000_0001;
export const KDF_NONE = 0;
export const KDF_ARGON2ID = 1;
export const SALT_LEN = 16;
export const NONCE_LEN = 12;
export const TAG_LEN = 16;
export const MAX_M_COST = 262_144;
export const MAX_T_COST = 10;
export const DEFAULT_MAX_PAYLOAD = 67_108_864;

export const DEFAULT_KDF = {
  mCost: 65_536,
  tCost: 3,
  pCost: 1
} as const;

export type KdfParams = {
  mCost: number;
  tCost: number;
  pCost: number;
};

export type DiaryHeader = {
  version: number;
  flags: number;
  kdf: number;
  mCost: number;
  tCost: number;
  pCost: number;
  salt: Uint8Array;
  nonce: Uint8Array;
  payloadLen: number;
};

export type DiaryErrorCode =
  | 'not_diary'
  | 'unsupported_version'
  | 'bad_flags'
  | 'bad_kdf'
  | 'kdf_limits'
  | 'length_mismatch'
  | 'too_large'
  | 'decrypt_failed'
  | 'bad_json'
  | 'insecure_context';

export class DiaryFormatError extends Error {
  constructor(
    public readonly code: DiaryErrorCode,
    message: string
  ) {
    super(message);
    this.name = 'DiaryFormatError';
  }
}

export function parseHeader(
  file: Uint8Array,
  maxPayload = DEFAULT_MAX_PAYLOAD
): DiaryHeader {
  if (file.length < HEADER_LEN) {
    throw new DiaryFormatError('not_diary', '文件头不完整');
  }
  for (let i = 0; i < MAGIC.length; i++) {
    if (file[i] !== MAGIC[i]) {
      throw new DiaryFormatError('not_diary', 'magic 不匹配，不是 .diary 文件');
    }
  }
  const view = new DataView(file.buffer, file.byteOffset, file.byteLength);
  const version = view.getUint16(8, true);
  if (version !== VERSION) {
    throw new DiaryFormatError(
      'unsupported_version',
      `不支持的 .diary 版本 ${version}`
    );
  }
  const flags = view.getUint8(10);
  if ((flags & ~FLAG_ENCRYPTED) !== 0) {
    throw new DiaryFormatError('bad_flags', 'flags 含未知位');
  }
  const kdf = view.getUint8(11);
  if (kdf !== KDF_NONE && kdf !== KDF_ARGON2ID) {
    throw new DiaryFormatError('bad_kdf', '未知 KDF');
  }
  const mCost = view.getUint32(12, true);
  const tCost = view.getUint32(16, true);
  const pCost = view.getUint8(20);
  const salt = file.slice(21, 37);
  const nonce = file.slice(37, 49);
  const bigLen = view.getBigUint64(49, true);
  if (bigLen > BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new DiaryFormatError('too_large', 'payload 长度无意义');
  }
  const payloadLen = Number(bigLen);
  if (payloadLen > maxPayload) {
    throw new DiaryFormatError('too_large', 'payload 超过上限');
  }
  if (payloadLen !== file.length - HEADER_LEN) {
    throw new DiaryFormatError('length_mismatch', 'payload_len 与实际长度不一致');
  }

  const encrypted = (flags & FLAG_ENCRYPTED) !== 0;
  if (kdf === KDF_NONE) {
    if (encrypted) {
      throw new DiaryFormatError('bad_flags', '未加密文件不能带 ENCRYPTED 标志');
    }
    if (mCost !== 0 || tCost !== 0 || pCost !== 0) {
      throw new DiaryFormatError('bad_kdf', 'KDF=none 时参数必须为 0');
    }
    if (salt.some((byte) => byte !== 0) || nonce.some((byte) => byte !== 0)) {
      throw new DiaryFormatError('bad_kdf', 'KDF=none 时 salt/nonce 必须为 0');
    }
  } else {
    if (!encrypted) {
      throw new DiaryFormatError('bad_flags', 'KDF=argon2id 时必须设置 ENCRYPTED 标志');
    }
    if (mCost === 0 || tCost === 0 || pCost === 0) {
      throw new DiaryFormatError('bad_kdf', 'Argon2id 参数不能为 0');
    }
    if (mCost < 8 * pCost) {
      throw new DiaryFormatError('bad_kdf', 'Argon2id 内存参数过小');
    }
    if (mCost > MAX_M_COST || tCost > MAX_T_COST) {
      throw new DiaryFormatError('kdf_limits', 'KDF 参数超过上限');
    }
    if (payloadLen < TAG_LEN) {
      throw new DiaryFormatError('length_mismatch', '密文被截断');
    }
  }

  return {
    version,
    flags,
    kdf,
    mCost,
    tCost,
    pCost,
    salt,
    nonce,
    payloadLen
  };
}

export function buildHeader(header: DiaryHeader): Uint8Array {
  const out = new Uint8Array(HEADER_LEN);
  out.set(MAGIC, 0);
  const view = new DataView(out.buffer);
  view.setUint16(8, header.version, true);
  view.setUint8(10, header.flags);
  view.setUint8(11, header.kdf);
  view.setUint32(12, header.mCost, true);
  view.setUint32(16, header.tCost, true);
  view.setUint8(20, header.pCost);
  out.set(header.salt, 21);
  out.set(header.nonce, 37);
  view.setBigUint64(49, BigInt(header.payloadLen), true);
  return out;
}

export type EncryptOptions = Partial<KdfParams> & {
  salt?: Uint8Array;
  nonce?: Uint8Array;
};

export async function encryptDiary(
  input: Uint8Array | string,
  password: string,
  options: EncryptOptions = {}
): Promise<Uint8Array> {
  const subtle = requireSubtle();
  const encoder = new TextEncoder();
  const plaintext = typeof input === 'string' ? encoder.encode(input) : input;
  if (plaintext.length + TAG_LEN > DEFAULT_MAX_PAYLOAD) {
    throw new DiaryFormatError('too_large', '导出内容超过 64 MiB');
  }

  const params: KdfParams = {
    mCost: options.mCost ?? DEFAULT_KDF.mCost,
    tCost: options.tCost ?? DEFAULT_KDF.tCost,
    pCost: options.pCost ?? DEFAULT_KDF.pCost
  };
  if (
    params.mCost === 0 ||
    params.tCost === 0 ||
    params.pCost === 0 ||
    params.mCost > MAX_M_COST ||
    params.tCost > MAX_T_COST
  ) {
    throw new DiaryFormatError('kdf_limits', 'KDF 参数超过上限');
  }

  const salt = options.salt ?? randomBytes(SALT_LEN);
  const nonce = options.nonce ?? randomBytes(NONCE_LEN);
  if (salt.length !== SALT_LEN || nonce.length !== NONCE_LEN) {
    throw new DiaryFormatError('length_mismatch', 'salt 或 nonce 长度错误');
  }

  const header = buildHeader({
    version: VERSION,
    flags: FLAG_ENCRYPTED,
    kdf: KDF_ARGON2ID,
    ...params,
    salt,
    nonce,
    payloadLen: plaintext.length + TAG_LEN
  });

  const rawKey = await deriveDiaryKey(password, salt, params);
  try {
    const key = await subtle.importKey(
      'raw',
      rawKey as BufferSource,
      { name: 'AES-GCM' },
      false,
      ['encrypt']
    );
    const ciphertext = await subtle.encrypt(
      {
        name: 'AES-GCM',
        iv: nonce as BufferSource,
        additionalData: header as BufferSource
      },
      key,
      plaintext as BufferSource
    );
    const out = new Uint8Array(HEADER_LEN + ciphertext.byteLength);
    out.set(header, 0);
    out.set(new Uint8Array(ciphertext), HEADER_LEN);
    return out;
  } finally {
    rawKey.fill(0);
  }
}

export async function decryptDiary(
  file: Uint8Array,
  password: string,
  maxPayload = DEFAULT_MAX_PAYLOAD
): Promise<Uint8Array> {
  const header = parseHeader(file, maxPayload);
  const payload = file.slice(HEADER_LEN);
  if (header.kdf === KDF_NONE) {
    return payload;
  }

  const subtle = requireSubtle();
  let rawKey: Uint8Array;
  try {
    rawKey = await deriveDiaryKey(password, header.salt, {
      mCost: header.mCost,
      tCost: header.tCost,
      pCost: header.pCost
    });
  } catch {
    throw new DiaryFormatError('bad_kdf', 'KDF 参数非法');
  }
  try {
    const key = await subtle.importKey(
      'raw',
      rawKey as BufferSource,
      { name: 'AES-GCM' },
      false,
      ['decrypt']
    );
    try {
      const plaintext = await subtle.decrypt(
        {
          name: 'AES-GCM',
          iv: header.nonce as BufferSource,
          additionalData: file.slice(0, HEADER_LEN) as BufferSource
        },
        key,
        payload as BufferSource
      );
      return new Uint8Array(plaintext);
    } catch {
      throw new DiaryFormatError('decrypt_failed', '密码错误，或文件已损坏');
    }
  } finally {
    rawKey.fill(0);
  }
}

export async function deriveDiaryKey(
  password: string,
  salt: Uint8Array,
  params: KdfParams = DEFAULT_KDF
): Promise<Uint8Array> {
  const encoder = new TextEncoder();
  const passwordBytes = encoder.encode(password);
  try {
    return await argon2id({
      password: passwordBytes,
      salt,
      parallelism: params.pCost,
      iterations: params.tCost,
      memorySize: params.mCost,
      hashLength: 32,
      outputType: 'binary'
    });
  } finally {
    passwordBytes.fill(0);
  }
}

export function parseDiaryJson(
  bytes: Uint8Array,
  maxPayload = DEFAULT_MAX_PAYLOAD
): DiaryPayload {
  if (bytes.length > maxPayload) {
    throw new DiaryFormatError('too_large', 'payload 超过上限');
  }
  let text: string;
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(bytes);
  } catch {
    throw new DiaryFormatError('bad_json', 'payload 不是 UTF-8');
  }
  let json: unknown;
  try {
    json = JSON.parse(text);
  } catch {
    throw new DiaryFormatError('bad_json', 'payload 不是合法 JSON');
  }
  const parsed = diaryPayloadSchema.safeParse(json);
  if (!parsed.success) {
    throw new DiaryFormatError(
      'bad_json',
      `payload 结构非法: ${parsed.error.issues[0]?.message ?? '未知错误'}`
    );
  }
  return parsed.data;
}

export function serializeDiaryPayload(payload: DiaryPayload): Uint8Array {
  const parsed = diaryPayloadSchema.parse(payload);
  return new TextEncoder().encode(JSON.stringify(parsed));
}

export function isEncrypted(header: DiaryHeader): boolean {
  return (header.flags & FLAG_ENCRYPTED) !== 0;
}

export function randomBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length);
  globalThis.crypto.getRandomValues(bytes);
  return bytes;
}

export function hexToBytes(hex: string): Uint8Array {
  if (hex.length % 2 !== 0 || !/^[0-9a-f]*$/i.test(hex)) {
    throw new Error('非法十六进制字符串');
  }
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) {
    out[i] = Number.parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

export function bytesToHex(bytes: Uint8Array): string {
  return [...bytes].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

function requireSubtle(): SubtleCrypto {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) {
    throw new DiaryFormatError(
      'insecure_context',
      '浏览器加密需要 HTTPS（secure context）'
    );
  }
  return subtle;
}
