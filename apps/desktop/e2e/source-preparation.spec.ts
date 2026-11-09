import { expect, test } from '@playwright/test';
import type { Page } from '@playwright/test';
import type { IngestionJob } from '@proteus-int/contracts';
interface PreparationFixture {
  calls: { command: string; args: Record<string, unknown> }[];
  completeImport(): void;
}
async function fixture(page: Page, denyPreview = false) {
  await page.addInitScript((denyPreview: boolean) => {
    const calls: PreparationFixture['calls'] = [];
    let job: IngestionJob | undefined;
    Reflect.set(window, 'calls', calls);
    Reflect.set(window, 'completeImport', () => {
      if (job)
        job = {
          ...job,
          status: 'completed',
          coverage: { ...job.coverage, loadedDocuments: 2, loadedBytes: 128, complete: true },
          publishedSnapshotId: 'import:job',
        };
    });
    Reflect.set(window, 'isTauri', true);
    Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
    Reflect.set(window, '__TAURI_INTERNALS__', {
      transformCallback: () => 1,
      async invoke(command: string, args: Record<string, unknown> = {}) {
        await Promise.resolve();
        calls.push({ command, args });
        if (command === 'cli_status')
          return [
            { provider: 'codex', available: false, path: null },
            { provider: 'kimi', available: false, path: null },
          ];
        if (command === 'take_pending_presets' || command === 'connection_statuses') return [];
        if (command === 'pick_ingestion_source')
          return {
            schemaVersion: 1,
            sourceHandle: 'a'.repeat(32),
            label: 'Ёлка exact archive.json',
            inputBytes: 128,
          };
        if (command === 'preview_ingestion_source' && denyPreview)
          throw new Error('Доступ к архиву отозван.');
        if (command === 'preview_ingestion_source')
          return {
            schemaVersion: 1,
            id: 'preview',
            sourceHandle: 'a'.repeat(32),
            sourceFormat: 'telegramDesktopJson',
            adapterVersion: 'telegram:1',
            resources: [
              {
                id: 'chat',
                label: 'Чат Ёлка — исходное имя',
                authorIds: ['author', 'friend'],
                estimatedDocuments: 2,
              },
            ],
            limitations: ['Вложения не загружены'],
          };
        if (command === 'create_ingestion_binding') {
          const request = args.request as Record<string, unknown>;
          const selection = { ...request };
          delete selection.previewId;
          delete selection.allowModelProcessing;
          delete selection.allowExport;
          return {
            ...selection,
            schemaVersion: 1,
            id: 'binding',
            provider: 'telegram',
            accountId: null,
            grantId: 'grant',
            policyVersion: 'ingestionBinding:1',
          };
        }
        if (command === 'start_ingestion_job') {
          const request = args.request as {
            operationId: string;
            selection: IngestionJob['selection'];
            budget: IngestionJob['budget'];
          };
          job = {
            schemaVersion: 1,
            id: request.operationId,
            selection: request.selection,
            budget: request.budget,
            status: 'running',
            phase: 'reading',
            checkpoint: {
              adapterVersion: 'telegram:1',
              cursor: null,
              committedBatch: 0,
              stagingGeneration: 1,
            },
            coverage: {
              loadedDocuments: 0,
              indexedDocuments: 0,
              analyzedDocuments: 0,
              usedDocuments: 0,
              loadedBytes: 0,
              totalDocuments: null,
              complete: false,
              limitations: [],
            },
            publishedSnapshotId: null,
            error: null,
          };
          return job;
        }
        if (command === 'get_ingestion_job') return job;
        if (
          ['pause_ingestion_job', 'resume_ingestion_job', 'cancel_ingestion_job'].includes(
            command,
          ) &&
          job
        ) {
          job = {
            ...job,
            status: command.startsWith('pause')
              ? 'paused'
              : command.startsWith('resume')
                ? 'running'
                : 'cancelled',
          };
          return job;
        }
        if (command === 'prepare_ingestion_job' && job)
          return {
            schemaVersion: 1,
            id: 'prepared',
            corpusId: `corpus:${job.id}`,
            parentSnapshotId: 'import:job',
            createdAt: '2026-11-09T00:00:00.000Z',
            bindingIds: ['binding'],
            documentRefs: [],
            episodeIds: ['episode'],
            splitVersion: 'episodeSplit:1:test',
            generation: 2,
            coverage: { ...job.coverage, indexedDocuments: 2 },
          };
        if (
          command.startsWith('plugin:') ||
          command === 'release_ingestion_source' ||
          command === 'cancel_ingestion_operation'
        )
          return null;
        throw new Error(`Unexpected native command: ${command}`);
      },
    });
  }, denyPreview);
}
for (const [width, height, scale] of [
  [640, 600, 1],
  [1024, 768, 1],
  [1280, 800, 1],
  [640, 600, 1.25],
] as const) {
  test(`selected archive imports and prepares without model access at ${width}×${height} text ${scale}`, async ({
    page,
  }, testInfo) => {
    await fixture(page);
    await page.setViewportSize({ width, height });
    await page.goto('/');
    await page.addStyleTag({
      content: `:root { --reading: ${14 * scale}px; --caption: ${12 * scale}px; }`,
    });
    await page.getByRole('button', { name: 'Источники персоны A' }).click();
    await page.getByRole('button', { name: 'Подготовить корпус персоны A' }).click();
    const dialog = page.getByRole('dialog', { name: 'Подготовка источника A' });
    await dialog.getByRole('button', { name: 'Выбрать файл' }).click();
    await expect(dialog.getByText('Ёлка exact archive.json')).toBeVisible();
    expect(
      await dialog.evaluate((element) => element.scrollHeight - element.clientHeight),
    ).toBeLessThanOrEqual(1);
    await dialog.getByLabel('Ресурс').selectOption('chat');
    await dialog.getByLabel('Автор персоны').selectOption('author');
    await page.screenshot({ path: testInfo.outputPath('source-selection.png') });
    await dialog.getByRole('button', { name: 'Выбрать контекст и период' }).click();
    await dialog.getByLabel('Собеседники в контексте (необязательно)').selectOption('friend');
    await dialog.getByRole('button', { name: 'Перейти к разрешениям' }).click();
    await page.screenshot({ path: testInfo.outputPath('source-permissions.png') });
    expect(
      await dialog.evaluate((element) => element.scrollHeight - element.clientHeight),
    ).toBeLessThanOrEqual(1);
    await dialog.getByRole('button', { name: 'Ограничения источника: 1' }).click();
    await expect(dialog.getByLabel('Ограничения источника', { exact: true })).toContainText(
      'Вложения не загружены',
    );
    await dialog.getByRole('button', { name: 'Вернуться к разрешениям' }).click();
    await dialog.getByRole('button', { name: 'Назад к выбору' }).click();
    await expect(dialog.getByLabel('Собеседники в контексте (необязательно)')).toHaveValues([
      'friend',
    ]);
    await dialog.getByRole('button', { name: 'Перейти к разрешениям' }).click();
    await dialog.getByLabel('Разрешаю локальный импорт выбранного материала').check();
    await expect(dialog.getByLabel('Разрешаю обработку моделями')).not.toBeChecked();
    await expect(dialog.getByLabel('Разрешаю экспорт исходного материала')).not.toBeChecked();
    await dialog.getByRole('button', { name: 'Импортировать выбранное' }).click();
    await expect(dialog.getByText('Импорт выполняется', { exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: 'Пауза', exact: true }).click();
    await expect(dialog.getByText('Импорт приостановлен', { exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: 'Продолжить импорт' }).click();
    await page.evaluate(() => (window as unknown as PreparationFixture).completeImport());
    await expect(dialog.getByText('Импорт завершён', { exact: true })).toBeVisible();
    await dialog.getByRole('button', { name: 'Подготовить корпус' }).click();
    await expect(dialog.getByText('Корпус подготовлен', { exact: true })).toBeVisible();
    await expect(dialog.getByText('Изучено моделью: 0', { exact: true })).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('source-prepared.png') });
    const calls = await page.evaluate(() => (window as unknown as PreparationFixture).calls);
    const request = calls.find((call) => call.command === 'create_ingestion_binding')?.args.request;
    expect(request).toMatchObject({
      targetAuthorId: 'author',
      role: 'own',
      allowModelProcessing: false,
      allowExport: false,
      allowedContextAuthorIds: ['friend'],
    });
    expect(calls.some((call) => call.command === 'run_cli')).toBe(false);
    expect(await dialog.evaluate((element) => element.scrollWidth <= element.clientWidth + 1)).toBe(
      true,
    );
    await dialog.getByRole('button', { name: 'Закрыть подготовку источника' }).click();
    await expect(dialog).not.toBeVisible();
  });
}
test('browser explains native-only preparation without pretending to select a file', async ({
  page,
}) => {
  await page.goto('/');
  await page.getByRole('button', { name: 'Источники персоны A' }).click();
  await page.getByRole('button', { name: 'Подготовить корпус персоны A' }).click();
  await expect(
    page.getByText('Подготовка источников доступна в настольном приложении PROTEUS.'),
  ).toBeVisible();
  await expect(page.getByRole('button', { name: 'Выбрать файл' })).not.toBeVisible();
});

test('native preview denial never enables import and releases the selected capability on close', async ({
  page,
}) => {
  await fixture(page, true);
  await page.goto('/');
  await page.getByRole('button', { name: 'Источники персоны A' }).click();
  await page.getByRole('button', { name: 'Подготовить корпус персоны A' }).click();
  const dialog = page.getByRole('dialog', { name: 'Подготовка источника A' });
  await dialog.getByRole('button', { name: 'Выбрать файл' }).click();
  await expect(dialog.getByRole('alert')).toHaveText('Доступ к архиву отозван.');
  await expect(dialog.getByRole('button', { name: 'Импортировать выбранное' })).not.toBeVisible();
  await dialog.getByRole('button', { name: 'Закрыть подготовку источника' }).click();
  await expect
    .poll(async () =>
      (await page.evaluate(() => (window as unknown as PreparationFixture).calls)).some(
        (call) => call.command === 'release_ingestion_source',
      ),
    )
    .toBe(true);
  const calls = await page.evaluate(() => (window as unknown as PreparationFixture).calls);
  expect(calls.some((call) => call.command === 'create_ingestion_binding')).toBe(false);
});
test('closing the import screen cancels a running native job', async ({ page }) => {
  await fixture(page);
  await page.goto('/');
  await page.getByRole('button', { name: 'Источники персоны A' }).click();
  await page.getByRole('button', { name: 'Подготовить корпус персоны A' }).click();
  const dialog = page.getByRole('dialog', { name: 'Подготовка источника A' });
  await dialog.getByRole('button', { name: 'Выбрать файл' }).click();
  await dialog.getByLabel('Ресурс').selectOption('chat');
  await dialog.getByLabel('Автор персоны').selectOption('author');
  await dialog.getByRole('button', { name: 'Выбрать контекст и период' }).click();
  await dialog.getByRole('button', { name: 'Перейти к разрешениям' }).click();
  expect(
    await dialog.evaluate((element) => element.scrollHeight - element.clientHeight),
  ).toBeLessThanOrEqual(1);
  await dialog.getByLabel('Разрешаю локальный импорт выбранного материала').check();
  await dialog.getByRole('button', { name: 'Импортировать выбранное' }).click();
  await expect(dialog.getByText('Импорт выполняется', { exact: true })).toBeVisible();
  await dialog.getByRole('button', { name: 'Закрыть подготовку источника' }).click();
  await expect
    .poll(async () =>
      (await page.evaluate(() => (window as unknown as PreparationFixture).calls)).some(
        (call) => call.command === 'cancel_ingestion_job',
      ),
    )
    .toBe(true);
});
