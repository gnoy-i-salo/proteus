import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
async function nativeFixture(page: Page, cancelMode = false) {
  await page.addInitScript(
    ({ cancelMode }) => {
      let rejectRun: ((error: Error) => void) | undefined;
      const calls: string[] = [];
      Reflect.set(window, 'isTauri', true);
      Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
      Reflect.set(window, '__TAURI_INTERNALS__', {
        transformCallback: () => 1,
        async invoke(
          command: string,
          args: { request?: { 'system-prompt': string; provider: string }; data?: string },
        ) {
          if (command === 'cli_status')
            return [
              { provider: 'codex', available: true },
              { provider: 'kimi', available: true },
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
          const request = args.request;
          if (!request) throw new Error('Missing request');
          calls.push(request['system-prompt'].split('\n')[0] ?? '');
          Reflect.set(window, 'stageCalls', calls);
          if (cancelMode)
            return new Promise((_resolve, reject) => {
              rejectRun = reject;
            });
          await new Promise((resolve) => setTimeout(resolve, 120));
          const data = request['system-prompt'].startsWith('STALKER')
            ? {
                evidence: [
                  {
                    id: 'e1',
                    'source-url': '',
                    kind: 'hypothesis',
                    summary: 'User-supplied context',
                  },
                ],
                'estimated-bsc': 2,
                effort: 'medium',
                character: 'Careful',
                'thinking-style': 'Structured',
                activity: 'Planning',
                'behavior-summary': 'Plans before acting',
              }
            : request['system-prompt'].startsWith('PERSON_FABRIC')
              ? { archetype: 'Structured planner', skills: ['Compare options before choosing'] }
              : request['system-prompt'].startsWith('BRAIN_ANALYZATOR')
                ? {
                    summary: 'Описание результатов.\n'.repeat(60),
                    'key-differences': ['Planning depth'],
                    people: [
                      {
                        strengths: ['Structure'],
                        weaknesses: ['Speed'],
                        observations: ['Methodical'],
                      },
                      { strengths: ['Speed'], weaknesses: ['Detail'], observations: ['Flexible'] },
                    ],
                  }
                : {
                    answer: 'Длинный ответ для проверки пагинации. '.repeat(100),
                    summary: 'Three-zone plan',
                  };
          return {
            answer: JSON.stringify(data),
            trace: {
              provider: request.provider,
              'reasoning-ms': 60000,
              'terminal-ms': 0,
              summary: 'Completed',
            },
          };
        },
      });
    },
    { cancelMode },
  );
}
async function fillExperiment(page: Page) {
  await page.goto('/');
  await page.getByLabel('Контекст A', { exact: true }).fill('Careful planner');
  await page.getByLabel('Контекст B', { exact: true }).fill('Fast improviser');
  await page.getByLabel('Общая задача', { exact: true }).fill('Plan a library');
  await page.getByRole('button', { name: 'Начать эксперимент' }).click();
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
    await expect(page.getByRole('heading', { name: 'BSC × 1.00' })).toBeVisible();
    await noOverflow(page);
    await page.getByRole('button', { name: 'Ответ', exact: true }).click();
    await noOverflow(page);
    const navigation = page.getByRole('navigation', { name: 'Страницы текста' });
    await navigation.getByRole('button', { name: '→' }).click();
    await noOverflow(page);
    await expect(navigation).toContainText('2 /');
    expect(await page.evaluate(() => Reflect.get(window, 'stageCalls'))).toEqual([
      'STALKER',
      'PERSON_FABRIC',
      'PERSON',
      'STALKER',
      'PERSON_FABRIC',
      'PERSON',
      'BRAIN_ANALYZATOR',
    ]);
    expect(await page.evaluate(() => Reflect.get(window, 'savedExperiment').people.length)).toBe(2);
    await page.getByRole('button', { name: /Person A/ }).click();
    await page.getByRole('button', { name: 'Архетип', exact: true }).click();
    await expect(page.getByText('Structured planner', { exact: false })).toBeVisible();
  });
}
test('cancellation stops native execution and allows a new experiment', async ({ page }) => {
  await nativeFixture(page, true);
  await fillExperiment(page);
  await page.getByRole('button', { name: 'Остановить' }).click();
  await expect(page.getByRole('alert')).toContainText('остановлен');
  await page.getByRole('button', { name: '← Новый запуск' }).click();
  await expect(page.getByRole('button', { name: 'Начать эксперимент' })).toBeVisible();
  expect(await page.evaluate(() => Reflect.get(window, 'stageCalls').length)).toBe(1);
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
