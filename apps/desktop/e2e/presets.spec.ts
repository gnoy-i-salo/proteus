import { expect, test } from '@playwright/test';
const preset = JSON.stringify({
  format: 'proteus-personas',
  version: 1,
  task: 'Общая задача',
  people: [
    { label: 'Анна', context: 'Точный контекст', 'public-urls': ['https://example.org/a'] },
    { label: 'Борис', context: 'Импровизация', 'public-urls': [] },
  ],
});
const expectedPreset = {
  format: 'proteus-personas',
  version: 2,
  task: 'Общая задача',
  people: [
    { label: 'Анна', context: 'Точный контекст', publicUrls: ['https://example.org/a'] },
    { label: 'Борис', context: 'Импровизация', publicUrls: [] },
  ],
};
for (const mode of ['import', 'system'] as const) {
  test(`${mode} .proteus populates both people without starting CLI and exports a round trip`, async ({
    page,
  }) => {
    await page.addInitScript(
      ({ preset, mode }) => {
        Reflect.set(window, 'isTauri', true);
        Reflect.set(window, '__TAURI_EVENT_PLUGIN_INTERNALS__', { unregisterListener() {} });
        Reflect.set(window, '__TAURI_INTERNALS__', {
          transformCallback: () => 1,
          async invoke(command: string, args: { data?: string }) {
            if (command === 'cli_status')
              return [
                { provider: 'codex', available: false, path: null },
                { provider: 'kimi', available: false, path: null },
              ];
            if (command === 'take_pending_presets')
              return mode === 'system' ? [{ Ok: preset }] : [];
            if (command === 'import_preset') return preset;
            if (command === 'export_preset') {
              Reflect.set(window, 'exportedPreset', args.data);
              return '/tmp/personas.proteus';
            }
            if (command === 'run_cli') throw new Error('Must not start CLI');
            return 1;
          },
        });
      },
      { preset, mode },
    );
    await page.setViewportSize({ width: 640, height: 600 });
    await page.goto('/');
    if (mode === 'import')
      await page.getByRole('button', { name: 'Импорт .proteus', exact: true }).click();
    await expect(page.getByLabel('Имя персоны A')).toHaveValue('Анна');
    await expect(page.getByLabel('Имя персоны B')).toHaveValue('Борис');
    await expect(page.getByLabel('Контекст A', { exact: true })).toHaveValue('Точный контекст');
    await expect(page.getByLabel('Контекст B', { exact: true })).toHaveValue('Импровизация');
    await expect(page.getByLabel('Общая задача', { exact: true })).toHaveValue('Общая задача');
    await page.getByRole('button', { name: 'Экспорт .proteus', exact: true }).click();
    expect(JSON.parse(await page.evaluate(() => Reflect.get(window, 'exportedPreset')))).toEqual(
      expectedPreset,
    );
    await expect(page.getByRole('button', { name: 'Начать эксперимент' })).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(
      true,
    );
  });
}
test('invalid browser import preserves the existing form', async ({ page }) => {
  await page.goto('/');
  await page.getByLabel('Имя персоны A').fill('Сохранить меня');
  await page.getByLabel('Файл пресета').setInputFiles({
    name: 'broken.proteus',
    mimeType: 'application/json',
    buffer: Buffer.from('{}'),
  });
  await expect(page.getByRole('alert')).toContainText('формат');
  await expect(page.getByLabel('Имя персоны A')).toHaveValue('Сохранить меня');
});

test('long Unicode presets survive import, resize and export without changing their text', async ({
  page,
}) => {
  const data = JSON.parse(preset);
  data.people[0].context = 'Точный  текст ✨\n'.repeat(100);
  data.task = 'Задача с пробелами  и переносами.\n'.repeat(50);
  await page.goto('/');
  await page.getByLabel('Файл пресета').setInputFiles({
    name: 'long.proteus',
    mimeType: 'application/json',
    buffer: Buffer.from(JSON.stringify(data)),
  });
  await page.setViewportSize({ width: 640, height: 600 });
  await expect(page.getByLabel('Имя персоны A')).toHaveValue('Анна');
  const download = page.waitForEvent('download');
  await page.getByRole('button', { name: 'Экспорт .proteus', exact: true }).click();
  const file = await download;
  expect(file.suggestedFilename()).toBe('personas.proteus');
  const stream = await file.createReadStream();
  const chunks: Buffer[] = [];
  for await (const chunk of stream!) chunks.push(chunk);
  expect(JSON.parse(Buffer.concat(chunks).toString('utf8'))).toEqual({
    ...expectedPreset,
    task: data.task,
    people: [
      { ...expectedPreset.people[0], context: data.people[0].context },
      expectedPreset.people[1],
    ],
  });
});
