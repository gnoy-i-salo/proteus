import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
interface ProtocolFixture {
  readonly protocolCalls: readonly string[];
}
async function installProtocolFixture(page: Page, count = 1) {
  await page.addInitScript(
    ({ count }) => {
      const calls: string[] = [];
      Reflect.set(window, 'protocolCalls', calls);
      Reflect.set(window, 'isTauri', true);
      Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
      Reflect.set(window, '__TAURI_INTERNALS__', {
        transformCallback: () => 1,
        invoke(command: string, args: { request?: { systemPrompt: string; task: string } }) {
          if (command === 'cli_status')
            return Promise.resolve([
              { provider: 'codex', available: true, path: '/synthetic/codex' },
              { provider: 'kimi', available: false, path: null },
            ]);
          if (command === 'take_pending_presets') return Promise.resolve([]);
          if (command !== 'run_cli') return Promise.resolve(1);
          const stage = args.request?.systemPrompt.split('\n')[0] ?? '';
          calls.push(stage);
          if (stage !== 'BRAIN_ANALYZATOR_RUBRIC')
            return Promise.reject(new Error('Synthetic stop after approval'));
          return Promise.resolve({
            answer: JSON.stringify({
              criteria: Array.from({ length: count }, (_, index) => ({
                id: `criterion-${index}`,
                label: `Критерий ${index + 1}`,
                weight: 0.0000001 * (index + 1),
                required: index === 0,
                anchors: {
                  unmet: `Нет результата ${index + 1}`,
                  partial: `Частичный результат ${index + 1}`,
                  met: `Полный результат ${index + 1}`,
                },
              })),
            }),
            trace: {
              provider: 'codex',
              reasoningMs: 0,
              wallTimeMs: 10,
              terminalMs: 0,
              summary: 'Критерии готовы',
              reasoning: [],
            },
          });
        },
      });
    },
    { count },
  );
}
async function enterTask(page: Page, scale = 1) {
  await page.goto('/');
  await page.addStyleTag({
    content: `:root { --reading:${14 * scale}px; --caption:${12 * scale}px; }`,
  });
  await page.getByRole('textbox', { name: 'Контекст A', exact: true }).fill('Контекст A');
  await page.getByRole('textbox', { name: 'Контекст B', exact: true }).fill('Контекст B');
  await page
    .getByRole('textbox', { name: 'Общая задача', exact: true })
    .fill('Точная общая задача');
  await page.getByRole('button', { name: 'Начать эксперимент' }).click();
}
const calls = (page: Page) =>
  page.evaluate(() => (window as unknown as ProtocolFixture).protocolCalls);
async function noOverflow(page: Page) {
  expect(
    await page.evaluate(() =>
      [...document.querySelectorAll<HTMLElement>('body, body *')]
        .filter((element) => {
          const rect = element.getBoundingClientRect();
          return (
            rect.width > 0 &&
            rect.height > 0 &&
            !['INPUT', 'SELECT', 'OPTION', 'SVG', 'PATH'].includes(element.tagName) &&
            (element.scrollHeight > element.clientHeight + 2 ||
              element.scrollWidth > element.clientWidth + 2)
          );
        })
        .map((element) => `${element.tagName}.${element.className}`),
    ),
  ).toEqual([]);
}
for (const [width, height, scale] of [
  [640, 600, 1],
  [1024, 768, 1],
  [1280, 800, 1],
  [640, 600, 1.25],
] as const) {
  for (const count of [1, 12]) {
    test(`structured approval exposes all ${count} criteria and anchors without running sources ${width}/${scale}`, async ({
      page,
    }) => {
      await page.setViewportSize({ width, height });
      await installProtocolFixture(page, count);
      await enterTask(page, scale);
      await expect(
        page.getByRole('heading', { name: 'Ожидает подтверждения', exact: true }),
      ).toBeVisible();
      await expect(page.getByText('LIVE EXPERIMENT', { exact: false })).toHaveCount(0);
      const criteria = page.getByRole('combobox', { name: 'Критерий оценки' });
      await expect(criteria.locator('option')).toHaveCount(count);
      for (let index = 1; index <= count; index++) {
        await criteria.selectOption(String(index - 1));
        await expect(page.getByTestId('criterion-weight')).toHaveText(String(0.0000001 * index));
        for (const [anchor, text] of [
          ['Не выполнено', 'Нет результата'],
          ['Частично', 'Частичный результат'],
          ['Выполнено', 'Полный результат'],
        ] as const) {
          await page.getByRole('tab', { name: anchor, exact: true }).click();
          await expect(page.getByRole('tabpanel', { name: anchor, exact: true })).toContainText(
            `${text} ${index}`,
          );
        }
      }
      await noOverflow(page);
      await page.screenshot({ path: test.info().outputPath('protocol.png') });
      expect(await calls(page)).toEqual(['BRAIN_ANALYZATOR_RUBRIC']);
      await page.getByRole('tab', { name: 'Задача', exact: true }).click();
      await expect(page.getByRole('tabpanel', { name: 'Задача', exact: true })).toContainText(
        'Точная общая задача',
      );
      await noOverflow(page);
      await page.getByRole('tab', { name: 'Участники', exact: true }).click();
      await expect(page.getByRole('tabpanel', { name: 'Участники', exact: true })).toContainText(
        'Контроль',
      );
      await noOverflow(page);
      await page.getByRole('tab', { name: 'Настройки', exact: true }).click();
      await expect(page.getByRole('tabpanel', { name: 'Настройки', exact: true })).toContainText(
        'Запрошенная модель',
      );
      await noOverflow(page);
      await page.getByRole('button', { name: 'Подтвердить и продолжить', exact: true }).click();
      await expect.poll(() => calls(page)).toContain('STALKER');
    });
  }
}

test('returning to the task cancels approval, keeps input and builds a fresh protocol after edits', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await installProtocolFixture(page);
  await enterTask(page);
  await expect(
    page.getByRole('heading', { name: 'Ожидает подтверждения', exact: true }),
  ).toBeVisible();
  await page.getByRole('button', { name: 'Вернуться к задаче', exact: true }).click();
  await expect(page.getByRole('textbox', { name: 'Общая задача', exact: true })).toHaveValue(
    'Точная общая задача',
  );
  await expect(page.getByRole('alert')).toHaveCount(0);
  expect(await calls(page)).toEqual(['BRAIN_ANALYZATOR_RUBRIC']);
  await page.getByRole('textbox', { name: 'Общая задача', exact: true }).fill('Новая задача');
  await page.getByRole('button', { name: 'Начать эксперимент' }).click();
  await expect(
    page.getByRole('heading', { name: 'Ожидает подтверждения', exact: true }),
  ).toBeVisible();
  await page.getByRole('tab', { name: 'Критерии', exact: true }).focus();
  await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('tab', { name: 'Задача', exact: true })).toBeFocused();
  await expect(page.getByRole('tabpanel', { name: 'Задача', exact: true })).toContainText(
    'Новая задача',
  );
  expect(await calls(page)).toEqual(['BRAIN_ANALYZATOR_RUBRIC', 'BRAIN_ANALYZATOR_RUBRIC']);
});
