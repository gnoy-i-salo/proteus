import { expect, test } from '@playwright/test';

for (const [width, height, scale] of [
  [640, 600, 1],
  [1024, 768, 1],
  [1280, 800, 1],
  [1280, 800, 1.25],
] as const) {
  test(`input foundation stays readable and aligned at ${width}×${height} text ${scale}`, async ({
    page,
  }, testInfo) => {
    await page.setViewportSize({ width, height });
    await page.goto('/');
    await page.addStyleTag({
      content: `:root { --reading: ${14 * scale}px; --caption: ${12 * scale}px; }`,
    });
    await page.getByLabel('Имя персоны A').fill('Александра Константиновна');
    await page.getByLabel('Контекст A', { exact: true }).fill('Точный текст: имя, связи, цитаты.');
    for (const label of ['Имя персоны A', 'Имя персоны B', 'Общая задача', 'Контекст A']) {
      const field = page.getByLabel(label, { exact: true });
      await expect(field).toBeVisible();
      expect(
        await field.evaluate((element) => Number.parseFloat(getComputedStyle(element).fontSize)),
      ).toBeGreaterThanOrEqual(14);
    }
    for (const name of ['Источники персоны A', 'Настройки PROTEUS']) {
      const button = page.getByRole('button', { name });
      expect(
        await button.evaluate((element) => Number.parseFloat(getComputedStyle(element).fontSize)),
      ).toBeGreaterThanOrEqual(12);
      const before = await button.boundingBox();
      await button.focus();
      expect(await button.boundingBox()).toEqual(before);
    }
    if (width > 900) {
      const name = await page.getByLabel('Имя персоны A').boundingBox();
      const url = await page.getByLabel('Публичная ссылка A', { exact: true }).boundingBox();
      expect(name).not.toBeNull();
      expect(url).not.toBeNull();
      expect(Math.abs(name!.y - url!.y)).toBeLessThanOrEqual(1);
    }
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
          .map(
            (element) =>
              `${element.tagName}.${element.className}: ${element.scrollWidth}/${element.clientWidth} ${element.scrollHeight}/${element.clientHeight}`,
          ),
      ),
    ).toEqual([]);
    await page.screenshot({ path: testInfo.outputPath('foundation.png') });
    await expect(page.getByLabel('Контекст A', { exact: true })).toHaveValue(
      'Точный текст: имя, связи, цитаты.',
    );
  });
}

test('resizing preserves the selected original text in a paginated editor', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  const context = page.getByLabel('Контекст A', { exact: true });
  await context.fill('Точная строка с пробелами  и emoji 👩🏽‍🔬\n'.repeat(12) + 'ФИНИШ');
  await context.evaluate((element: HTMLTextAreaElement) => {
    element.focus();
    const start = element.value.indexOf('ФИНИШ');
    element.setSelectionRange(start, start + 'ФИНИШ'.length);
  });
  await page.setViewportSize({ width: 640, height: 600 });
  await page.evaluate(
    () =>
      new Promise<void>((resolve) =>
        requestAnimationFrame(() => requestAnimationFrame(() => resolve())),
      ),
  );
  await expect
    .poll(() =>
      context.evaluate((element: HTMLTextAreaElement) =>
        element.value.slice(element.selectionStart, element.selectionEnd),
      ),
    )
    .toBe('ФИНИШ');
});
