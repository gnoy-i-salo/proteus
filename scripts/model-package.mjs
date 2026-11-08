import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { stat, mkdir, mkdtemp, open, link, rm } from 'node:fs/promises';
import { dirname, join } from 'node:path';

function validPin(pin) {
  if (!Number.isSafeInteger(pin.bytes) || pin.bytes < 1 || !/^[a-f0-9]{64}$/.test(pin.sha256))
    throw new Error('Invalid model package pin');
}
function checkDigest(size, hash, pin) {
  if (size !== pin.bytes || hash.digest('hex') !== pin.sha256)
    throw new Error('Model package size or SHA256 mismatch');
}
export async function verifyModelFile(path, pin) {
  validPin(pin);
  const metadata = await stat(path);
  if (!metadata.isFile() || metadata.size !== pin.bytes)
    throw new Error('Model package is missing or has a different size');
  const hash = createHash('sha256');
  let size = 0;
  for await (const chunk of createReadStream(path)) {
    size += chunk.length;
    if (size > pin.bytes) throw new Error('Model package exceeds pinned size');
    hash.update(chunk);
  }
  checkDigest(size, hash, pin);
}
export async function prepareModelFile(destination, pin, fetchFile = fetch) {
  validPin(pin);
  try {
    await verifyModelFile(destination, pin);
    return;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
  await mkdir(dirname(destination), { recursive: true });
  const temporary = await mkdtemp(join(dirname(destination), '.model-download-'));
  try {
    const response = await fetchFile(pin.url, { signal: AbortSignal.timeout(180000) });
    if (!response.ok || !response.body) throw new Error('Model package download failed');
    const path = join(temporary, 'verified-file');
    const file = await open(path, 'wx');
    try {
      const hash = createHash('sha256');
      let size = 0;
      for await (const chunk of response.body) {
        size += chunk.length;
        if (size > pin.bytes) throw new Error('Model download exceeds pinned size');
        hash.update(chunk);
        await file.writeFile(chunk);
      }
      checkDigest(size, hash, pin);
      await file.sync();
    } finally {
      await file.close();
    }
    // Exclusive atomic publication also preserves a concurrently created cache.
    try {
      await link(path, destination);
    } catch (error) {
      if (error.code !== 'EEXIST') throw error;
      await verifyModelFile(destination, pin);
    }
  } finally {
    await rm(temporary, { recursive: true, force: true });
  }
}
