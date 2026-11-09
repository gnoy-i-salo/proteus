import { expect, test } from '@playwright/test';
import { fillReplyExperiment, nativeReplyFixture, noReplyOverflow } from './replies-fixture';

for (const [width, height, scale] of [
  [640, 600, 1],
  [1024, 768, 1],
  [1280, 800, 1],
  [1280, 800, 1.25],
] as const) {
  test(`profile inspector separates baseline and adaptation without scrolling at ${width}x${height} scale ${scale}`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height });
    await nativeReplyFixture(page);
    await fillReplyExperiment(page);
    await page.addStyleTag({
      content: `:root { --reading: ${14 * scale}px; --caption: ${12 * scale}px; }`,
    });
    await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
    await page.getByRole('button', { name: 'Профиль', exact: true }).click();
    const section = page.getByLabel('Раздел профиля', { exact: true });
    await expect(section).toBeVisible();
    await section.selectOption({ label: 'Базовые правила' });
    await expect(page.getByText('Structured planner', { exact: false })).toBeVisible();
    await section.selectOption({ label: 'Адаптация к задаче' });
    await expect(page.getByText('Compare options before choosing', { exact: false })).toBeVisible();
    await noReplyOverflow(page);
    await page.screenshot({ path: testInfo.outputPath('profile.png') });
    await section.selectOption({ label: 'Память' });
    await expect(
      page.getByText('Ссылка на память в профиле не сохранена.', { exact: false }),
    ).toBeVisible();
    await page.getByRole('button', { name: 'Контроль', exact: true }).click();
    await expect(
      page.getByRole('heading', { name: 'Нейтральный контроль', exact: true }),
    ).toBeVisible();
    await noReplyOverflow(page);
  });
}
