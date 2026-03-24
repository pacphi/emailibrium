import { get, set, del } from 'idb-keyval';

const CRYPTO_KEY_HANDLE = '__emailibrium_crypto_key__';
const IV_LENGTH = 12;

async function getCryptoKey(): Promise<CryptoKey> {
  const existing = await get<CryptoKey>(CRYPTO_KEY_HANDLE);
  if (existing) {
    return existing;
  }

  const key = await crypto.subtle.generateKey(
    { name: 'AES-GCM', length: 256 },
    false, // non-extractable
    ['encrypt', 'decrypt'],
  );

  await set(CRYPTO_KEY_HANDLE, key);
  return key;
}

async function encrypt(
  key: CryptoKey,
  plaintext: string,
): Promise<ArrayBuffer> {
  const iv = crypto.getRandomValues(new Uint8Array(IV_LENGTH));
  const encoded = new TextEncoder().encode(plaintext);

  const ciphertext = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    key,
    encoded,
  );

  // Prepend IV to ciphertext for storage
  const result = new Uint8Array(iv.length + ciphertext.byteLength);
  result.set(iv, 0);
  result.set(new Uint8Array(ciphertext), iv.length);
  return result.buffer;
}

async function decrypt(
  key: CryptoKey,
  data: ArrayBuffer,
): Promise<string> {
  const bytes = new Uint8Array(data);
  const iv = bytes.slice(0, IV_LENGTH);
  const ciphertext = bytes.slice(IV_LENGTH);

  const decrypted = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv },
    key,
    ciphertext,
  );

  return new TextDecoder().decode(decrypted);
}

function storageKey(key: string): string {
  return `__emailibrium_secure_${key}__`;
}

export const secureStorage = {
  async setItem(key: string, value: string): Promise<void> {
    const cryptoKey = await getCryptoKey();
    const encrypted = await encrypt(cryptoKey, value);
    await set(storageKey(key), encrypted);
  },

  async getItem(key: string): Promise<string | null> {
    const cryptoKey = await getCryptoKey();
    const data = await get<ArrayBuffer>(storageKey(key));
    if (!data) {
      return null;
    }
    return decrypt(cryptoKey, data);
  },

  async removeItem(key: string): Promise<void> {
    await del(storageKey(key));
  },
};
