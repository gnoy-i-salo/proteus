import { expect, test } from '@playwright/test';

test('model settings keep label spacing and prose typography', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  const section = page.getByRole('region', { name: 'Настройки моделей' });
  const label = section
    .locator('label')
    .filter({ has: page.getByLabel('Общая модель', { exact: true }) });
  await expect(label).toHaveCSS('display', 'grid');
  await expect(label).toHaveCSS('gap', '5px');
  await expect(section.locator('p').first()).toHaveCSS('font-size', '12px');
  await expect(section.locator('p').first()).toHaveCSS('line-height', '18px');
});

test('three-screen entry composition and settings persist', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'PROTEUS', exact: true })).toBeVisible();
  await expect(page.getByLabel('Имя персоны A')).toBeVisible();
  await expect(page.getByLabel('Имя персоны B')).toBeVisible();
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByLabel('Общая модель', { exact: true }).selectOption('kimi');
  await page.getByLabel('Модель STALKER', { exact: true }).selectOption('codex');
  await page.getByLabel('Название модели Person', { exact: true }).fill('custom-model');
  await page.getByRole('button', { name: 'Закрыть настройки' }).click();
  await page.reload();
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await expect(page.getByLabel('Модель STALKER', { exact: true })).toHaveValue('codex');
  await expect(page.getByLabel('Модель Person', { exact: true })).toHaveValue('kimi');
  await expect(page.getByLabel('Название модели Person', { exact: true })).toHaveValue(
    'custom-model',
  );
});
for (const [width, height] of [
  [1440, 900],
  [1024, 768],
  [640, 600],
] as const) {
  test(`no scroll or overflowing content at ${width}×${height}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.goto('/');
    const check = async () => {
      const overflow = await page.evaluate(() =>
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
      );
      expect(overflow).toEqual([]);
    };
    await check();
    await page
      .getByLabel('Контекст A', { exact: true })
      .fill(
        'Подходит к задачам последовательно. Сначала составляет план и проверяет исходные данные.',
      );
    await check();
    await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
    await check();
  });
}
test('browser preview cannot pretend a native CLI run succeeded', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('Контекст A', { exact: true }).fill('Careful planner');
  await page.getByLabel('Контекст B', { exact: true }).fill('Fast improviser');
  await page.getByLabel('Общая задача', { exact: true }).fill('Plan a library');
  await page.getByRole('button', { name: 'Начать эксперимент' }).click();
  await expect(page.getByRole('alert')).toContainText('настольном приложении');
});

test('model settings migrate to version2 without rewriting the legacy record', async ({ page }) => {
  const legacy = JSON.stringify({
    'global-model': 'kimi',
    components: {
      stalker: 'codex',
      'person-fabric': 'kimi',
      person: 'kimi',
      'brain-analyzator': 'kimi',
    },
    aliases: { stalker: ' exact alias ', 'person-fabric': '', person: '', 'brain-analyzator': '' },
  });
  await page.addInitScript(
    (legacy) => localStorage.setItem('proteus-model-settings', legacy),
    legacy,
  );
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await expect(page.getByLabel('Название модели STALKER', { exact: true })).toHaveValue(
    ' exact alias ',
  );
  const before = await page.evaluate(() => ({
    legacy: localStorage.getItem('proteus-model-settings'),
    current: JSON.parse(localStorage.getItem('proteus-model-settings-v2') ?? '{}'),
  }));
  expect(before.legacy).toBe(legacy);
  expect(before.current).toMatchObject({
    format: 'proteus-model-settings',
    version: 2,
    globalModel: 'kimi',
  });
  await page.getByLabel('Название модели Person', { exact: true }).fill('explicit-change');
  const after = await page.evaluate(() => ({
    legacy: localStorage.getItem('proteus-model-settings'),
    current: JSON.parse(localStorage.getItem('proteus-model-settings-v2') ?? '{}'),
  }));
  expect(after.legacy).toBe(legacy);
  expect(after.current.aliases.person).toBe('explicit-change');
});

test('invalid stored model settings remain recoverable and show a visible error', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('proteus-model-settings', '{broken-exact'));
  await page.goto('/');
  await expect(page.getByRole('alert')).toContainText('Не удалось восстановить настройки');
  expect(await page.evaluate(() => localStorage.getItem('proteus-model-settings'))).toBe(
    '{broken-exact',
  );
  expect(await page.evaluate(() => localStorage.getItem('proteus-model-settings-v2'))).toBeNull();
});
