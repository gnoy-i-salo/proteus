import { expect, test } from '@playwright/test';
test('persona changes the palette and settings survive a reload', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Настройки PROTEUS' })).toBeVisible();
  await page.getByRole('button', { name: /PERSON B/ }).click();
  await expect(page.locator('[data-palette]')).toHaveAttribute('data-palette', 'violet');
  await expect(page.getByRole('button', { name: /PERSON B/ })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.getByLabel('Модель STALKER', { exact: true }).selectOption('kimi');
  await expect(page.getByLabel('Модель Person', { exact: true })).toHaveValue('codex');
  await page.reload();
  await expect(page.getByLabel('Модель STALKER', { exact: true })).toHaveValue('kimi');
  await page.getByLabel('Общая модель', { exact: true }).selectOption('kimi');
  for (const name of ['STALKER', 'Person Fabric', 'Person', 'Brain Analyzator'])
    await expect(page.getByLabel(`Модель ${name}`, { exact: true })).toHaveValue('kimi');
  await page.getByLabel('Модель Person', { exact: true }).selectOption('codex');
  await page.getByRole('button', { name: 'Применить ко всем' }).click();
  await expect(page.getByLabel('Модель Person', { exact: true })).toHaveValue('kimi');
  await page.reload();
  await expect(page.getByLabel('Модель Person', { exact: true })).toHaveValue('kimi');
});
test('corrupt storage falls back to defaults; browser has no dead native controls', async ({
  page,
}) => {
  await page.addInitScript(() => localStorage.setItem('proteus-model-settings', '{bad-json'));
  await page.goto('/');
  await expect(page.getByLabel('Общая модель', { exact: true })).toHaveValue('codex');
  await expect(page.getByRole('button', { name: 'Закрыть окно' })).toHaveCount(0);
  await expect(page.getByText('WEB PREVIEW')).toBeVisible();
});
test('settings remain usable at the native minimum width', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 900 });
  await page.goto('/');
  await expect(page.getByLabel('Модель STALKER', { exact: true })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
});
