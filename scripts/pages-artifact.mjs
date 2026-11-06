import {
  cpSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readdirSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { join } from 'node:path';

function validateTree(directory) {
  for (const entry of readdirSync(directory)) {
    const path = join(directory, entry);
    const stat = lstatSync(path);
    if (stat.isSymbolicLink()) throw new Error('Symbolic links are not allowed in Pages artifacts');
    if (stat.isDirectory()) validateTree(path);
    else if (!stat.isFile()) throw new Error('Only regular files are allowed in Pages artifacts');
  }
}

export function assemblePages({ authPath, diagramsPath, outputPath, revision }) {
  if (!/^[a-f0-9]{40}$/u.test(revision)) throw new Error('Invalid revision metadata');
  if (!existsSync(join(authPath, 'index.html'))) throw new Error('Missing auth index');
  if (!existsSync(join(diagramsPath, 'index.html'))) throw new Error('Missing architecture index');
  validateTree(authPath);
  validateTree(diagramsPath);
  rmSync(outputPath, { recursive: true, force: true });
  mkdirSync(outputPath, { recursive: true });
  cpSync(authPath, join(outputPath, 'auth'), { recursive: true });
  cpSync(diagramsPath, join(outputPath, 'architecture'), { recursive: true });
  writeFileSync(join(outputPath, '.nojekyll'), '');
  writeFileSync(join(outputPath, 'build-info.json'), `${JSON.stringify({ revision })}\n`);
  writeFileSync(
    join(outputPath, 'index.html'),
    `<!doctype html>
<html lang="ru">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <meta name="referrer" content="no-referrer" />
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'self'; base-uri 'none'; form-action 'none'" />
    <title>PROTEUS</title>
    <link rel="stylesheet" href="./site.css" />
  </head>
  <body>
    <main>
      <p class="eyebrow">ONE TASK. MANY MINDS.</p>
      <h1>PROTEUS</h1>
      <p>Лаборатория персон</p>
      <nav aria-label="Разделы PROTEUS">
        <a href="./auth/">Подключение VK</a>
        <a href="./architecture/">Архитектура проекта</a>
      </nav>
    </main>
  </body>
</html>
`,
  );
  writeFileSync(
    join(outputPath, 'site.css'),
    `:root{color-scheme:dark;font-family:system-ui,sans-serif;background:#101316;color:#eef4f4}body{margin:0;min-height:100svh;display:grid;place-items:center}main{max-width:40rem;padding:2rem}.eyebrow{font-size:.75rem;letter-spacing:.15em;color:#67d5d1}h1{font-size:clamp(2rem,8vw,4rem);margin:.6rem 0}nav{display:flex;flex-wrap:wrap;gap:1rem;margin-top:2rem}a{color:#9eebdf;text-underline-offset:.25rem}a:nth-child(2){color:#c6abff}a:focus-visible{outline:2px solid currentColor;outline-offset:6px}\n`,
  );
}
