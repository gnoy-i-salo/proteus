import { readFileSync } from 'node:fs';
import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import { fillReplyExperiment, nativeReplyFixture } from './replies-fixture';
const fixture = (name: string) =>
  readFileSync(
    new URL(`../../../packages/pipeline/src/testing/legacy-results/${name}`, import.meta.url),
    'utf8',
  );
const modern = fixture('v4-current-with-execution.json');
const original = fixture('v1-unversioned-result.json');

async function historyFixture(page: Page, records: Record<string, string>, failList = false) {
  await page.addInitScript(
    ({ records, failList }) => {
      const commands: string[] = [];
      Reflect.set(window, 'historyCommands', commands);
      Reflect.set(window, 'isTauri', true);
      Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
      Reflect.set(window, '__TAURI_INTERNALS__', {
        transformCallback: () => 1,
        invoke(command: string, args: { id?: string; offset?: number; limit?: number } = {}) {
          commands.push(command);
          if (command === 'cli_status') return Promise.resolve([]);
          if (command === 'take_pending_presets') return Promise.resolve([]);
          if (command === 'list_saved_experiments') {
            if (failList) return Promise.reject(new Error('Private disk path must not be exposed'));
            const ids = Object.keys(records).sort();
            const offset = args.offset ?? 0;
            const entries = ids
              .slice(offset, offset + (args.limit ?? 50))
              .map((id) => ({ id, ok: true, error: null }));
            return Promise.resolve({
              schemaVersion: 1,
              entries,
              matched: ids.length,
              nextOffset: offset + entries.length < ids.length ? offset + entries.length : null,
              scanTruncated: false,
            });
          }
          if (command === 'read_saved_experiment')
            return Promise.resolve({ schemaVersion: 1, id: args.id, text: records[args.id ?? ''] });
          if (
            command === 'run_cli' ||
            command.includes('grant') ||
            command.includes('source_binding')
          )
            return Promise.reject(new Error('History must not start providers or grant access'));
          return Promise.resolve(1);
        },
      });
    },
    { records, failList },
  );
}
async function noProviderCalls(page: Page) {
  const commands = await page.evaluate(() => Reflect.get(window, 'historyCommands') as string[]);
  expect(
    commands.filter(
      (command) =>
        command === 'run_cli' || command.includes('grant') || command.includes('source_binding'),
    ),
  ).toEqual([]);
}

test('reopens saved versions after restart, retains a good result on corruption and keeps the draft form', async ({
  page,
}) => {
  await historyFixture(
    page,
    Object.fromEntries([
      ['a-modern', modern],
      ['b-corrupt', '{}'],
      ['c-original', original],
    ]),
  );
  await page.goto('/');
  await page.getByLabel('Имя персоны A').fill('Оставить мой черновик');
  await page.getByRole('button', { name: 'История запусков', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть a-modern', exact: true }).click();
  await expect(page.getByText('Версия файла: 4', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100', exact: true })).toBeVisible();
  await page.getByText('Источники и ограничения сохранения', { exact: true }).click();
  await expect(
    page.getByText('Доступ к исходникам сейчас не проверен.', { exact: false }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Открыть b-corrupt', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Не удалось открыть результат');
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100', exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Открыть c-original', exact: true }).click();
  await expect(page.getByText('Версия файла: 1', { exact: true })).toBeVisible();
  await expect(page.getByRole('heading', { name: 'Ранний BSC', exact: true })).toBeVisible();
  await expect(
    page.getByText('Эти значения несопоставимы с BSC v2.', { exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Вернуться к эксперименту', exact: true }).click();
  await expect(page.getByLabel('Имя персоны A')).toHaveValue('Оставить мой черновик');
  await noProviderCalls(page);
  await page.reload();
  await page.getByRole('button', { name: 'История запусков', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть a-modern', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100', exact: true })).toBeVisible();
  await noProviderCalls(page);
});

test('copies conditions only after explicit choice without starting or restoring source permissions', async ({
  page,
}) => {
  await historyFixture(page, { saved: modern });
  await page.goto('/');
  await page.getByRole('button', { name: 'История запусков', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть saved', exact: true }).click();
  await page.getByRole('button', { name: 'Новый запуск из этих условий', exact: true }).click();
  await expect(page.getByLabel('Имя персоны A')).toHaveValue('Ada Sample');
  await expect(page.getByLabel('Общая задача', { exact: true })).toHaveValue(
    (JSON.parse(modern) as { input: { task: string } }).input.task,
  );
  await expect(
    page.getByRole('button', { name: 'Источники персоны A', exact: true }),
  ).toContainText('0');
  await expect(page.getByRole('button', { name: 'Начать эксперимент', exact: true })).toBeVisible();
  await noProviderCalls(page);
});

for (const fails of [false, true])
  test(`history shows ${fails ? 'read failure' : 'empty state'} without changing the form`, async ({
    page,
  }) => {
    await historyFixture(page, {}, fails);
    await page.goto('/');
    await page.getByRole('button', { name: 'История запусков', exact: true }).click();
    await expect(
      page.getByText(
        fails
          ? 'Не удалось прочитать историю. Попробуйте обновить список.'
          : 'Сохранённых запусков пока нет.',
        { exact: true },
      ),
    ).toBeVisible();
    await expect(page.getByRole('button', { name: 'Обновить историю', exact: true })).toBeVisible();
    await noProviderCalls(page);
  });

test('history stays readable at a narrow width, keeps persona colors and does not offer raw export', async ({
  page,
}, testInfo) => {
  await historyFixture(page, { saved: modern });
  await page.setViewportSize({ width: 640, height: 700 });
  await page.goto('/');
  await page.getByRole('button', { name: 'История запусков', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть saved', exact: true }).click();
  await page.getByRole('button', { name: 'Boris Fixture', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'BSC v2 · 75', exact: true })).toBeVisible();
  expect(
    await page
      .locator('[data-palette="violet"]')
      .evaluate((element) => getComputedStyle(element).getPropertyValue('--accent').trim()),
  ).toBe('#c49cff');
  await expect(page.getByRole('button', { name: '↓ JSON', exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: testInfo.outputPath('history-narrow.png') });
  await noProviderCalls(page);
});

test('history pages replace the visible list and retain a selected result', async ({ page }) => {
  const entries = Object.fromEntries(
    Array.from({ length: 51 }, (_, index) => [`run-${String(index).padStart(2, '0')}`, modern]),
  );
  await historyFixture(page, entries);
  await page.goto('/');
  await page.getByRole('button', { name: 'История запусков', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть run-00', exact: true }).click();
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100', exact: true })).toBeVisible();
  await page
    .getByRole('navigation', { name: 'Страницы истории' })
    .getByRole('button', { name: 'Далее', exact: true })
    .click();
  await expect(page.getByRole('button', { name: 'Открыть run-50', exact: true })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Открыть run-00', exact: true })).toHaveCount(0);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100', exact: true })).toBeVisible();
  await page
    .getByRole('navigation', { name: 'Страницы истории' })
    .getByRole('button', { name: 'Назад', exact: true })
    .click();
  await expect(page.getByRole('button', { name: 'Открыть run-00', exact: true })).toBeVisible();
  await noProviderCalls(page);
});

for (const status of ['incomplete', 'cancelled'] as const)
  test(`history retains an actual ${status} saved run`, async ({ page, context }) => {
    await nativeReplyFixture(page, { failSolve: status === 'incomplete' });
    await fillReplyExperiment(page);
    if (status === 'cancelled') {
      await expect
        .poll(() =>
          page.evaluate(
            () =>
              (Reflect.get(window, 'stageCalls') as string[] | undefined)?.includes('PERSON') ??
              false,
          ),
        )
        .toBe(true);
      await page.getByRole('button', { name: 'Остановить', exact: true }).click();
    }
    await page.waitForFunction(() => Reflect.has(window, 'savedExperiment'));
    const saved = await page.evaluate(() => JSON.stringify(Reflect.get(window, 'savedExperiment')));
    expect((JSON.parse(saved) as { status: string }).status).toBe(status);
    const reopened = await context.newPage();
    await historyFixture(reopened, { saved });
    await reopened.goto('/');
    await reopened.getByRole('button', { name: 'История запусков', exact: true }).click();
    await reopened.getByRole('button', { name: 'Открыть saved', exact: true }).click();
    await expect(
      reopened.getByText(`Статус: ${status === 'cancelled' ? 'отменён' : 'не завершён'}`, {
        exact: true,
      }),
    ).toBeVisible();
    await expect(
      reopened.getByRole('heading', { name: 'BSC v2 · не оценено', exact: true }),
    ).toBeVisible();
    await noProviderCalls(reopened);
    await reopened.close();
  });
