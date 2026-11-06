import { expect, test } from '@playwright/test';

const callbackPath = '/proteus/auth/';
const syntheticCode = 'synthetic-code-for-browser-test';
const syntheticState = 'abcdefghijklmnopqrstuvwxyz0123456789';
const syntheticDevice = 'synthetic-device';
const validQuery = new URLSearchParams({
  code: syntheticCode,
  state: syntheticState,
  device_id: syntheticDevice,
}).toString();

test('valid callback returns a fixed native handoff after clearing the browser URL', async ({
  page,
}) => {
  const requests: { url: string; type: string; referrer: string | undefined }[] = [];
  page.on('request', (request) =>
    requests.push({
      url: request.url(),
      type: request.resourceType(),
      referrer: request.headers()['referer'],
    }),
  );
  await page.goto(`?${validQuery}`);
  await expect(
    page.getByRole('heading', { name: 'Вернитесь в PROTEUS', exact: true }),
  ).toBeVisible();
  await expect(page).toHaveURL(/\/proteus\/auth\/$/);
  const link = page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true });
  await expect(link).toBeVisible();
  const handoff = new URL((await link.getAttribute('href'))!);
  expect(`${handoff.protocol}//${handoff.host}${handoff.pathname}`).toBe('proteus://oauth/vk');
  expect(Object.fromEntries(handoff.searchParams)).toEqual({
    v: '1',
    code: syntheticCode,
    device_id: syntheticDevice,
    state: syntheticState,
  });
  await expect(page.locator('body')).not.toContainText(syntheticCode);
  await expect(page.locator('body')).not.toContainText(syntheticState);
  await expect(page.locator('body')).not.toContainText(syntheticDevice);
  await page.evaluate(() => document.fonts.ready);
  for (const request of requests) {
    const url = new URL(request.url);
    expect(url.origin).toBe('http://127.0.0.1:1421');
    expect(url.pathname.startsWith(callbackPath)).toBe(true);
    if (request.referrer) {
      // Fonts may refer to their public stylesheet even when the document sends no referrer.
      const referrer = new URL(request.referrer);
      expect(referrer.origin).toBe(url.origin);
      expect(referrer.pathname.startsWith(`${callbackPath}assets/`)).toBe(true);
      expect(referrer.search).toBe('');
      expect(referrer.hash).toBe('');
    }
    if (request.type !== 'document') expect(url.search).toBe('');
  }
  expect(
    await page.evaluate(async () => ({
      local: Object.keys(localStorage),
      session: Object.keys(sessionStorage),
      cookie: document.cookie,
      databases: await indexedDB.databases(),
      caches: await caches.keys(),
    })),
  ).toEqual({ local: [], session: [], cookie: '', databases: [], caches: [] });
  await expect(page.locator('meta[name="referrer"]')).toHaveAttribute('content', 'no-referrer');
  await expect(page.locator('meta[http-equiv="Content-Security-Policy"]')).toHaveAttribute(
    'content',
    /connect-src 'none'/,
  );
});

test('direct production callback path and reload show idle without creating a connection', async ({
  page,
}) => {
  const response = await page.goto(callbackPath);
  expect(response?.status()).toBe(200);
  await expect(page.getByRole('heading', { name: 'Подключение VK', exact: true })).toBeVisible();
  await expect(page.getByTestId('auth-status')).toContainText('Начните подключение в PROTEUS');
  await expect(page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true })).toHaveCount(0);
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Подключение VK', exact: true })).toBeVisible();
});

test('reload after a callback cannot reuse its authorization response', async ({ page }) => {
  await page.goto(`?${validQuery}`);
  await expect(page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true })).toBeVisible();
  await page.reload();
  await expect(page.getByRole('heading', { name: 'Подключение VK', exact: true })).toBeVisible();
  await expect(page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true })).toHaveCount(0);
});

for (const [name, query] of [
  ['missing required fields', '?code=synthetic-only-code'],
  ['duplicate code', `?${validQuery}&code=second-code`],
  ['access token injection', `?${validQuery}&access_token=synthetic-token-must-not-leak`],
  ['redirect override', `?${validQuery}&redirect_uri=https%3A%2F%2Fexample.invalid%2F`],
  ['conflicting error and code', `?${validQuery}&error=access_denied`],
  ['fragment token injection', `?${validQuery}#access_token=synthetic-token-must-not-leak`],
  ['malformed encoding', `?code=%ZZ&state=${syntheticState}&device_id=${syntheticDevice}`],
] as const) {
  test(`rejects ${name} and clears all callback parameters`, async ({ page }) => {
    await page.goto(query);
    await expect(
      page.getByRole('heading', { name: 'Не удалось проверить ссылку', exact: true }),
    ).toBeVisible();
    await expect(page).toHaveURL(/\/proteus\/auth\/$/);
    await expect(page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true })).toHaveCount(
      0,
    );
    await expect(page.locator('body')).not.toContainText(syntheticCode);
    await expect(page.locator('body')).not.toContainText('synthetic-token-must-not-leak');
    expect(await page.evaluate(() => localStorage.length + sessionStorage.length)).toBe(0);
  });
}

for (const [error, expectedError] of [
  ['access_denied', 'access_denied'],
  ['unexpected-provider-detail', 'provider_error'],
] as const) {
  test(`normalizes ${error} without displaying or forwarding provider descriptions`, async ({
    page,
  }) => {
    const query = new URLSearchParams({
      error,
      state: syntheticState,
      error_description: 'untrusted-provider-description <script>window.injected=true</script>',
    });
    await page.goto(`?${query.toString()}`);
    await expect(
      page.getByRole('heading', { name: 'Подключение не завершено', exact: true }),
    ).toBeVisible();
    await expect(page).toHaveURL(/\/proteus\/auth\/$/);
    const link = page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true });
    const handoff = new URL((await link.getAttribute('href'))!);
    expect(`${handoff.protocol}//${handoff.host}${handoff.pathname}`).toBe('proteus://oauth/vk');
    expect(Object.fromEntries(handoff.searchParams)).toEqual({
      v: '1',
      error: expectedError,
      state: syntheticState,
    });
    await expect(page.locator('body')).not.toContainText('untrusted-provider-description');
    expect(await page.evaluate(() => Reflect.get(window, 'injected'))).toBeUndefined();
  });
}

test('cannot offer a native handoff if browser history cannot be scrubbed', async ({ page }) => {
  await page.addInitScript(() => {
    history.replaceState = () => {
      throw new Error('History unavailable in this test');
    };
  });
  await page.goto(`?${validQuery}`);
  await expect(
    page.getByRole('heading', { name: 'Не удалось проверить ссылку', exact: true }),
  ).toBeVisible();
  await expect(page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true })).toHaveCount(0);
  await expect(page.locator('body')).not.toContainText(syntheticCode);
});

for (const [width, height] of [
  [320, 720],
  [768, 1024],
  [1440, 735],
] as const) {
  test(`return action remains visible and keyboard-operable at ${width}×${height}`, async ({
    page,
  }) => {
    await page.setViewportSize({ width, height });
    await page.goto(`?${validQuery}`);
    const link = page.getByRole('link', { name: 'Вернуться в PROTEUS', exact: true });
    await expect(link).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
    await page.keyboard.press('Tab');
    await expect(link).toBeFocused();
    // Exercise the real UI event while preventing this browser test from launching a native app.
    await link.evaluate((element) =>
      element.addEventListener('click', (event) => event.preventDefault()),
    );
    await page.keyboard.press('Enter');
    await expect(page.getByRole('status')).toContainText('Запрос на открытие PROTEUS отправлен.');
    await expect(link).toBeVisible();
    await page.screenshot({ path: test.info().outputPath('auth-callback.png') });
  });
}
