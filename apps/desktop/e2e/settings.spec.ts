import { expect, test } from '@playwright/test';

test('model settings keep label spacing and prose typography', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  const section = page.getByRole('region', { name: 'Настройки моделей' });
  const label = section
    .locator('label')
    .filter({ has: page.getByLabel('Общий провайдер', { exact: true }) });
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
  await page.getByLabel('Общий провайдер', { exact: true }).selectOption('kimi');
  await page.getByRole('button', { name: 'Применить ко всем', exact: true }).click();
  await page.getByLabel('Провайдер STALKER', { exact: true }).selectOption('codex');
  await page.getByLabel('Название модели Person', { exact: true }).fill('custom-model');
  await page.getByRole('button', { name: 'Закрыть настройки' }).click();
  await page.reload();
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await expect(page.getByLabel('Провайдер STALKER', { exact: true })).toHaveValue('codex');
  await expect(page.getByLabel('Провайдер Person', { exact: true })).toHaveValue('kimi');
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
    current: JSON.parse(localStorage.getItem('proteus-model-settings-v2') ?? '{}') as unknown,
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
    current: JSON.parse(localStorage.getItem('proteus-model-settings-v2') ?? '{}') as unknown,
  }));
  expect(after.legacy).toBe(legacy);
  expect(after.current).toHaveProperty(['aliases', 'person'], 'explicit-change');
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

test('global provider selection previews four targets without applying before the button', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByLabel('Общий провайдер', { exact: true }).selectOption('kimi');
  for (const component of ['STALKER', 'Person Fabric', 'Person', 'Brain Analyzator']) {
    await expect(page.getByLabel(`Провайдер ${component}`, { exact: true })).toHaveValue('codex');
  }
  await page.getByRole('button', { name: 'Применить ко всем', exact: true }).click();
  for (const component of ['STALKER', 'Person Fabric', 'Person', 'Brain Analyzator']) {
    await expect(page.getByLabel(`Провайдер ${component}`, { exact: true })).toHaveValue('kimi');
  }
});

test('settings tabs use arrow navigation and a single tab stop', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  const models = page.getByRole('tab', { name: 'Модели', exact: true });
  const connections = page.getByRole('tab', { name: 'Подключения', exact: true });
  await expect(models).toBeFocused();
  await models.press('ArrowRight');
  await expect(connections).toBeFocused();
  await expect(connections).toHaveAttribute('aria-selected', 'true');
  await expect(models).toHaveAttribute('tabindex', '-1');
  await connections.press('Home');
  await expect(models).toBeFocused();
  await expect(models).toHaveAttribute('aria-selected', 'true');
  await models.press('Shift+Tab');
  const close = page.getByRole('button', { name: 'Закрыть настройки' });
  const done = page.getByRole('button', { name: 'Готово ↵' });
  await expect(close).toBeFocused();
  await close.press('Shift+Tab');
  await expect(done).toBeFocused();
  await done.press('Tab');
  await expect(close).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(page.getByRole('button', { name: 'Настройки PROTEUS' })).toBeFocused();
});

for (const [width, height] of [
  [640, 600],
  [1024, 768],
  [1280, 800],
] as const) {
  test(`model settings honor enlarged text without clipping at ${width}`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    await page.goto('/');
    await page.addStyleTag({ content: ':root { --reading: 17.5px; --caption: 15px; }' });
    await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
    const panel = page.getByRole('region', { name: 'Настройки моделей' });
    await expect(panel.locator('p').first()).toHaveCSS('font-size', '15px');
    const dialog = page.getByRole('dialog', { name: 'Настройки PROTEUS' });
    const bounds = await dialog.boundingBox();
    expect(bounds).not.toBeNull();
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(height);
    expect(
      await dialog.evaluate((element) =>
        [...element.querySelectorAll<HTMLElement>('*')]
          .filter(
            (child) =>
              child.clientWidth > 0 && !['INPUT', 'SELECT', 'OPTION'].includes(child.tagName),
          )
          .filter(
            (child) =>
              child.scrollWidth > child.clientWidth + 2 ||
              child.scrollHeight > child.clientHeight + 2,
          )
          .map((child) => child.tagName),
      ),
    ).toEqual([]);
  });
}
