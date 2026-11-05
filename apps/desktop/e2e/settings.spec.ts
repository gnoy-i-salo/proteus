import { expect, test } from '@playwright/test';
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
