import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import type { ExperimentInput } from '@proteus-int/contracts';
async function exported(page: Page) {
  await page.getByRole('textbox', { name: 'Контекст B', exact: true }).fill('Контекст B');
  await page.getByRole('textbox', { name: 'Общая задача', exact: true }).fill('Общая задача');
  const downloading = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Экспорт .proteus', exact: true }).click();
  const stream = await (await downloading).createReadStream();
  const chunks: Buffer[] = [];
  for await (const chunk of stream) {
    if (!Buffer.isBuffer(chunk)) throw new Error('Unexpected preset download chunk');
    chunks.push(chunk);
  }
  return JSON.parse(Buffer.concat(chunks).toString('utf8')) as ExperimentInput;
}
test('backward selection crossing a resized page replaces the entire original range', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  const text = 'Точное слово  👩🏽‍🔬 конец. '.repeat(5);
  const editor = page.getByRole('textbox', { name: 'Контекст A', exact: true });
  await editor.fill(text);
  const selected = await editor.evaluate((element: HTMLTextAreaElement) => {
    element.focus();
    element.setSelectionRange(3, element.value.length - 3, 'backward');
    element.dispatchEvent(new Event('select', { bubbles: true }));
    return { text: element.value, start: 3, end: element.value.length - 3 };
  });
  await page.setViewportSize({ width: 640, height: 600 });
  await page.waitForTimeout(100);
  await page.keyboard.insertText('Ж');
  const result = await exported(page);
  const prefix = text.slice(0, text.length - selected.text.length);
  expect(result.people[0].context).toBe(
    prefix + selected.text.slice(0, selected.start) + 'Ж' + selected.text.slice(selected.end),
  );
});
test('Next never creates an implicit empty page', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('textbox', { name: 'Контекст A', exact: true }).fill('Точный текст');
  await expect(page.getByRole('button', { name: 'Далее: Контекст A', exact: true })).toBeDisabled();
});
for (const [width, height, scale] of [
  [640, 600, 1],
  [1024, 768, 1],
  [1280, 800, 1],
  [640, 600, 1.25],
] as const) {
  test(`expanded editor search and return preserve source and no scroll ${width}/${scale}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await page.goto('/');
    await page.addStyleTag({
      content: `:root{--reading:${14 * scale}px;--caption:${12 * scale}px;}`,
    });
    const text = 'Оригинальный абзац  👩🏽‍🔬\n\n'.repeat(24) + 'ПОИСКОВАЯ МЕТКА\n';
    const editor = page.getByRole('textbox', { name: 'Контекст A', exact: true });
    await editor.fill(text);
    await page.getByRole('button', { name: 'Развернуть: Контекст A', exact: true }).click();
    const dialog = page.getByRole('dialog', { name: 'Контекст A', exact: true });
    await expect(dialog).toBeVisible();
    await dialog.getByRole('button', { name: 'Назад: Контекст A' }).click();
    await editor.evaluate((element: HTMLTextAreaElement) => {
      element.focus();
      element.setSelectionRange(1, 15, 'backward');
      element.dispatchEvent(new Event('select', { bubbles: true }));
    });
    const before = await editor.evaluate((element: HTMLTextAreaElement) =>
      element.value.slice(element.selectionStart, element.selectionEnd),
    );
    await page.getByRole('textbox', { name: 'Поиск: Контекст A' }).fill('ПОИСКОВАЯ МЕТКА');
    await expect(editor).toHaveValue(/ПОИСКОВАЯ МЕТКА/);
    await page.getByRole('button', { name: 'Вернуться к месту' }).click();
    await expect
      .poll(() =>
        editor.evaluate((element: HTMLTextAreaElement) =>
          element.value.slice(element.selectionStart, element.selectionEnd),
        ),
      )
      .toBe(before);
    expect(
      await editor.evaluate((element: HTMLTextAreaElement) => element.selectionDirection),
    ).toBe('backward');
    const overflow = await dialog
      .locator('*')
      .evaluateAll((elements) =>
        elements
          .filter(
            (element) =>
              element instanceof HTMLElement &&
              !['SELECT', 'OPTION', 'INPUT'].includes(element.tagName) &&
              element.clientHeight > 0 &&
              (element.scrollHeight > element.clientHeight + 2 ||
                element.scrollWidth > element.clientWidth + 2),
          )
          .map((element) => element.tagName + '.' + element.className),
      );
    expect(overflow).toEqual([]);
    await page.getByRole('button', { name: 'Свернуть: Контекст A' }).click();
    expect((await exported(page)).people[0].context).toBe(text);
  });
}
test('expanded to compact retains backward copy, cut and Unicode paste across page boundaries', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await page.goto('/');
  const original = 'Точная строка  👩🏽‍🔬 и пробелы. '.repeat(24);
  const editor = page.getByRole('textbox', { name: 'Контекст A', exact: true });
  await editor.fill(original);
  await page.getByRole('button', { name: 'Развернуть: Контекст A' }).click();
  await page.getByLabel('Оглавление: Контекст A').selectOption('0');
  const range = await editor.evaluate((element: HTMLTextAreaElement) => {
    element.focus();
    element.setSelectionRange(5, element.value.length - 5, 'backward');
    element.dispatchEvent(new Event('select', { bubbles: true }));
    return { start: 5, end: element.value.length - 5, selected: element.value.slice(5, -5) };
  });
  expect(range.selected.length).toBeGreaterThan(100);
  await page.getByRole('button', { name: 'Свернуть: Контекст A' }).click();
  await expect(page.getByRole('status')).toHaveAttribute('title', /включая другие страницы/);
  await editor.evaluate((element) => {
    element.dispatchEvent(new ClipboardEvent('cut', { bubbles: true, cancelable: true }));
    element.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true }));
  });
  expect(
    await editor.evaluate((element) => {
      const data = new DataTransfer();
      element.dispatchEvent(
        new ClipboardEvent('copy', { clipboardData: data, bubbles: true, cancelable: true }),
      );
      return data.getData('text/plain');
    }),
  ).toBe(range.selected);
  expect(
    await editor.evaluate((element) => {
      const data = new DataTransfer();
      element.dispatchEvent(
        new ClipboardEvent('cut', { clipboardData: data, bubbles: true, cancelable: true }),
      );
      return data.getData('text/plain');
    }),
  ).toBe(range.selected);
  await editor.evaluate((element) => {
    const data = new DataTransfer();
    data.setData('text/plain', '  ✨\nНОВОЕ  ');
    element.dispatchEvent(
      new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }),
    );
  });
  expect((await exported(page)).people[0].context).toBe(
    original.slice(0, range.start) + '  ✨\nНОВОЕ  ' + original.slice(range.end),
  );
});
test('compact editors show every newline page without hidden overflow after text enlargement', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await page.goto('/');
  await page.addStyleTag({ content: ':root{--reading:17.5px;--caption:15px;}' });
  for (const label of ['Контекст A', 'Общая задача']) {
    const editor = page.getByRole('textbox', { name: label, exact: true });
    await editor.fill('Первая строка\n\nВторая строка 👩🏽‍🔬\n'.repeat(2));
    const previous = page.getByRole('button', { name: `Назад: ${label}`, exact: true });
    for (let step = 0; step < 50; step++) {
      expect(
        await editor.evaluate(
          (element) =>
            element.scrollHeight <= element.clientHeight + 2 &&
            element.scrollWidth <= element.clientWidth + 2,
        ),
      ).toBe(true);
      if (await previous.isDisabled()) break;
      await previous.click();
      if (step === 49) throw new Error('Unexpected micro-pagination');
    }
  }
});
test('native IME composition survives candidate updates and resize before committing the full selected range', async ({
  page,
}) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.goto('/');
  const original = 'Точная строка  👩🏽‍🔬 и пробелы. '.repeat(5);
  const editor = page.getByRole('textbox', { name: 'Контекст A', exact: true });
  await editor.fill(original);
  const range = await editor.evaluate((element: HTMLTextAreaElement) => {
    element.focus();
    element.setSelectionRange(3, element.value.length - 3, 'backward');
    element.dispatchEvent(new Event('select', { bubbles: true }));
    const compositionEvents: string[] = [];
    Reflect.set(window, 'compositionEvents', compositionEvents);
    for (const type of ['compositionstart', 'compositionend'])
      element.addEventListener(type, () => compositionEvents.push(type));
    return { text: element.value, start: 3, end: element.value.length - 3 };
  });
  await page.setViewportSize({ width: 640, height: 600 });
  const session = await page.context().newCDPSession(page);
  await session.send('Input.imeSetComposition', { text: 'に', selectionStart: 1, selectionEnd: 1 });
  await page.setViewportSize({ width: 1024, height: 768 });
  await session.send('Input.imeSetComposition', {
    text: '日本',
    selectionStart: 2,
    selectionEnd: 2,
  });
  await page.evaluate(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())));
  expect(
    await page.evaluate(() => Reflect.get(window, 'compositionEvents') as readonly string[]),
  ).toEqual(['compositionstart']);
  await session.send('Input.insertText', { text: '日本語' });
  expect(
    await page.evaluate(() => Reflect.get(window, 'compositionEvents') as readonly string[]),
  ).toEqual(['compositionstart', 'compositionend']);
  const prefix = original.slice(0, original.length - range.text.length);
  expect((await exported(page)).people[0].context).toBe(
    prefix + range.text.slice(0, range.start) + '日本語' + range.text.slice(range.end),
  );
  await session.detach();
});
