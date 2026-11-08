import { expect, test } from '@playwright/test';

test('local search settings explain native availability without pretending a model is ready', async ({
  page,
}) => {
  await page.setViewportSize({ width: 640, height: 600 });
  await page.goto('/');
  await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
  await page.getByLabel('Общий провайдер', { exact: true }).selectOption('kimi');
  await page.getByRole('button', { name: 'Локальный поиск', exact: true }).click();
  const panel = page.getByRole('region', { name: 'Локальная модель поиска' });
  await expect(panel).toContainText('настольном приложении');
  await expect(panel).toContainText('не проверены');
  await expect(panel.getByRole('button', { name: 'Загрузить модель' })).toBeDisabled();
  await expect(page.getByRole('button', { name: 'Назад к моделям CLI' })).toBeFocused();
  const rect = await panel.boundingBox();
  expect(rect!.x).toBeGreaterThanOrEqual(0);
  expect(rect!.y + rect!.height).toBeLessThanOrEqual(600);
  await page.getByRole('button', { name: 'Назад к моделям CLI' }).click();
  await expect(page.getByLabel('Провайдер STALKER', { exact: true })).toBeVisible();
  await expect(page.getByLabel('Общий провайдер', { exact: true })).toHaveValue('kimi');
  await expect(page.getByRole('button', { name: 'Локальный поиск', exact: true })).toBeFocused();
});

for (const font of ['system', 'fallback'] as const) {
  test(`native model download is explicit and the interface distinguishes disk files from RAM (${font})`, async ({
    page,
  }) => {
    await page.setViewportSize({ width: 640, height: 600 });
    await page.addInitScript(() => {
      const calls: string[] = [];
      const callbacks = new Map<number, (event: unknown) => void>();
      let progressHandler = 0;
      let ready = false;
      Reflect.set(window, 'modelCalls', calls);
      Reflect.set(window, 'isTauri', true);
      Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
      Reflect.set(window, '__TAURI_INTERNALS__', {
        metadata: { currentWindow: { label: 'main' } },
        transformCallback: (callback: (event: unknown) => void) => {
          const id = callbacks.size + 1;
          callbacks.set(id, callback);
          return id;
        },
        invoke(command: string, args: Record<string, unknown> = {}) {
          calls.push(command);
          if (command === 'plugin:event|listen' && args.event === 'local-model-progress')
            progressHandler = Number(args.handler);
          if (command === 'local_model_state')
            return Promise.resolve({
              schemaVersion: 1,
              modelId: 'intfloat/multilingual-e5-small',
              modelRevision: 'a'.repeat(40),
              packageBytes: 487724985,
              runtimeLoaded: false,
              operationActive: false,
            });
          if (command === 'inspect_local_model')
            return Promise.resolve(ready ? 'ready' : 'missing');
          if (command === 'download_local_model') {
            callbacks.get(progressHandler)?.({
              event: 'local-model-progress',
              id: 1,
              payload: {
                schemaVersion: 1,
                operationId: args.operationId,
                artifactsVerified: 1,
                bytesVerified: 100,
              },
            });
            return new Promise((resolve) => {
              Reflect.set(window, 'finishModelDownload', () => {
                ready = true;
                resolve({
                  schemaVersion: 1,
                  operationId: args.operationId,
                  artifactsVerified: 3,
                  bytesVerified: 487724985,
                });
              });
            });
          }
          if (
            command === 'cli_status' ||
            command === 'take_pending_presets' ||
            command === 'connection_statuses'
          )
            return Promise.resolve([]);
          return Promise.resolve(1);
        },
      });
    });
    await page.goto('/');
    await page.getByRole('button', { name: 'Настройки PROTEUS' }).click();
    await page.getByRole('button', { name: 'Локальный поиск', exact: true }).click();
    const panel = page.getByRole('region', { name: 'Локальная модель поиска' });
    await expect(panel).toContainText('466 МиБ');
    const calls = () => page.evaluate(() => Reflect.get(window, 'modelCalls') as string[]);
    expect(await calls()).not.toContain('download_local_model');
    await panel.getByRole('button', { name: 'Проверить файлы' }).click();
    await expect(panel).toContainText('ещё не загружены');
    await panel.getByRole('button', { name: 'Загрузить модель' }).click();
    await expect(panel).toContainText('Проверено файлов: 1 из 3');
    await page.addStyleTag({
      content: `:root { --reading: 17.5px; --caption: 15px; ${font === 'fallback' ? "font-family: 'Courier New', monospace;" : ''} }`,
    });
    const activeBounds = await page
      .getByRole('dialog', { name: 'Настройки PROTEUS' })
      .boundingBox();
    expect(activeBounds!.y).toBeGreaterThanOrEqual(0);
    expect(activeBounds!.y + activeBounds!.height).toBeLessThanOrEqual(600);
    await page.evaluate(() => (Reflect.get(window, 'finishModelDownload') as () => void)());
    await expect(panel).toContainText('загружен и проверен');
    await expect(panel.locator('dl > div').last()).toHaveText('В памяти:не загружена');
    await expect(panel.getByRole('button', { name: 'Освободить память' })).toBeDisabled();
    expect((await calls()).filter((command) => command === 'download_local_model')).toHaveLength(1);
    await page.addStyleTag({
      content: `:root { --reading: 17.5px; --caption: 15px; ${font === 'fallback' ? "font-family: 'Courier New', monospace;" : ''} }`,
    });
    const dialog = page.getByRole('dialog', { name: 'Настройки PROTEUS' });
    const bounds = await dialog.boundingBox();
    expect(bounds!.y).toBeGreaterThanOrEqual(0);
    expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(600);
  });
}
