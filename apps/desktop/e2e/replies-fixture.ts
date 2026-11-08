import { expect } from '@playwright/test';
import type { Page } from '@playwright/test';
import type { ExperimentResult } from '@proteus-int/pipeline';

export interface NativeFixtureWindow {
  readonly stageCalls: readonly string[];
  readonly savedExperiment: ExperimentResult;
}
interface FixtureModelInput {
  readonly archetype: string;
  readonly answers: readonly { label: string; answer: string }[];
  readonly rubric: { readonly criteria: readonly { id: string }[] };
}
export interface ReplyFixtureOptions {
  noReasoning?: boolean;
  unknown?: boolean;
  unknownOnce?: boolean;
  required?: boolean;
  longCriteria?: boolean;
  exactQuote?: string;
  failSolve?: boolean;
  messageReply?: boolean;
  customMessages?: readonly string[];
  failPreparation?: boolean;
  errorText?: string;
}

export async function nativeReplyFixture(page: Page, options: ReplyFixtureOptions = {}) {
  await page.addInitScript(
    ({ options }) => {
      let eventCallback: ((event: unknown) => void) | undefined;
      let sources = 0;
      let evaluations = 0;
      const calls: string[] = [];
      Reflect.set(window, 'isTauri', true);
      Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
      const actorFor = (stage: string, input: FixtureModelInput) =>
        stage === 'PERSON' || stage === 'PERSON_FABRIC_ADAPT'
          ? input.archetype.includes('Structured planner')
            ? 'A'
            : input.archetype.includes('Flexible planner')
              ? 'B'
              : 'Контроль'
          : stage === 'STALKER' || stage === 'PERSON_FABRIC'
            ? sources === 1
              ? 'A'
              : 'B'
            : 'Общее';
      const stageFailure = (stage: string) => {
        if (stage === 'STALKER' && options.failPreparation)
          return options.errorText ?? 'Fixture preparation failed';
        if (stage === 'PERSON' && options.failSolve) return 'Fixture CLI unavailable';
        return undefined;
      };
      const runFixture = async (
        request: { id: string; task: string; systemPrompt: string; provider: string } | undefined,
      ) => {
        if (!request) throw new Error('Missing request');
        const stage = request.systemPrompt.split('\n')[0] ?? '';
        calls.push(stage);
        Reflect.set(window, 'stageCalls', calls);
        const failure = stageFailure(stage);
        if (failure !== undefined) throw new Error(failure);
        const input = JSON.parse(request.task) as FixtureModelInput;
        if (stage === 'STALKER') sources++;
        const actor = actorFor(stage, input);
        const reasoning = options.noReasoning
          ? []
          : [
              {
                id: 'r1',
                kind: 'summary',
                text: `Персона ${actor}: Сравниваю ограничения и варианты.\n`.repeat(60),
              },
            ];
        if (reasoning.length)
          eventCallback?.({
            payload: {
              id: request.id,
              kind: 'reasoning',
              elapsedMs: 100,
              reasoning: reasoning[0],
            },
          });
        await new Promise((resolve) => setTimeout(resolve, 180));
        const rule = (id: string, instruction: string, evidenceIds: string[]) => ({
          id,
          instruction,
          evidenceIds: evidenceIds,
          status: 'hypothesis',
          limitations: ['Unverified user context'],
        });
        let data: unknown;
        if (stage === 'BRAIN_ANALYZATOR_RUBRIC')
          data = {
            criteria: (options.unknown ? [50, 30, 20] : [1]).map((weight, index) => ({
              id: `criterion-${index}`,
              label: `Полнота решения ${index + 1}${options.longCriteria ? ' · длинное название критерия'.repeat(40) : ''}`,
              weight,
              required: options.required ?? false,
              anchors: {
                unmet: 'Решения нет',
                partial: 'Решение частичное',
                met: 'Решение полное',
              },
            })),
          };
        else if (stage === 'STALKER')
          data = {
            evidence: [
              {
                id: 'e1',
                sourceUrl: '',
                kind: 'hypothesis',
                summary: actor === 'A' ? 'Careful planner' : 'Fast improviser',
                status: 'hypothesis',
                excerpt: null,
                limitations: ['Unverified user context'],
              },
            ],
            character: 'Unknown',
            thinkingStyle: 'Unknown',
            activity: 'Unknown',
            behaviorSummary: 'Unknown',
          };
        else if (stage === 'PERSON_FABRIC')
          data = {
            rules: [rule('r1', actor === 'A' ? 'Structured planner' : 'Flexible planner', ['e1'])],
          };
        else if (stage === 'PERSON_FABRIC_ADAPT')
          data = {
            skills: [
              rule('s1', 'Compare options before choosing', actor === 'Контроль' ? [] : ['e1']),
            ],
          };
        else if (stage === 'BRAIN_ANALYZATOR_EVALUATE') {
          evaluations++;
          data = {
            evaluations: input.answers.map((answer: { label: string; answer: string }) => ({
              label: answer.label,
              criteria: input.rubric.criteria.map((criterion: { id: string }, index: number) => ({
                criterionId: criterion.id,
                status:
                  options.unknownOnce && answer.answer.startsWith('Ответ A.')
                    ? evaluations <= 2
                      ? 'unknown'
                      : 'partial'
                    : options.unknown && answer.answer.startsWith('Ответ A.')
                      ? ['met', 'partial', 'unknown'][index]
                      : answer.answer.startsWith('Ответ A.')
                        ? 'met'
                        : 'partial',
                reason: 'Проверка содержания по заранее заданному критерию',
                quote: options.exactQuote ?? answer.answer.slice(0, answer.answer.indexOf('.') + 1),
              })),
            })),
          };
        } else
          data = {
            answer:
              `Ответ ${actor}. ${options.exactQuote ? `${options.exactQuote} ` : ''}Длинный ответ для проверки пагинации. `.repeat(
                100,
              ),
            ...(options.messageReply || options.customMessages
              ? {
                  messages: options.customMessages ?? [
                    `Ответ ${actor}.  Первая строка\n\n  отступ`,
                    '<b>Второе сообщение</b> 🙂',
                  ],
                  styleConflicts: [],
                }
              : {}),
            summary: 'Three-zone plan',
          };
        return {
          answer: JSON.stringify(data),
          trace: {
            provider: request.provider,
            wallTimeMs: actor === 'A' ? 60000 : 180000,
            reasoningMs: actor === 'A' ? 60000 : 180000,
            terminalMs: 0,
            summary: 'Completed',
            reasoning,
          },
        };
      };
      Reflect.set(window, '__TAURI_INTERNALS__', {
        transformCallback: (callback: (event: unknown) => void) => {
          eventCallback = callback;
          return 1;
        },
        async invoke(
          command: string,
          args: {
            request?: { id: string; task: string; systemPrompt: string; provider: string };
            data?: string;
            event?: string;
          },
        ) {
          if (command === 'take_pending_presets') return [];
          if (command === 'cli_status')
            return [
              { provider: 'codex', available: true, path: '/test/bin/codex' },
              { provider: 'kimi', available: true, path: '/test/bin/kimi' },
            ];
          if (command === 'cancel_cli') return;
          if (command === 'save_experiment') {
            Reflect.set(window, 'savedExperiment', JSON.parse(args.data ?? '{}'));
            return '/test/experiments/fixture';
          }
          if (command !== 'run_cli') return 1;
          return await runFixture(args.request);
        },
      });
    },
    { options },
  );
}

export async function fillReplyExperiment(page: Page, repetitions = '1') {
  await page.goto('/');
  await page.getByLabel('Контекст A', { exact: true }).fill('Careful planner');
  await page.getByLabel('Контекст B', { exact: true }).fill('Fast improviser');
  await page.getByLabel('Общая задача', { exact: true }).fill('Plan a library');
  if (repetitions !== '1')
    await page.getByLabel('Режим эксперимента', { exact: true }).selectOption(repetitions);
  await page.getByRole('button', { name: 'Начать эксперимент' }).click();
  await page.getByRole('button', { name: 'Подтвердить и продолжить', exact: true }).click();
}

export async function noReplyOverflow(page: Page) {
  expect(
    await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('body, body *')]
        .filter((el) => {
          const rect = el.getBoundingClientRect();
          if (
            !rect.width ||
            !rect.height ||
            ['INPUT', 'SELECT', 'OPTION', 'SVG', 'PATH'].includes(el.tagName)
          )
            return false;
          return el.scrollHeight > el.clientHeight + 2 || el.scrollWidth > el.clientWidth + 2;
        })
        .map((el) => `${el.tagName}.${el.className}`),
    ),
  ).toEqual([]);
}
