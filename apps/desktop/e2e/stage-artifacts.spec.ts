import { expect, test } from '@playwright/test';
import { fillReplyExperiment, nativeReplyFixture, noReplyOverflow } from './replies-fixture';

test('stage artifacts and call observations remain available without reasoning', async ({
  page,
}) => {
  await nativeReplyFixture(page, {
    noReasoning: true,
    exactQuote: 'Точный ответ',
    customMessages: ['  Точный ответ\r\n🙂 <b>текст</b>'],
  });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 50' })).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await page.getByRole('button', { name: 'К вызову', exact: true }).click();
  const reader = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await reader.getByRole('button', { name: 'Опубликованный текст', exact: true }).click();
  await expect(reader.getByLabel('Опубликованный текст', { exact: true }).last()).toContainText(
    'Точный ответ',
  );
  expect(
    await reader
      .getByLabel('Опубликованный текст', { exact: true })
      .last()
      .locator('p')
      .textContent(),
  ).toBe('  Точный ответ\r\n🙂 <b>текст</b>');
  await expect(reader.locator('b')).toHaveCount(0);
  await reader.getByRole('button', { name: 'Результат этапа', exact: true }).click();
  await expect(reader.getByLabel('Результат этапа', { exact: true }).last()).toContainText(
    'Прогон 1',
  );
  await reader.getByRole('button', { name: 'Вызовы и ошибки', exact: true }).click();
  const calls = reader.getByLabel('Вызовы и ошибки', { exact: true }).last();
  await expect(calls).toContainText('первичный вызов');
  await expect(calls).toContainText('завершён');
  await expect(calls).toContainText('60000 мс');
  await expect(calls).toContainText('Наблюдаемая модель: не раскрыта');
  await expect(calls).toContainText('Запрошенная модель:');
  await reader.getByRole('button', { name: 'Reasoning', exact: true }).click();
  await expect(reader.getByLabel('Текст', { exact: true }).last()).toContainText(
    'CLI не передал текст reasoning',
  );
});

test('rubric artifact uses the frozen task and criteria of the selected stage', async ({
  page,
}) => {
  await nativeReplyFixture(page, { noReasoning: true });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
  const reader = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await reader.getByRole('radio', { name: 'Общие', exact: true }).check();
  await reader.getByRole('button', { name: /Общие · Критерии/ }).click();
  await reader.getByRole('button', { name: 'Результат этапа', exact: true }).click();
  const artifact = reader.getByLabel('Результат этапа', { exact: true }).last();
  await expect(artifact).toContainText('Plan a library');
  await expect(artifact).toContainText('Полнота решения 1');
  await reader.getByRole('button', { name: 'Опубликованный текст', exact: true }).click();
  await expect(reader.getByLabel('Опубликованный текст', { exact: true }).last()).toContainText(
    'Отдельный опубликованный текст для этого этапа не сохранён',
  );
});

test('failed solution exposes its error without fabricating an answer', async ({ page }) => {
  await nativeReplyFixture(page, { failSolve: true, noReasoning: true });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · не оценено' })).toBeVisible();
  await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
  const reader = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await reader.getByRole('button', { name: /A · Решение/ }).click();
  await reader.getByRole('button', { name: 'Опубликованный текст', exact: true }).click();
  await expect(reader.getByLabel('Опубликованный текст', { exact: true }).last()).toContainText(
    'Ответ не получен',
  );
  await reader.getByRole('button', { name: 'Вызовы и ошибки', exact: true }).click();
  await expect(reader.getByLabel('Вызовы и ошибки', { exact: true }).last()).toContainText(
    'Fixture CLI unavailable',
  );
});

test('content choice and text position survive stage and result tab changes in a narrow window', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeReplyFixture(page);
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await page.getByRole('button', { name: 'К вызову', exact: true }).click();
  const reader = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await reader.getByLabel('Представление этапа', { exact: true }).selectOption('published');
  const pager = reader.getByRole('navigation', { name: 'Страницы текста' });
  await pager.getByRole('button', { name: 'Следующая страница текста' }).click();
  await expect(pager).toContainText('2 /');
  await reader.getByLabel('Этап', { exact: true }).selectOption('rubric');
  await reader.getByLabel('Представление этапа', { exact: true }).selectOption('artifact');
  await noReplyOverflow(page);
  await reader.getByLabel('Этап', { exact: true }).selectOption('solve-1-a');
  await expect(reader.getByLabel('Представление этапа', { exact: true })).toHaveValue('published');
  await expect(pager).toContainText('2 /');
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await page.getByRole('button', { name: 'Intelligence traces', exact: true }).click();
  await expect(pager).toContainText('2 /');
  await noReplyOverflow(page);
});
