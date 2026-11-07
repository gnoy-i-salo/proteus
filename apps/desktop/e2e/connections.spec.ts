import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';

async function nativeConnections(page: Page) {
  await page.addInitScript(() => {
    Reflect.set(window, 'isTauri', true);
    Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
    const states: Record<string, Record<string, unknown>> = Object.fromEntries(
      ['telegram', 'vk', 'github', 'instagram'].map((provider) => [
        provider,
        { provider, status: 'unconfigured', capabilities: [] },
      ]),
    );
    const calls: { command: string; args: unknown }[] = [];
    const callbacks = new Map<number, (event: unknown) => void>();
    const eventHandlers = new Map<string, number>();
    let callbackId = 0;
    Reflect.set(window, 'emitConnectionError', () =>
      callbacks.get(eventHandlers.get('connection-error') ?? -1)?.({
        payload: 'raw-provider-detail-never-display',
      }),
    );
    Reflect.set(window, 'connectionCalls', calls);
    Reflect.set(
      window,
      'setConnectionState',
      (provider: string, state: Record<string, unknown>) => {
        states[provider] = { ...states[provider], ...state };
      },
    );
    const qrCode = (version = 'first') =>
      `data:image/svg+xml;base64,${btoa(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"><title>${version}</title><rect width="100" height="100" fill="white"/><rect x="10" y="10" width="30" height="30"/></svg>`)}`;
    Reflect.set(window, 'rotateTelegramQr', () => {
      states['telegram'] = {
        ...states['telegram'],
        status: 'qr-required',
        'qr-code': qrCode('rotated'),
      };
    });
    Reflect.set(window, '__TAURI_INTERNALS__', {
      transformCallback: (callback: (event: unknown) => void) => {
        callbacks.set(++callbackId, callback);
        return callbackId;
      },
      async invoke(command: string, args: Record<string, unknown> = {}) {
        if (command === 'cli_status' || command === 'take_pending_presets') return [];
        if (command === 'plugin:event|listen') {
          eventHandlers.set(String(args['event']), Number(args['handler']));
          return args['handler'];
        }
        if (command === 'connection_statuses') {
          if (Reflect.get(window, 'rejectNextConnectionPoll')) {
            Reflect.set(window, 'rejectNextConnectionPoll', false);
            throw new Error('test-transient-status-error');
          }
          Reflect.set(
            window,
            'connectionPollCount',
            Number(Reflect.get(window, 'connectionPollCount') ?? 0) + 1,
          );
          const snapshot = structuredClone(Object.values(states));
          if (Reflect.get(window, 'holdNextConnectionPoll')) {
            Reflect.set(window, 'holdNextConnectionPoll', false);
            return new Promise((resolve) =>
              Reflect.set(window, 'releaseConnectionPoll', () => resolve(snapshot)),
            );
          }
          return snapshot;
        }
        calls.push({ command, args });
        if (command === 'open_connection_page') return;
        if (command === 'begin_connection' || command === 'disconnect_connection') {
          if (Reflect.get(window, 'rejectNextConnectionAction')) {
            Reflect.set(window, 'rejectNextConnectionAction', false);
            throw new Error('test-native-credential-error');
          }
          if (Reflect.get(window, 'holdNextConnectionAction')) {
            Reflect.set(window, 'holdNextConnectionAction', false);
            await new Promise<void>((resolve) =>
              Reflect.set(window, 'releaseConnectionAction', resolve),
            );
          }
        }
        const request = args['request'] as Record<string, unknown> | undefined;
        const provider = String(request?.['provider'] ?? args['provider']);
        if (command === 'create_source_grant')
          return {
            ...request,
            id: 'grant-test',
            'account-id': 'account-test',
            'snapshot-id': 'snapshot-test',
            'expires-at': '2099-01-01T00:00:00Z',
          };
        if (command === 'configure_connection') {
          if (request?.['token'] === 'bad-test-token') throw new Error('bad-test-token rejected');
          states[provider] = {
            provider,
            status: provider === 'instagram' ? 'connected' : 'disconnected',
            capabilities: provider === 'instagram' ? ['profile', 'posts'] : [],
            ...(request?.['client-id'] ? { 'client-id': request['client-id'] } : {}),
            ...(request?.['api-id'] ? { 'api-id': request['api-id'] } : {}),
          };
        }
        if (command === 'begin_connection') {
          if (provider === 'telegram') delete states[provider]?.['qr-code'];
          states[provider] = {
            ...states[provider],
            status:
              provider === 'telegram'
                ? args['telegramMethod'] === 'phone'
                  ? 'phone-required'
                  : 'qr-required'
                : 'pending',
            ...(provider === 'telegram' && args['telegramMethod'] !== 'phone'
              ? { 'qr-code': qrCode() }
              : {}),
            ...(provider === 'github' ? { 'user-code': 'TEST-CODE', 'poll-after-ms': 60000 } : {}),
          };
        }
        if (command === 'advance_connection') {
          const next = {
            'phone-required': 'code-required',
            'code-required': 'password-required',
            'password-required': 'connected',
            pending: 'connected',
          }[String(states[provider]?.['status'])];
          states[provider] = {
            ...states[provider],
            status: next,
            ...(next === 'connected' ? { label: 'test-account', capabilities: ['profile'] } : {}),
          };
        }
        if (command === 'disconnect_connection')
          states[provider] = { ...states[provider], status: 'disconnected', capabilities: [] };
        if (command === 'import_source')
          return {
            id: 'import:test-archive',
            label: 'messages.json',
            'document-count': 4,
            warnings: [],
          };
        return states[provider] ?? 1;
      },
    });
  });
}

async function openConnections(page: Page) {
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByRole('tab', { name: 'Подключения', exact: true }).click();
}

test('browser settings explain that account connections require desktop', async ({ page }) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByRole('tab', { name: 'Подключения', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('настольном приложении');
  for (const provider of ['Telegram', 'VK', 'GitHub', 'Instagram']) {
    await page.getByRole('tab', { name: provider, exact: true }).click();
    await expect(page.getByRole('status')).toContainText('настольном приложении');
    await expect(page.getByRole('button', { name: /^Подключить / })).toHaveCount(0);
  }
});

test('source selection requires explicit model consent and stays outside saved personas', async ({
  page,
}) => {
  await nativeConnections(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Источники персоны A', exact: true }).click();
  await expect(page.getByRole('dialog', { name: 'Источники персоны A' })).toBeVisible();
  await page.getByLabel('Ресурс', { exact: true }).fill('-1001234567890');
  await page.getByLabel('Автор', { exact: true }).fill('12345');
  await expect(page.getByRole('button', { name: 'Добавить источник', exact: true })).toBeDisabled();
  await page.getByRole('checkbox', { name: /Разрешаю передать/ }).check();
  await page.getByRole('button', { name: 'Добавить источник', exact: true }).click();
  await expect(page.getByText('Telegram · -1001234567890', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Готово', exact: true }).click();
  await expect(
    page.getByRole('button', { name: 'Источники персоны A', exact: true }),
  ).toContainText('1');
  await expect(
    page.getByRole('button', { name: 'Источники персоны B', exact: true }),
  ).toContainText('0');
  const calls = await page.evaluate(() => Reflect.get(window, 'connectionCalls'));
  expect(
    calls.find((call: { command: string }) => call.command === 'create_source_grant').args.request,
  ).toMatchObject({
    provider: 'telegram',
    'resource-id': '-1001234567890',
    'author-id': '12345',
    'allow-model-processing': true,
    budget: {
      'max-steps': 40,
      'max-documents': 1000,
      'max-bytes': 1048576,
      'max-duration-ms': 300000,
    },
  });
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage }))).not.toContain(
    'grant-test',
  );
});

test('connection and source dialogs fit the native 640 by 600 window', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await nativeConnections(page);
  await openConnections(page);
  const assertBounds = async () => {
    expect(
      await page.evaluate(() =>
        [...document.querySelectorAll<HTMLElement>('[role="dialog"], [role="dialog"] *')]
          .filter((element) => {
            const rect = element.getBoundingClientRect();
            return (
              rect.width > 0 &&
              rect.height > 0 &&
              (rect.left < 0 ||
                rect.right > innerWidth ||
                rect.top < 0 ||
                rect.bottom > innerHeight ||
                (!['INPUT', 'SELECT', 'OPTION'].includes(element.tagName) &&
                  (element.scrollWidth > element.clientWidth + 2 ||
                    element.scrollHeight > element.clientHeight + 2)))
            );
          })
          .map((element) => element.className),
      ),
    ).toEqual([]);
  };
  for (const provider of ['Telegram', 'VK', 'GitHub', 'Instagram']) {
    await page.getByRole('tab', { name: provider, exact: true }).click();
    await assertBounds();
  }
  await page.screenshot({ path: testInfo.outputPath('connections-640.png') });
  await page.getByRole('button', { name: 'Закрыть настройки' }).click();
  await page.getByRole('button', { name: 'Источники персоны A', exact: true }).click();
  await assertBounds();
  await page.getByRole('button', { name: 'Импорт JSON / текста', exact: true }).click();
  await assertBounds();
  await page.getByRole('checkbox', { name: /Разрешаю передать/ }).check();
  await page.getByRole('button', { name: 'Добавить источник', exact: true }).click();
  await expect(page.getByText('Telegram · import:test-archive', { exact: true })).toBeVisible();
  await assertBounds();
  await page.screenshot({ path: testInfo.outputPath('sources-640.png') });
});

test('Telegram advances native challenges and clears secret inputs', async ({ page }) => {
  await nativeConnections(page);
  await openConnections(page);
  await page.getByLabel('API ID Telegram', { exact: true }).fill('12345');
  await page.getByLabel('API hash Telegram', { exact: true }).fill('test-api-hash');
  await page.getByRole('button', { name: 'Сохранить настройки Telegram' }).click();
  await page.getByRole('button', { name: 'Войти по номеру телефона', exact: true }).click();
  await page.getByLabel('Номер телефона Telegram', { exact: true }).fill('+79990000000');
  await page.getByRole('button', { name: 'Продолжить', exact: true }).click();
  await page.getByLabel('Код Telegram', { exact: true }).fill('12345');
  await page.getByRole('button', { name: 'Продолжить', exact: true }).click();
  await page.getByLabel('Пароль Telegram', { exact: true }).fill('test-2fa-password');
  await page.getByRole('button', { name: 'Продолжить', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Подключено');
  await expect(page.getByLabel('Пароль Telegram', { exact: true })).toHaveCount(0);
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage }))).not.toContain('test-');
  await page.getByRole('button', { name: 'Отключить Telegram', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Не подключено');
});

test('connected sources explain the current Codex requirement before granting access', async ({
  page,
}) => {
  await nativeConnections(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByLabel('Модель Person', { exact: true }).selectOption('kimi');
  await page.getByRole('button', { name: 'Закрыть настройки' }).click();
  await page.getByRole('button', { name: 'Источники персоны A', exact: true }).click();
  await expect(page.getByRole('alert')).toContainText('Codex для всех компонентов');
  await page.getByLabel('Ресурс', { exact: true }).fill('-1001234567890');
  await expect(page.getByRole('button', { name: 'Добавить источник', exact: true })).toBeDisabled();
});

test('native callback failure is visible without exposing provider response details', async ({
  page,
}) => {
  await nativeConnections(page);
  await openConnections(page);
  await page.evaluate(() => Reflect.get(window, 'emitConnectionError')());
  await expect(page.getByRole('alert')).toContainText('Подключение не завершено');
  await expect(page.getByRole('alert')).not.toContainText('raw-provider-detail');
});

test('deleting downloaded provider data is explicit and removes its attached sources', async ({
  page,
}) => {
  await nativeConnections(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Источники персоны A', exact: true }).click();
  await page.getByLabel('Ресурс', { exact: true }).fill('-1001234567890');
  await page.getByRole('checkbox', { name: /Разрешаю передать/ }).check();
  await page.getByRole('button', { name: 'Добавить источник', exact: true }).click();
  await page.getByRole('button', { name: 'Готово', exact: true }).click();
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByRole('tab', { name: 'Подключения', exact: true }).click();
  await page.getByRole('button', { name: 'Удалить материалы STALKER', exact: true }).click();
  expect(
    await page.evaluate(() =>
      Reflect.get(window, 'connectionCalls').filter(
        (call: { command: string }) => call.command === 'clear_source_data',
      ),
    ),
  ).toHaveLength(0);
  await page
    .getByRole('button', { name: 'Подтвердить удаление материалов Telegram', exact: true })
    .click();
  await expect(page.getByText('Материалы STALKER удалены.', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Закрыть настройки' }).click();
  await expect(
    page.getByRole('button', { name: 'Источники персоны A', exact: true }),
  ).toContainText('0');
});

for (const provider of ['VK', 'GitHub']) {
  test(`${provider} stays pending until native confirms connection`, async ({ page }) => {
    await nativeConnections(page);
    await openConnections(page);
    await page.getByRole('tab', { name: provider, exact: true }).click();
    await page.getByLabel(`Client ID ${provider}`, { exact: true }).fill('test-client');
    await page.getByRole('button', { name: `Сохранить настройки ${provider}` }).click();
    await page
      .getByRole('button', {
        name: provider === 'VK' ? 'Войти в VK через браузер' : 'Подключить GitHub',
        exact: true,
      })
      .click();
    await expect(page.getByRole('status')).toContainText('Ожидаем подтверждения');
    if (provider === 'GitHub')
      await expect(page.getByText('TEST-CODE', { exact: true })).toBeVisible();
    await page.getByRole('button', { name: 'Проверить подключение', exact: true }).click();
    await expect(page.getByRole('status')).toContainText('Подключено');
  });
}

test('Instagram imports real file metadata separately from professional API connection', async ({
  page,
}) => {
  await nativeConnections(page);
  await openConnections(page);
  await page.getByRole('tab', { name: 'Instagram', exact: true }).click();
  await page.getByRole('button', { name: 'Открыть экспорт Instagram', exact: true }).click();
  expect(await page.evaluate(() => Reflect.get(window, 'connectionCalls').at(-1))).toEqual({
    command: 'open_connection_page',
    args: { page: 'instagram-export' },
  });
  await expect(page.getByText(/Центр аккаунтов → Ваша информация и разрешения/)).toBeVisible();
  await expect(page.getByRole('status')).not.toContainText('Подключено');
  await page.getByRole('button', { name: 'Импортировать JSON или текст', exact: true }).click();
  await expect(page.getByText('messages.json · 4 объекта', { exact: true })).toBeVisible();
  await expect(page.getByRole('status')).not.toContainText('Подключено');
  await page.getByRole('tab', { name: 'API Business / Creator', exact: true }).click();
  await page.getByLabel('Токен Instagram', { exact: true }).fill('bad-test-token');
  await page.getByRole('button', { name: 'Подключить Instagram', exact: true }).click();
  await expect(page.getByRole('alert')).toBeVisible();
  await expect(page.getByRole('alert')).not.toContainText('bad-test-token');
  await expect(page.getByLabel('Токен Instagram', { exact: true })).toHaveValue('');
  await page.getByLabel('Токен Instagram', { exact: true }).fill('good-test-token');
  await page.getByRole('button', { name: 'Подключить Instagram', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Подключено');
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage }))).not.toContain(
    'test-token',
  );
});

async function startTelegramQr(page: Page) {
  await nativeConnections(page);
  await openConnections(page);
  await page.getByLabel('API ID Telegram', { exact: true }).fill('12345');
  await page.getByLabel('API hash Telegram', { exact: true }).fill('test-api-hash');
  await page.getByRole('button', { name: 'Сохранить настройки Telegram' }).click();
  await page.getByRole('button', { name: 'Войти по QR-коду', exact: true }).click();
  await expect(
    page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true }),
  ).toBeVisible();
}

test('VK browser entry and reopening are explicit and keep pending confirmation', async ({
  page,
}) => {
  await nativeConnections(page);
  await openConnections(page);
  await page.getByRole('tab', { name: 'VK', exact: true }).click();
  await expect(page.getByLabel('Client ID VK', { exact: true })).toHaveValue('54881156');
  await page.getByLabel('Client ID VK', { exact: true }).fill('test-client');
  await page.getByRole('button', { name: 'Сохранить настройки VK' }).click();
  expect(
    await page.evaluate(() =>
      Reflect.get(window, 'connectionCalls').filter(
        (call: { command: string }) => call.command === 'begin_connection',
      ),
    ),
  ).toHaveLength(0);
  await page.getByRole('button', { name: 'Войти в VK через браузер', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Ожидаем подтверждения');
  await page.getByRole('button', { name: 'Открыть браузер ещё раз', exact: true }).click();
  expect(await page.evaluate(() => Reflect.get(window, 'connectionCalls').at(-1))).toEqual({
    command: 'open_connection_page',
    args: { page: 'vk-auth' },
  });
  await expect(page.getByRole('status')).toContainText('Ожидаем подтверждения');
  await page.getByRole('button', { name: 'Отменить подключение', exact: true }).click();
  await page.getByRole('button', { name: 'Настроить VK', exact: true }).click();
  await expect(page.getByLabel('Client ID VK', { exact: true })).toHaveValue('test-client');
});

test('Telegram QR rotates in place and fits the native window', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await startTelegramQr(page);
  const qr = page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true });
  const first = await qr.getAttribute('src');
  await page.evaluate(() => Reflect.get(window, 'rotateTelegramQr')());
  await expect(qr).not.toHaveAttribute('src', first!);
  await expect(page.getByText('QR-код обновляется автоматически.', { exact: true })).toBeVisible();
  await expect(page.getByText(/Завершите вход в открывшемся браузере/)).toHaveCount(0);
  const box = await qr.boundingBox();
  expect(box).not.toBeNull();
  expect(box!.width).toBeGreaterThanOrEqual(160);
  expect(box!.x + box!.width).toBeLessThanOrEqual(640);
  expect(box!.y + box!.height).toBeLessThanOrEqual(600);
  const cancel = page.getByRole('button', { name: 'Отменить подключение', exact: true });
  const dialog = await page.getByRole('dialog', { name: 'Настройки PROTEUS' }).boundingBox();
  expect(dialog!.y + dialog!.height).toBeLessThanOrEqual(600);
  await cancel.focus();
  await expect(cancel).toBeFocused();
  await page.screenshot({ path: testInfo.outputPath('telegram-qr-640.png') });
  expect(await page.evaluate(() => JSON.stringify({ ...localStorage }))).not.toContain(
    'data:image',
  );
  await page.keyboard.press('Enter');
  await expect(qr).toHaveCount(0);
  await expect(page.getByRole('status')).toContainText('Не подключено');
});

test('Telegram QR transitions to password without keeping the QR image', async ({ page }) => {
  await startTelegramQr(page);
  await page.evaluate(() =>
    Reflect.get(window, 'setConnectionState')('telegram', { status: 'password-required' }),
  );
  await expect(page.getByLabel('Пароль Telegram', { exact: true })).toBeVisible();
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
  await page.getByLabel('Пароль Telegram', { exact: true }).fill('test-2fa-password');
  await page.getByRole('button', { name: 'Продолжить', exact: true }).click();
  await expect(page.getByRole('status')).toContainText('Подключено');
  await expect(
    page.getByRole('button', { name: 'Войти по номеру телефона', exact: true }),
  ).toHaveCount(0);
});

test('Telegram QR can switch to the native phone flow', async ({ page }) => {
  await startTelegramQr(page);
  await page.evaluate(() => Reflect.set(window, 'holdNextConnectionAction', true));
  await page
    .getByRole('button', { name: 'Отменить вход по QR и войти по номеру', exact: true })
    .click();
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
  await page.evaluate(() => Reflect.get(window, 'releaseConnectionAction')());
  await expect(page.getByLabel('Номер телефона Telegram', { exact: true })).toBeVisible();
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
  expect(await page.evaluate(() => Reflect.get(window, 'connectionCalls').at(-1))).toEqual({
    command: 'begin_connection',
    args: { provider: 'telegram', telegramMethod: 'phone' },
  });
});

test('Telegram polling stays serial and an old poll cannot restore a cancelled QR', async ({
  page,
}) => {
  await startTelegramQr(page);
  await page.evaluate(() => Reflect.set(window, 'holdNextConnectionPoll', true));
  await expect
    .poll(() => page.evaluate(() => typeof Reflect.get(window, 'releaseConnectionPoll')))
    .toBe('function');
  const count = await page.evaluate(() => Reflect.get(window, 'connectionPollCount'));
  await page.waitForTimeout(3200);
  expect(await page.evaluate(() => Reflect.get(window, 'connectionPollCount'))).toBe(count);
  await page.getByRole('button', { name: 'Отменить подключение', exact: true }).click();
  await page.evaluate(() => Reflect.get(window, 'releaseConnectionPoll')());
  await expect(page.getByRole('status')).toContainText('Не подключено');
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
});

test('Telegram QR recovers after a transient status poll failure', async ({ page }) => {
  await startTelegramQr(page);
  await page.evaluate(() => Reflect.set(window, 'rejectNextConnectionPoll', true));
  await expect(page.getByRole('alert')).toContainText('Не удалось получить состояние подключений');
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
  await expect(
    page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true }),
  ).toBeVisible();
  await expect(page.getByRole('alert')).toHaveCount(0);
});

test('Telegram action errors and panel closure discard the displayed QR', async ({ page }) => {
  await startTelegramQr(page);
  await page.evaluate(() => Reflect.set(window, 'rejectNextConnectionAction', true));
  await page
    .getByRole('button', { name: 'Отменить вход по QR и войти по номеру', exact: true })
    .click();
  await expect(page.getByRole('alert')).toContainText('Не удалось выполнить подключение');
  await expect(page.getByRole('alert')).not.toContainText('test-native-credential-error');
  await page.waitForTimeout(1700);
  await expect(page.getByRole('alert')).toContainText('Не удалось выполнить подключение');
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
  await page.evaluate(() => Reflect.set(window, 'holdNextConnectionAction', true));
  await page
    .getByRole('button', { name: 'Отменить вход по QR и войти по номеру', exact: true })
    .click();
  await page.getByRole('button', { name: 'Закрыть настройки', exact: true }).click();
  await page.evaluate(() => Reflect.get(window, 'releaseConnectionAction')());
  await expect(page.getByRole('dialog', { name: 'Настройки PROTEUS' })).toHaveCount(0);
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
});

test('Telegram QR disappears on provider switch and panel close', async ({ page }) => {
  await startTelegramQr(page);
  await page.getByRole('tab', { name: 'VK', exact: true }).click();
  await expect(page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true })).toHaveCount(
    0,
  );
  await page.getByRole('button', { name: 'Закрыть настройки', exact: true }).click();
  await expect(page.locator('img[src^="data:image/svg+xml;base64,"]')).toHaveCount(0);
});

test('Telegram rejects nonlocal or oversized QR images', async ({ page }) => {
  await startTelegramQr(page);
  for (const value of [
    'https://example.invalid/secret-qr',
    `data:image/svg+xml;base64,${'A'.repeat(131073)}`,
  ]) {
    await page.evaluate(() => Reflect.get(window, 'rotateTelegramQr')());
    await expect(
      page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true }),
    ).toBeVisible();
    await page.evaluate(
      (qr) => Reflect.get(window, 'setConnectionState')('telegram', { 'qr-code': qr }),
      value,
    );
    await expect(
      page.getByRole('img', { name: 'QR-код входа в Telegram', exact: true }),
    ).toHaveCount(0);
  }
});
