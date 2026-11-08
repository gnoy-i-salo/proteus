import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { prepareModelFile } from './model-package.mjs';
const pin = JSON.parse(await readFile(new URL('./e5-package.json', import.meta.url), 'utf8'));
const destination = fileURLToPath(new URL('../.cache/models/e5-tokenizer.json', import.meta.url));
await prepareModelFile(destination, {
  ...pin,
  url: `https://huggingface.co/intfloat/multilingual-e5-small/resolve/${pin.revision}/onnx/tokenizer.json`,
});
console.log(`Verified E5 tokenizer: ${pin.revision}`);
