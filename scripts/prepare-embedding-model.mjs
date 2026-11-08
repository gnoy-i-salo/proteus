import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { prepareModelFile } from './model-package.mjs';
const pin = JSON.parse(
  await readFile(new URL('./e5-embedding-package.json', import.meta.url), 'utf8'),
);
for (const [key, filename, destination] of [
  ['config', 'config.json', 'e5-config.json'],
  ['weights', 'model.safetensors', 'e5-model.safetensors'],
]) {
  await prepareModelFile(
    fileURLToPath(new URL(`../.cache/models/${destination}`, import.meta.url)),
    {
      ...pin[key],
      url: `https://huggingface.co/${pin.modelId}/resolve/${pin.revision}/${filename}`,
    },
    (url) => fetch(url, { signal: AbortSignal.timeout(600000) }),
  );
}
console.log(`Verified local E5 CPU model: ${pin.revision}`);
