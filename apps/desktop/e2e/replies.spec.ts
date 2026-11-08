import { expect, test } from '@playwright/test';
import { fillReplyExperiment, nativeReplyFixture, noReplyOverflow } from './replies-fixture';

test('answer reader selects round and message individually across three rounds and control', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeReplyFixture(page, { messageReply: true });
  await fillReplyExperiment(page, '3');
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible({ timeout: 15000 });
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await expect(page.getByText('Поведенческая симуляция', { exact: true })).toBeVisible();
  const roundSelect = page.getByLabel('Прогон ответа', { exact: true });
  await expect(roundSelect.locator('option')).toHaveCount(3);
  const messageSelect = page.getByLabel('Сообщение ответа', { exact: true });
  await expect(messageSelect.locator('option')).toHaveCount(2);
  await expect(page.getByText('Прогон 1 · Сообщение 1 / 2', { exact: true })).toBeVisible();
  await roundSelect.selectOption('2');
  await expect(page.getByText('Прогон 2 · Сообщение 1 / 2', { exact: true })).toBeVisible();
  await messageSelect.selectOption('1');
  const body = page.getByTestId('persona-reply-body');
  await expect(body.locator('p')).toHaveText('<b>Второе сообщение</b> 🙂', {
    useInnerText: false,
  });
  await expect(body.locator('b')).toHaveCount(0);
  await expect(body).not.toContainText('симуляция');
  await page.getByRole('button', { name: 'Предыдущее сообщение', exact: true }).click();
  await expect(page.getByText('Прогон 2 · Сообщение 1 / 2', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Следующее сообщение', exact: true }).click();
  await expect(page.getByText('Прогон 2 · Сообщение 2 / 2', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Контроль', exact: true }).click();
  await expect(
    page.getByRole('heading', { name: 'Нейтральный контроль', exact: true }),
  ).toBeVisible();
  await expect(roundSelect.locator('option')).toHaveCount(3);
  await expect(messageSelect.locator('option')).toHaveCount(2);
  await expect(body).toContainText('Ответ Контроль.');
  await noReplyOverflow(page);
});

test('answer reader keeps exact whitespace, CRLF and Unicode bytes and renders markup as text', async ({
  page,
}) => {
  const first = '  Шаг один\r\n\tвторой ряд 🙂';
  await nativeReplyFixture(page, {
    customMessages: [first, '<b>Второе</b> сообщение'],
    exactQuote: 'второй ряд',
  });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 50' })).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  const body = page.getByTestId('persona-reply-body');
  expect(await body.locator('p').textContent()).toBe(first);
  await expect(body.locator('b')).toHaveCount(0);
  await page.getByLabel('Сообщение ответа', { exact: true }).selectOption('1');
  expect(await body.locator('p').textContent()).toBe('<b>Второе</b> сообщение');
  await expect(body.locator('b')).toHaveCount(0);
});

test('assessment quote drilldown highlights an explicit occurrence and returns to the prior tab', async ({
  page,
}) => {
  await nativeReplyFixture(page, { exactQuote: 'Опорный тезис.' });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Критерии', exact: true }).click();
  await page.getByRole('button', { name: 'Развернуть критерий', exact: true }).click();
  await page.getByRole('button', { name: 'Показать в ответе', exact: true }).first().click();
  await expect(page.getByRole('button', { name: 'Ответ', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  const quoteBar = page.getByRole('status', { name: 'Переход к цитате' });
  await expect(quoteBar).toContainText('вхождение 1 из 100');
  const body = page.getByTestId('persona-reply-body');
  await expect(body.locator('mark')).toHaveText('Опорный тезис.');
  const before = await body.locator('p').textContent();
  await page.getByRole('button', { name: 'Следующее вхождение', exact: true }).click();
  await expect(quoteBar).toContainText('вхождение 2 из 100');
  await expect(body.locator('mark')).toHaveText('Опорный тезис.');
  expect(await body.locator('p').textContent()).toBe(before);
  await page.getByRole('button', { name: '← Вернуться к анализу', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Анализ', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.getByRole('button', { name: '← К матрице', exact: true })).toBeVisible();
  await expect(page.getByLabel('Основания и точные цитаты', { exact: true }).last()).toBeVisible();
});

test('quote spanning joined messages highlights each exact part', async ({ page }) => {
  await nativeReplyFixture(page, {
    customMessages: ['Первое сообщение', 'Второе вложение'],
    exactQuote: 'сообщение\n\nВторое',
  });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 50' })).toBeVisible();
  await page.getByRole('button', { name: 'Критерии', exact: true }).click();
  await page.getByRole('button', { name: 'Развернуть критерий', exact: true }).click();
  await page.getByRole('button', { name: 'Показать в ответе', exact: true }).first().click();
  const body = page.getByTestId('persona-reply-body');
  await expect(page.getByLabel('Сообщение ответа', { exact: true })).toHaveValue('0');
  await expect(body.locator('mark')).toHaveText('сообщение');
  await page.getByLabel('Сообщение ответа', { exact: true }).selectOption('1');
  await expect(body.locator('mark')).toHaveText('Второе');
});

test('failed round keeps the answer unpublished and the creating call explicit', async ({
  page,
}) => {
  await nativeReplyFixture(page, { failSolve: true });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · не оценено' })).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  const body = page.getByTestId('persona-reply-body');
  await expect(body).toContainText('Fixture CLI unavailable');
  await expect(
    page.getByLabel('Прогон ответа', { exact: true }).locator('option').first(),
  ).toContainText('нет ответа');
  const call = page.getByRole('group', { name: 'Создавший вызов' });
  await expect(call).toContainText('не связан');
  await expect(call.getByRole('button', { name: 'К вызову', exact: true })).toBeDisabled();
  await expect(page.getByRole('status', { name: 'Переход к цитате' })).toHaveCount(0);
});

test('creating call opens the exact stage and the answer position survives the return', async ({
  page,
}) => {
  await nativeReplyFixture(page);
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  const call = page.getByRole('group', { name: 'Создавший вызов' });
  await expect(call).toContainText('первичный вызов');
  await expect(call).toContainText('завершён');
  const pager = page.getByRole('navigation', { name: 'Страницы текста' });
  await pager.getByRole('button', { name: 'Следующая страница текста' }).click();
  await expect(pager).toContainText('2 /');
  await call.getByRole('button', { name: 'К вызову', exact: true }).click();
  const logs = page.getByRole('region', { name: 'Текст Intelligence traces' });
  await expect(logs.getByRole('button', { name: /A · Решение/ })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await page.getByRole('button', { name: 'Ответ', exact: true }).click();
  await expect(page.getByRole('navigation', { name: 'Страницы текста' })).toContainText('2 /');
});

test('returns from a creating call through the quote to the same analysis criterion', async ({
  page,
}) => {
  await nativeReplyFixture(page, { exactQuote: 'Опорный тезис.' });
  await fillReplyExperiment(page);
  await expect(page.getByRole('heading', { name: 'BSC v2 · 100' })).toBeVisible();
  await page.getByRole('button', { name: 'Критерии', exact: true }).click();
  await page.getByRole('button', { name: 'Развернуть критерий', exact: true }).click();
  await page.getByRole('button', { name: 'Показать в ответе', exact: true }).first().click();
  await page.getByRole('button', { name: 'Следующее вхождение', exact: true }).click();
  await page.getByRole('button', { name: 'К вызову', exact: true }).click();
  await page.getByRole('button', { name: '← Вернуться к ответу', exact: true }).click();
  await expect(page.getByRole('status', { name: 'Переход к цитате' })).toContainText(
    'вхождение 2 из 100',
  );
  await page.getByRole('button', { name: '← Вернуться к анализу', exact: true }).click();
  await expect(page.getByRole('button', { name: 'Анализ', exact: true })).toHaveAttribute(
    'aria-pressed',
    'true',
  );
  await expect(page.getByRole('button', { name: '← К матрице', exact: true })).toBeVisible();
});
