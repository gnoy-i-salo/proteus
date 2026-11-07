import { expect, test } from '@playwright/test';

for (const platform of ['MacIntel', 'Win32', 'Linux x86_64']) {
  test(`pixel window controls preserve ${platform} semantics and geometry`, async ({ page }) => {
    await page.addInitScript((platform) => {
      Object.defineProperty(navigator, 'platform', { value: platform });
      const calls: { command: string; args: Record<string, unknown> }[] = [];
      let fullscreen = false;
      Reflect.set(window, 'windowCalls', calls);
      Reflect.set(window, 'isTauri', true);
      Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
      Reflect.set(window, '__TAURI_INTERNALS__', {
        metadata: { currentWindow: { label: 'main' } },
        transformCallback: () => 1,
        async invoke(command: string, args: Record<string, unknown> = {}) {
          if (command.startsWith('plugin:window|')) {
            calls.push({ command, args });
            if (Reflect.get(window, 'windowFailure')) throw new Error('Native failure');
          }
          if (command === 'plugin:window|is_fullscreen') return fullscreen;
          if (command === 'plugin:window|set_fullscreen') fullscreen = args.value === true;
          if (command === 'cli_status')
            return [
              { provider: 'codex', available: false, path: null },
              { provider: 'kimi', available: false, path: null },
            ];
          if (command === 'take_pending_presets') return [];
          return 1;
        },
      });
    }, platform);
    await page.setViewportSize({ width: 640, height: 600 });
    await page.goto('/');
    const bar = page.locator('header').first();
    const controls = bar.getByRole('button');
    const labels = await controls.evaluateAll((buttons) =>
      buttons.map((button) => button.getAttribute('aria-label')),
    );
    expect(labels).toEqual(
      platform === 'MacIntel'
        ? ['Закрыть окно', 'Свернуть окно', 'Включить или выключить полноэкранный режим']
        : ['Свернуть окно', 'Развернуть или восстановить окно', 'Закрыть окно'],
    );
    for (const button of await controls.all()) {
      const rect = await button.boundingBox();
      expect(rect?.width).toBe(30);
      expect(rect?.height).toBe(28);
    }
    const first = await controls.first().boundingBox();
    expect(first!.x < 100).toBe(platform === 'MacIntel');
    if (platform === 'MacIntel') {
      const toggle = page.getByRole('button', {
        name: 'Включить или выключить полноэкранный режим',
      });
      await toggle.click();
      await toggle.click();
      expect(
        await page.evaluate(() =>
          Reflect.get(window, 'windowCalls')
            .filter((call: { command: string }) => call.command === 'plugin:window|set_fullscreen')
            .map((call: { args: { value: boolean } }) => call.args.value),
        ),
      ).toEqual([true, false]);
    } else {
      await page.getByRole('button', { name: 'Развернуть или восстановить окно' }).click();
      expect(await page.evaluate(() => Reflect.get(window, 'windowCalls').at(-1).command)).toBe(
        'plugin:window|toggle_maximize',
      );
    }
    const mainBefore = await page.getByRole('main').boundingBox();
    await page.evaluate(() => Reflect.set(window, 'windowFailure', true));
    await page.getByRole('button', { name: 'Свернуть окно' }).click();
    await expect(page.getByRole('alert')).toContainText('Не удалось изменить окно');
    expect(await page.getByRole('main').boundingBox()).toEqual(mainBefore);
    await page.evaluate(() => Reflect.set(window, 'windowFailure', false));
    await page.getByRole('button', { name: 'Свернуть окно' }).click();
    await expect(page.getByRole('alert')).toHaveCount(0);
  });
}

test('browser preview has no native window controls', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByText('WEB PREVIEW')).toBeVisible();
  await expect(page.locator('header').first().getByRole('button')).toHaveCount(0);
});
