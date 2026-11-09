import ts from 'typescript';
import assert from 'node:assert/strict';
import test from 'node:test';
import { fieldNamingErrors, publicEntryErrors, uiBoundaryErrors } from './code-rules.mjs';

const domain = 'packages/contracts/src/persona.ts';
const ui = 'apps/desktop/src/features/example/ui/example.tsx';

test('own TypeScript fields reject quoted kebab, snake and computed literal spellings', () => {
  for (const source of [
    "export interface Answer { 'answer-text': string }",
    "export const answer = { 'answer-text': 'exact words' };",
    'export class Answer { answer_text = "exact"; }',
    "export const answer = { ['answer-text']: 'exact' };",
    'export interface Answer { answer_text(): string }',
  ])
    assert.ok(fieldNamingErrors(source, domain).length > 0, source);
});

test('AST field checks preserve source strings and HTML/framework attributes', () => {
  assert.deepEqual(
    fieldNamingErrors(
      `
    export interface Answer { answerText: string; readEpisode(): void; }
    const exactText = '{"answer-text":"raw-provider-text"}';
    export const answer = { answerText: exactText };
    export const node = <div aria-label="answer-text" data-source-id="raw_key" />;
  `,
      'apps/desktop/src/shared/ui/answer.tsx',
    ),
    [],
  );
});

test('external and legacy exceptions are limited to the exact file and exact field', () => {
  const legacy = 'packages/contracts/src/source-document-codec.test.ts';
  const external = 'packages/contracts/src/auth-callback.ts';
  assert.deepEqual(fieldNamingErrors("export const value = { 'author-id': 'exact' };", legacy), []);
  assert.deepEqual(
    fieldNamingErrors('export interface Callback { device_id: string }', external),
    [],
  );
  assert.ok(fieldNamingErrors("export const value = { 'answer-text': 'bad' };", legacy).length);
  assert.ok(fieldNamingErrors('export interface Callback { device_id: string }', domain).length);
});

test('public entries contain reexports only, including type exports', () => {
  assert.deepEqual(
    publicEntryErrors(
      "export { answer } from './answer'; export type { Answer } from './answer';",
      'packages/person/src/index.ts',
    ),
    [],
  );
  for (const source of [
    'export function answer() { return 1; }',
    'export const answer = 1;',
    "import './initialize'; export { answer } from './answer';",
    'export interface Answer { text: string }',
    "export * from './answer';",
  ])
    assert.ok(publicEntryErrors(source, 'packages/person/src/index.ts').length, source);
  assert.deepEqual(
    publicEntryErrors('export const answer = 1;', 'packages/person/src/answer.ts'),
    [],
  );
});

test('UI cannot import native commands, provider orchestration or persistence adapters', () => {
  for (const source of [
    "import { invoke } from '@tauri-apps/api/core';",
    "import { runExperiment } from '@proteus-int/pipeline';",
    "import { createCliProvider } from '../../../shared/lib/cli';",
    "export { createSourceGrant } from '../../../shared/lib/sources';",
    "const module = import('../../../shared/lib/source-bindings');",
    "const module = require('../../../shared/lib/presets');",
    "import { controlWindow as control } from '../../../shared/lib/window-controls';",
    "import * as controls from '../../../shared/lib/window-controls';",
  ])
    assert.ok(uiBoundaryErrors(source, ui).length, source);
});

test('UI may compose models, use pure selectors and import boundary types only', () => {
  for (const source of [
    "import { createExample } from '../model/example';",
    "import type { CliStatus } from '../../../shared/lib/cli';",
    "import type { StageTrace } from '@proteus-int/pipeline';",
    "import { isDesktop, windowPlatform, windowActions } from '../../../shared/lib/window-controls';",
    "import { componentIds } from '@proteus-int/intelligence';",
  ])
    assert.deepEqual(uiBoundaryErrors(source, ui), [], source);
  assert.deepEqual(
    uiBoundaryErrors(
      "import { invoke } from '@tauri-apps/api/core';",
      'apps/desktop/src/shared/lib/cli.ts',
    ),
    [],
  );
});

test('private fields and asserted static computed keys cannot hide old owned naming', () => {
  assert.ok(fieldNamingErrors('export class Value { #old_name = 1; }', domain).length);
  assert.ok(fieldNamingErrors("export const value = { ['old-name' as const]: 1 };", domain).length);
  assert.deepEqual(fieldNamingErrors('export class Value { #newName = 1; }', domain), []);
});

test('UI adapter boundary also covers TypeScript import-equals syntax', () => {
  assert.ok(uiBoundaryErrors("import cli = require('../../../shared/lib/cli');", ui).length);
  assert.deepEqual(
    uiBoundaryErrors("import type cli = require('../../../shared/lib/cli');", ui),
    [],
  );
});

test('UI intelligence imports are limited to pure component/provider selectors', () => {
  assert.ok(
    uiBoundaryErrors("import { createUnconfiguredProvider } from '@proteus-int/intelligence';", ui)
      .length,
  );
  assert.ok(
    uiBoundaryErrors("import * as intelligence from '@proteus-int/intelligence';", ui).length,
  );
  assert.deepEqual(
    uiBoundaryErrors("import { componentIds, isProviderId } from '@proteus-int/intelligence';", ui),
    [],
  );
});

test('empty named imports still evaluate adapters; explicit type-only imports do not', () => {
  for (const source of [
    "import {} from '@tauri-apps/api/core';",
    "export {} from '../../../shared/lib/cli';",
  ])
    assert.ok(uiBoundaryErrors(source, ui).length, source);
  for (const source of ["import type {} from '@tauri-apps/api/core';"])
    assert.deepEqual(uiBoundaryErrors(source, ui), [], source);
});

test('inline-only type specifiers still emit an adapter import under verbatimModuleSyntax', () => {
  for (const source of [
    "import { type CliStatus } from '../../../shared/lib/cli';",
    "export { type CliStatus } from '../../../shared/lib/cli';",
  ]) {
    const output = ts.transpileModule(source, {
      compilerOptions: { module: ts.ModuleKind.ESNext, verbatimModuleSyntax: true },
    }).outputText;
    assert.match(output, /(?:import|export) \{\} from/);
    assert.ok(uiBoundaryErrors(source, ui).length, source);
  }
  assert.deepEqual(
    uiBoundaryErrors("export type { CliStatus } from '../../../shared/lib/cli';", ui),
    [],
  );
});

test('ingestion picker and job adapters stay behind UI scenario models', () => {
  for (const adapter of ['ingestion-input', 'ingestion']) {
    const source = `import { action } from '../../../shared/lib/${adapter}';`;
    assert.ok(uiBoundaryErrors(source, ui).length, adapter);
    assert.deepEqual(
      uiBoundaryErrors(source, 'apps/desktop/src/features/source-selection/model/import.ts'),
      [],
    );
    assert.deepEqual(
      uiBoundaryErrors(`import type { Input } from '../../../shared/lib/${adapter}';`, ui),
      [],
    );
  }
});

test('research, search and cancellable native adapters stay behind UI scenario models', () => {
  for (const adapter of ['research-memory', 'native-operation', 'local-model', 'corpus-search']) {
    const source = `import { action } from '../../../shared/lib/${adapter}';`;
    assert.ok(uiBoundaryErrors(source, ui).length, adapter);
    assert.deepEqual(
      uiBoundaryErrors(source, 'apps/desktop/src/features/research/model/research.ts'),
      [],
    );
    assert.deepEqual(
      uiBoundaryErrors(`import type { Input } from '../../../shared/lib/${adapter}';`, ui),
      [],
    );
  }
});
