import { expect, test } from '@playwright/test';
import type { Locator, Page } from '@playwright/test';
import type { ExperimentResult } from '@proteus-int/pipeline';
interface NativeFixtureWindow {
  readonly stageCalls: readonly string[];
  readonly savedExperiment: ExperimentResult;
  openPresetFile(data: string): void;
}
interface FixtureModelInput {
  readonly archetype: string;
  readonly answers: readonly { label: string; answer: string }[];
  readonly rubric: { readonly criteria: readonly { id: string }[] };
}

async function nativeFixture(
  page: Page,
  cancelMode = false,
  reasoningAvailable = true,
  options: {
    unknown?: boolean;
    unknownOnce?: boolean;
    failSolve?: boolean;
    messageReply?: boolean;
    holdStage?: string;
    failPreparation?: boolean;
    errorText?: string;
    delayedEvent?: boolean;
  } = {},
) {
  await page.addInitScript(
    ({ cancelMode, reasoningAvailable, options }) => {
      let eventCallback: ((event: unknown) => void) | undefined;
      let presetCallback: ((event: unknown) => void) | undefined;
      const pendingPresets: { Ok: string }[] = [];
      Reflect.set(window, 'openPresetFile', (data: string) => {
        pendingPresets.push({ Ok: data });
        presetCallback?.({ payload: null });
      });
      let rejectRun: ((error: Error) => void) | undefined;
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
      const waitForCancellation = (id: string) => {
        if (options.delayedEvent)
          setTimeout(
            () => eventCallback?.({ payload: { id, kind: 'started', elapsedMs: 25 } }),
            500,
          );
        return new Promise<never>((_resolve, reject) => {
          rejectRun = reject;
        });
      };
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
        if (cancelMode || options.holdStage === stage) return waitForCancellation(request.id);
        const failure = stageFailure(stage);
        if (failure !== undefined) throw new Error(failure);
        const input = JSON.parse(request.task) as FixtureModelInput;
        if (stage === 'STALKER') sources++;
        const actor = actorFor(stage, input);
        const reasoning = reasoningAvailable
          ? [
              {
                id: 'r1',
                kind: 'summary',
                text: `Персона ${actor}: Сравниваю ограничения и варианты.\n`.repeat(60),
              },
            ]
          : [];
        if (reasoningAvailable)
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
              label: `Полнота решения ${index + 1}`,
              weight,
              required: false,
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
                quote: answer.answer.slice(0, answer.answer.indexOf('.') + 1),
              })),
            })),
          };
        } else
          data = {
            answer: `Ответ ${actor}. Длинный ответ для проверки пагинации. `.repeat(100),
            ...(options.messageReply
              ? {
                  messages: [
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
          if (command === 'plugin:event|listen' && args.event === 'preset-opened')
            presetCallback = eventCallback;
          if (command === 'take_pending_presets') return pendingPresets.splice(0);
          if (command === 'cli_status')
            return [
              { provider: 'codex', available: true, path: '/test/bin/codex' },
              { provider: 'kimi', available: true, path: '/test/bin/kimi' },
            ];
          if (command === 'cancel_cli') {
            rejectRun?.(new Error('cancelled'));
            return;
          }
          if (command === 'save_experiment') {
            Reflect.set(window, 'savedExperiment', JSON.parse(args.data ?? '{}'));
            return '/test/experiments/fixture';
          }
          if (command !== 'run_cli') return 1;
          return await runFixture(args.request);
        },
      });
    },
    { cancelMode, reasoningAvailable, options },
  );
}
async function confirmProtocol(page: Page) {
  await page.getByRole('button', { name: 'Подтвердить и продолжить', exact: true }).click();
}

test('persona replies preserve separate messages and render markup as plain text', async ({
  page,
}) => {
  await nativeFixture(page, false, true, { messageReply: true });
  await fillExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await expect(page.getByText('Поведенческая симуляция', { exact: true })).toBeVisible();
  await expect(page.getByText('Прогон 1 · Сообщение 1 / 2', { exact: true })).toBeVisible();
  const body = page.getByTestId('persona-reply-body');
  await expect(body.locator('p')).toHaveText('Ответ A.  Первая строка\n\n  отступ', {
    useInnerText: false,
  });
  await page.getByRole('button', { name: 'Следующее сообщение', exact: true }).click();
  await expect(body.locator('p')).toHaveText('<b>Второе сообщение</b> 🙂');
  await expect(body.locator('b')).toHaveCount(0);
  await expect(body).not.toContainText('симуляция');
});
async function fillExperiment(page: Page, confirm = true, repetitions = '1') {
  await page.goto('/');
  await page.getByLabel('Контекст A', { exact: true }).fill('Careful planner');
  await page.getByLabel('Контекст B', { exact: true }).fill('Fast improviser');
  await page.getByLabel('Общая задача', { exact: true }).fill('Plan a library');
  if (repetitions !== '1')
    await page.getByLabel('Режим эксперимента', { exact: true }).selectOption(repetitions);
  await page.getByRole('button', { name: 'Начать эксперимент' }).click();
  if (confirm) await confirmProtocol(page);
}
async function noOverflow(page: Page) {
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
async function openStage(logs: Locator, name: RegExp) {
  for (let index = 0; index < 8; index++) {
    const button = logs.getByRole('button', { name });
    if (await button.count()) {
      await button.click();
      return;
    }
    const next = logs
      .getByRole('navigation', { name: 'Страницы этапов', exact: true })
      .getByRole('button', { name: '→', exact: true });
    await expect(next).toBeEnabled();
    await next.click();
  }
  throw new Error(`Missing trace stage ${name}`);
}

test('rubric review freezes the protocol before sources and answers are requested', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeFixture(page);
  await fillExperiment(page, false);
  await expect(
    page.getByRole('button', { name: 'Подтвердить и продолжить', exact: true }),
  ).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as NativeFixtureWindow).stageCalls)).toEqual([
    'BRAIN_ANALYZATOR_RUBRIC',
  ]);
  await noOverflow(page);
  await confirmProtocol(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
});

test('cancelling protocol review prevents all profile and answer requests', async ({ page }) => {
  await nativeFixture(page);
  await fillExperiment(page, false);
  await expect(
    page.getByRole('button', { name: 'Подтвердить и продолжить', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Вернуться к задаче', exact: true }).click();
  await expect(page.getByRole('textbox', { name: 'Общая задача', exact: true })).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as NativeFixtureWindow).stageCalls)).toEqual([
    'BRAIN_ANALYZATOR_RUBRIC',
  ]);
});

test('unknown criteria retain their weight and display a BSC interval', async ({ page }) => {
  await nativeFixture(page, false, true, { unknown: true });
  await fillExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 65–85' })).toBeVisible();
  expect(
    await page.evaluate(
      () => (window as unknown as NativeFixtureWindow).savedExperiment.people[0]!.score.interval,
    ),
  ).toEqual({ lower: 65, upper: 85 });
  expect(
    await page.evaluate(
      () =>
        (window as unknown as NativeFixtureWindow).savedExperiment.comparison.comparisonEligible,
    ),
  ).toBe(false);
});

test('failed solution remains incomplete and unscored instead of receiving zero', async ({
  page,
}) => {
  await nativeFixture(page, false, true, { failSolve: true });
  await fillExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · не оценено' })).toBeVisible();
  expect(
    await page.evaluate(() => (window as unknown as NativeFixtureWindow).savedExperiment.status),
  ).toBe('incomplete');
  expect(
    await page.evaluate(
      () => (window as unknown as NativeFixtureWindow).savedExperiment.people[0]!.score.interval,
    ),
  ).toBeNull();
  expect(
    await page.evaluate(
      () => (window as unknown as NativeFixtureWindow).savedExperiment.people[0]!.score.evaluated,
    ),
  ).toBe(0);
  expect(
    await page.evaluate(
      () =>
        (window as unknown as NativeFixtureWindow).savedExperiment.comparison.comparisonEligible,
    ),
  ).toBe(false);
  await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
  const logs = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await openStage(logs, /A · Решение/);
  const failedStage = logs.getByRole('button', { name: /A · Решение/ });
  await expect(failedStage).toContainText('!');
  await expect(failedStage).not.toContainText('✓');
});

test('a collapsed median still displays uncertainty from an unknown trial', async ({ page }) => {
  await nativeFixture(page, false, true, { unknownOnce: true });
  await fillExperiment(page, true, '3');
  await expect(page.getByRole('heading', { name: 'BSC v2 · ≈50' })).toBeVisible({ timeout: 15000 });
  const score = await page.evaluate(
    () => (window as unknown as NativeFixtureWindow).savedExperiment.people[0]!.score,
  );
  expect(score.interval).toEqual({ lower: 50, upper: 50 });
  expect(score.point).toBeNull();
  expect(score.comparisonEligible).toBe(false);
});

test('three repetitions preserve the frozen profiles and include matched neutral answers', async ({
  page,
}) => {
  await nativeFixture(page);
  await fillExperiment(page, true, '3');
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible({ timeout: 15000 });
  expect(
    await page.evaluate(() => (window as unknown as NativeFixtureWindow).stageCalls.length),
  ).toBe(23);
  expect(
    await page.evaluate(
      () => (window as unknown as NativeFixtureWindow).savedExperiment.trials.length,
    ),
  ).toBe(9);
  expect(
    await page.evaluate(
      () => (window as unknown as NativeFixtureWindow).savedExperiment.control.score.evaluated,
    ),
  ).toBe(3);
  expect(
    await page.evaluate(
      () =>
        (window as unknown as NativeFixtureWindow).savedExperiment.people[0]!.baselineDifference,
    ),
  ).toEqual({ lower: 50, upper: 50 });
  expect(
    await page.evaluate(
      () =>
        (window as unknown as NativeFixtureWindow).savedExperiment.comparison.comparisonEligible,
    ),
  ).toBe(true);
});
for (const [width, height] of [
  [1440, 900],
  [640, 600],
] as const) {
  test(`complete native-contract flow and paginated results ${width}×${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await nativeFixture(page);
    await fillExperiment(page);
    await expect(page.getByRole('heading', { name: 'Intelligence traces_' })).toBeVisible();
    await noOverflow(page);
    await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
    await noOverflow(page);
    await page.getByRole('button', { name: 'Ответ', exact: true }).click();
    await noOverflow(page);
    const navigation = page.getByRole('navigation', { name: 'Страницы текста' });
    await navigation.getByRole('button', { name: 'Следующая страница текста' }).click();
    await noOverflow(page);
    await expect(navigation).toContainText('2 /');
    expect(
      await page.evaluate(() => (window as unknown as NativeFixtureWindow).stageCalls),
    ).toEqual([
      'BRAIN_ANALYZATOR_RUBRIC',
      'STALKER',
      'PERSON_FABRIC',
      'STALKER',
      'PERSON_FABRIC',
      'PERSON_FABRIC_ADAPT',
      'PERSON_FABRIC_ADAPT',
      'PERSON_FABRIC_ADAPT',
      'PERSON',
      'PERSON',
      'PERSON',
      'BRAIN_ANALYZATOR_EVALUATE',
      'BRAIN_ANALYZATOR_EVALUATE',
    ]);
    expect(
      await page.evaluate(
        () => (window as unknown as NativeFixtureWindow).savedExperiment.people.length,
      ),
    ).toBe(2);
    await page.getByRole('button', { name: /Person A/ }).click();
    await page.getByRole('button', { name: 'Архетип', exact: true }).click();
    await expect(page.getByText('Structured planner', { exact: false })).toBeVisible();
  });
}
test('cancellation stops native execution and allows a new experiment', async ({ page }) => {
  await nativeFixture(page, true);
  await fillExperiment(page, false);
  await page.getByRole('button', { name: 'Остановить' }).click();
  await expect(page.getByRole('alert')).toContainText('остановлен');
  await page.getByRole('button', { name: '← Новый запуск' }).click();
  await expect(page.getByRole('button', { name: 'Начать эксперимент' })).toBeVisible();
  expect(
    await page.evaluate(() => (window as unknown as NativeFixtureWindow).stageCalls.length),
  ).toBe(1);
});
test('pasted paragraphs are paginated without a hidden scroll area', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await page.goto('/');
  await page.getByLabel('Контекст A', { exact: true }).fill('Строка контекста\n'.repeat(30));
  await page
    .getByLabel('Общая задача', { exact: true })
    .fill('Подробная задача для двух персон. '.repeat(20));
  await noOverflow(page);
  await expect(page.getByRole('button', { name: 'Назад: Контекст A' })).toBeEnabled();
});

for (const [width, height] of [
  [1440, 900],
  [640, 600],
] as const) {
  test(`reasoning is live, paginated and saved for all stages ${width}×${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await nativeFixture(page);
    await fillExperiment(page);
    const dialog = page.getByRole('region', { name: 'Текст Intelligence traces' });
    await expect(page.getByRole('button', { name: 'Текст reasoning', exact: true })).toHaveCount(0);
    await expect(dialog).toContainText('Сравниваю ограничения');
    await noOverflow(page);
    await page.screenshot({ path: test.info().outputPath('inline-traces.png') });
    const navigation = dialog.getByRole('navigation', { name: 'Страницы текста' });
    await navigation.getByRole('button', { name: 'Следующая страница текста' }).click();
    await expect(navigation).toContainText('2 /');
    await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
    await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
    await expect(dialog.getByLabel('Этап reasoning')).toHaveCount(0);
    await expect(dialog.getByRole('article')).toHaveCount(3);
    expect(
      await page.evaluate(() =>
        (window as unknown as NativeFixtureWindow).savedExperiment.traces.every(
          (item) => item.trace.reasoning?.length === 1,
        ),
      ),
    ).toBe(true);
    await noOverflow(page);
  });
}
test('missing reasoning is explicit and never replaced with the answer', async ({ page }) => {
  await nativeFixture(page, false, false);
  await fillExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
  const dialog = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await expect(dialog).toContainText('CLI не передал текст reasoning');
  await expect(dialog).not.toContainText('Длинный ответ');
});

for (const [width, height] of [
  [1440, 900],
  [640, 600],
] as const) {
  test(`multiple public links can be edited, removed and passed intact ${width}×${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await nativeFixture(page);
    await page.goto('/');
    await page.getByLabel('Публичная ссылка A', { exact: true }).fill('https://example.org/a,b');
    for (let i = 2; i <= 5; i++) {
      await page.getByRole('button', { name: 'Добавить ссылку A', exact: true }).click();
      await page
        .getByLabel('Публичная ссылка A', { exact: true })
        .fill(`https://example.org/source-${i}`);
    }
    await expect(
      page.getByRole('button', { name: 'Добавить ссылку A', exact: true }),
    ).toBeDisabled();
    await noOverflow(page);
    await page.screenshot({ path: test.info().outputPath('public-links.png') });
    await page.getByRole('button', { name: 'Удалить ссылку A', exact: true }).click();
    await page.getByRole('button', { name: 'Предыдущая ссылка A', exact: true }).click();
    await expect(page.getByLabel('Публичная ссылка A', { exact: true })).toHaveValue(
      'https://example.org/source-3',
    );
    await page.getByLabel('Публичная ссылка B', { exact: true }).fill('https://example.org/b');
    await page.getByLabel('Общая задача', { exact: true }).fill('Plan a library');
    await page.getByRole('button', { name: 'Начать эксперимент' }).click();
    await confirmProtocol(page);
    await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
    expect(
      await page.evaluate(() =>
        (window as unknown as NativeFixtureWindow).savedExperiment.input.people.map(
          (person) => person.publicUrls,
        ),
      ),
    ).toEqual([
      [
        'https://example.org/a,b',
        'https://example.org/source-2',
        'https://example.org/source-3',
        'https://example.org/source-4',
      ],
      ['https://example.org/b'],
    ]);
  });
}

test('sidebar switches independent scores, answers and inline persona logs', async ({ page }) => {
  await nativeFixture(page);
  await fillExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await expect(page.getByText('Разница A − B: 50 п.п.')).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await expect(page.getByText(/Ответ A\./)).toBeVisible();
  await page.getByRole('button', { name: /Person B/ }).click();
  await expect(page.getByRole('heading', { name: 'BSC v2 · 50' })).toBeVisible();
  await expect(page.getByText(/Ответ B\./)).toBeVisible();
  await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
  const logs = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await expect(logs.getByRole('combobox')).toHaveCount(0);
  await openStage(logs, /B · Решение/);
  await expect(logs).toContainText('Персона B:');
  await expect(logs).not.toContainText('Персона A:');
  await page.getByRole('button', { name: /Person A/ }).click();
  await openStage(logs, /A · Решение/);
  await expect(logs).toContainText('Персона A:');
  await expect(logs).not.toContainText('Персона B:');
});

test('system preset opening during execution waits without changing the active people', async ({
  page,
}) => {
  await nativeFixture(page, true);
  await fillExperiment(page, false);
  await page.evaluate(() =>
    (window as unknown as NativeFixtureWindow).openPresetFile(
      JSON.stringify({
        format: 'proteus-personas',
        version: 1,
        task: 'Imported task',
        people: [
          { label: 'Imported A', context: 'Planner', 'public-urls': [] },
          { label: 'Imported B', context: 'Builder', 'public-urls': [] },
        ],
      }),
    ),
  );
  await expect(page.getByRole('status')).toContainText('ожидает');
  await expect(page.getByRole('button', { name: 'Открыть пресет', exact: true })).toBeDisabled();
  await expect(page.getByRole('button', { name: /Person A/ })).toBeVisible();
  await page.getByRole('button', { name: 'Остановить', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть пресет', exact: true }).click();
  await expect(page.getByLabel('Имя персоны A')).toHaveValue('Imported A');
  expect(
    await page.evaluate(() => (window as unknown as NativeFixtureWindow).stageCalls.length),
  ).toBe(1);
});

for (const [width, height] of [
  [640, 600],
  [1024, 768],
  [1280, 800],
] as const) {
  test(`expanded trace reader is readable, searchable and returns without changing text ${width}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await nativeFixture(page);
    await fillExperiment(page);
    await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
    await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
    const traces = page.getByRole('region', { name: 'Текст Intelligence traces' });
    await traces.getByRole('button', { name: 'Развернуть: Текст', exact: true }).click();
    const reader = page.getByRole('dialog', { name: 'Текст', exact: true });
    const before = await reader.locator('p').textContent();
    await reader.getByRole('textbox', { name: 'Поиск: Текст' }).fill('ограничения и варианты.');
    await expect(reader.locator('mark')).toHaveText('ограничения и варианты.');
    await reader.getByRole('button', { name: 'Следующее совпадение' }).click();
    await expect(reader.getByRole('status').filter({ hasText: '2 /' })).toBeVisible();
    await reader.getByRole('button', { name: 'Вернуться к месту' }).click();
    await expect(reader.locator('div[tabindex="0"]')).toBeFocused();
    await expect(reader.locator('p')).toHaveText(before!, { useInnerText: false });
    await noOverflow(page);
    const pageCount = await reader.getByRole('navigation').innerText();
    expect(Number(pageCount.match(/\/\s*(\d+)/)?.[1])).toBeLessThan(10);
    await reader.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Текст', exact: true })).toHaveCount(0);
    await expect(
      traces.getByRole('button', { name: 'Развернуть: Текст', exact: true }),
    ).toBeFocused();
  });
}

test('silent execution advances host time and cancellation removes active status', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeFixture(page, true);
  await fillExperiment(page, false);
  const status = page.getByRole('region', { name: 'Состояние запуска', exact: true });
  await expect(status).toContainText('Выполняется');
  await expect(status).toContainText('Brain Analyzator');
  await expect(status).toContainText('Система');
  await expect(status).toContainText('Событий CLI ещё не было');
  await expect(status.getByTestId('host-elapsed')).toHaveText(/[1-9]\d* с/, { timeout: 5000 });
  await expect(page.getByText('Завершено этапов: 0 / 13', { exact: true })).toBeVisible();
  await noOverflow(page);
  await page.getByRole('button', { name: 'Остановить', exact: true }).click();
  await expect(status).toContainText('Отменён');
  await expect(status).not.toContainText('LIVE');
  await expect(status.locator('[data-active="true"]')).toHaveCount(0);
  await expect(page.getByRole('button', { name: 'Остановить', exact: true })).toBeDisabled();
  await expect(status).toContainText('Сохранённые этапы доступны ниже');
});

test('failed preparation shows its cause and retained stage without active indicator', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeFixture(page, false, true, { failPreparation: true });
  await fillExperiment(page);
  const status = page.getByRole('region', { name: 'Состояние запуска', exact: true });
  await expect(status).toContainText('Ошибка этапа');
  await expect(status).toContainText('STALKER');
  await expect(status).toContainText('Fixture preparation failed');
  await expect(status.locator('[data-active="true"]')).toHaveCount(0);
  await expect(page.getByRole('region', { name: 'Текст Intelligence traces' })).toBeVisible();
  await noOverflow(page);
});

test('stopping preparation preserves its stage and a delayed event is separate from host time', async ({
  page,
}) => {
  await nativeFixture(page, false, false, { holdStage: 'STALKER', delayedEvent: true });
  await fillExperiment(page);
  const status = page.getByRole('region', { name: 'Состояние запуска', exact: true });
  await expect(status).toContainText('STALKER');
  await expect(status).toContainText('Последнее событие CLI:');
  await expect(page.getByRole('region', { name: 'Текст Intelligence traces' })).not.toContainText(
    'Модель рассуждает',
  );
  await page.getByRole('button', { name: 'Остановить', exact: true }).click();
  await expect(status).toContainText('Отменён');
  await expect(status).toContainText('STALKER');
  await expect(status.locator('[data-active="true"]')).toHaveCount(0);
});

test('incomplete result exposes the actual failed stage cause alongside retained results', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeFixture(page, false, true, { failSolve: true });
  await fillExperiment(page);
  const failure = page.getByRole('region', { name: 'Причина неполной серии', exact: true });
  await expect(failure).toContainText('Fixture CLI unavailable');
  await expect(failure).toContainText('Сохранённые ответы и этапы доступны');
  await expect(page.getByRole('button', { name: '↓ JSON', exact: true })).toBeVisible();
  await noOverflow(page);
});

test('long preparation failure remains fully readable without overflowing the compact window', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  const reason = '  Точная причина é 👩🏽‍🔬\r\n'.repeat(160) + 'КОНЕЦ_ПРИЧИНЫ';
  await nativeFixture(page, false, true, { failPreparation: true, errorText: reason });
  await fillExperiment(page);
  const status = page.getByRole('region', { name: 'Состояние запуска', exact: true });
  await expect(status).toContainText('Ошибка этапа');
  await noOverflow(page);
  await status.getByRole('button', { name: 'Развернуть: Причина', exact: true }).click();
  await expect(page.getByRole('dialog')).toBeVisible();
  await page.getByRole('textbox', { name: 'Поиск: Причина', exact: true }).fill('КОНЕЦ_ПРИЧИНЫ');
  await expect(page.getByRole('dialog')).toContainText('КОНЕЦ_ПРИЧИНЫ');
  await page.keyboard.press('Escape');
  await expect(
    status.getByRole('button', { name: 'Развернуть: Причина', exact: true }),
  ).toBeFocused();
});
