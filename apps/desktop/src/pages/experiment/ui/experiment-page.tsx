import {
  batch,
  createEffect,
  createMemo,
  createSignal,
  For,
  on,
  onCleanup,
  onMount,
  Show,
  untrack,
} from 'solid-js';
import type { ExperimentInput, PersonInput } from '@proteus-int/contracts';
import { createModelSettings, parseModelSettings } from '@proteus-int/intelligence';
import type { ModelSettings } from '@proteus-int/intelligence';
import {
  runExperiment,
  validateExperiment,
  parsePersonaPreset,
  serializePersonaPreset,
} from '@proteus-int/pipeline';
import type { ExperimentResult, ProgressEvent, StageTrace } from '@proteus-int/pipeline';
import { ModelSettingsPanel } from '../../../features/model-settings';
import { cliProviders, cliStatus, saveExperiment } from '../../../shared/lib/cli';
import type { CliStatus } from '../../../shared/lib/cli';
import { paginateText, paginateInput } from '../../../shared/lib/pagination';
import { importPreset, exportPreset, listenPresets } from '../../../shared/lib/presets';
import { isDesktop } from '../../../shared/lib/window-controls';
import styles from './experiment-page.module.scss';
const storageKey = 'proteus-model-settings';
function readSettings() {
  try {
    return parseModelSettings(localStorage.getItem(storageKey));
  } catch {
    return createModelSettings();
  }
}
function PagedField(props: {
  label: string;
  pages: string[];
  onChange: (pages: string[]) => void;
  compact?: boolean;
  revision: number;
}) {
  const [page, setPage] = createSignal(0);
  let editor: HTMLTextAreaElement | undefined;
  const reflow = (value: string, replaceAll = false) => {
    if (!editor) return;
    const chunks = paginateInput(
      value,
      Math.max(10, Math.floor((editor.clientWidth - 20) / 8)),
      Math.max(1, Math.floor((editor.clientHeight - 18) / 18)),
    );
    const index = replaceAll ? 0 : page();
    props.onChange(
      replaceAll
        ? chunks
        : [...props.pages.slice(0, index), ...chunks, ...props.pages.slice(index + 1)],
    );
    setPage(replaceAll ? 0 : index + chunks.length - 1);
  };
  createEffect(
    on(
      () => props.revision,
      () => {
        reflow(props.pages.join(''), true);
      },
    ),
  );
  onMount(() => {
    reflow(props.pages.join(''), true);
    let dimensions = `${editor?.clientWidth}:${editor?.clientHeight}`;
    const observer = new ResizeObserver(() => {
      const next = `${editor?.clientWidth}:${editor?.clientHeight}`;
      if (next !== dimensions) reflow(props.pages.join(''), true);
      dimensions = next;
    });
    if (editor) observer.observe(editor);
    onCleanup(() => observer.disconnect());
  });
  return (
    <label class={styles['experiment__field']} data-compact={props.compact ? 'true' : 'false'}>
      <span>{props.label}</span>
      <textarea
        aria-label={props.label}
        ref={(element) => {
          editor = element;
        }}
        maxLength={4000}
        value={props.pages[page()] ?? ''}
        placeholder="Опишите своими словами…"
        onInput={(event) => reflow(event.currentTarget.value)}
      />
      <span class={styles['experiment__pager']}>
        <button
          type="button"
          aria-label={`Назад: ${props.label}`}
          disabled={page() === 0}
          onClick={() => setPage(page() - 1)}
        >
          ←
        </button>
        <small>
          {page() + 1}/{props.pages.length}
        </small>
        <button
          type="button"
          aria-label={`Далее: ${props.label}`}
          disabled={page() === props.pages.length - 1 && props.pages.length >= 35}
          onClick={() => {
            if (page() === props.pages.length - 1) props.onChange([...props.pages, '']);
            setPage(page() + 1);
          }}
        >
          →
        </button>
      </span>
    </label>
  );
}
function PublicLinks(props: {
  letter: string;
  revision: number;
  urls: string[];
  onChange: (urls: string[]) => void;
}) {
  const [index, setIndex] = createSignal(0);
  createEffect(
    on(
      () => props.revision,
      () => setIndex(0),
    ),
  );
  return (
    <div class={styles['experiment__links']}>
      <label>
        Публичные ссылки
        <input
          aria-label={`Публичная ссылка ${props.letter}`}
          type="url"
          placeholder="https://…"
          maxLength={2048}
          value={props.urls[index()] ?? ''}
          onInput={(event) =>
            props.onChange(
              props.urls.map((url, i) => (i === index() ? event.currentTarget.value : url)),
            )
          }
        />
      </label>
      <nav aria-label={`Ссылки ${props.letter}`}>
        <button
          type="button"
          aria-label={`Предыдущая ссылка ${props.letter}`}
          disabled={index() === 0}
          onClick={() => setIndex(index() - 1)}
        >
          ←
        </button>
        <span>
          {index() + 1} / {props.urls.length}
        </span>
        <button
          type="button"
          aria-label={`Следующая ссылка ${props.letter}`}
          disabled={index() >= props.urls.length - 1}
          onClick={() => setIndex(index() + 1)}
        >
          →
        </button>
        <button
          type="button"
          aria-label={`Добавить ссылку ${props.letter}`}
          title="Добавить ссылку (до 5)"
          disabled={props.urls.length >= 5}
          onClick={() => {
            props.onChange([...props.urls, '']);
            setIndex(props.urls.length - 1);
          }}
        >
          +
        </button>
        <button
          type="button"
          aria-label={`Удалить ссылку ${props.letter}`}
          title="Удалить ссылку"
          onClick={() => {
            const next = props.urls.filter((_, i) => i !== index());
            props.onChange(next.length ? next : ['']);
            setIndex(Math.max(0, index() - 1));
          }}
        >
          ×
        </button>
      </nav>
    </div>
  );
}
function TraceReader(props: { stages: StageTrace[]; running: boolean; person: number }) {
  const [chosen, setChosen] = createSignal('');
  const stages = () =>
    props.stages.filter((item) => item.person === props.person || item.person === null);
  const key = (item: StageTrace) => `${item.person}:${item.stage}`;
  const keys = createMemo(() => stages().map(key));
  const active = () => (keys().includes(chosen()) ? chosen() : keys().at(-1));
  createEffect(
    on(
      () => props.person,
      () => setChosen(''),
    ),
  );
  return (
    <section class={styles['experiment__reasoning']} aria-label="Текст Intelligence traces">
      <Show when={keys().length} fallback={<p>Этапы этой персоны ещё не начались.</p>}>
        <For each={keys()}>
          {(id) => {
            const item = () => stages().find((stage) => key(stage) === id)!;
            const label = () =>
              `${item().person === null ? 'SYS' : item().person === 0 ? 'A' : 'B'} · ${item().stage.toUpperCase()}`;
            const text = () =>
              item()
                .trace.reasoning?.map((block) => block.text)
                .join('\n\n') ||
              (props.running && !item().trace.summary
                ? 'Ожидаем текст reasoning от CLI…'
                : 'CLI не передал текст reasoning для этого этапа.');
            return (
              <article class={styles['experiment__trace']} data-expanded={active() === id}>
                <button
                  class={styles['experiment__trace-heading']}
                  aria-label={label()}
                  aria-expanded={active() === id}
                  onClick={() => setChosen(id)}
                >
                  <strong>{label()}</strong>
                  <small>
                    {item().trace.provider} · {(item().trace['reasoning-ms'] / 1000).toFixed(1)} с
                  </small>
                  <span>{item().trace.summary ? '✓' : props.running ? '◇' : '—'}</span>
                </button>
                <Show when={active() === id}>
                  <p class={styles['experiment__reasoning-note']}>
                    {item().trace.summary || (props.running ? 'Выполняется…' : 'Этап не завершён')}
                    {item().trace['reasoning-truncated']
                      ? ' · Текст сокращён до лимита сохранения.'
                      : ''}
                  </p>
                  <PagedText text={text()} resetKey={id} />
                </Show>
              </article>
            );
          }}
        </For>
      </Show>
    </section>
  );
}
function PagedText(props: { text: string; resetKey?: string }) {
  const [page, setPage] = createSignal(0);
  const [size, setSize] = createSignal({ columns: 38, rows: 7 });
  let reading!: HTMLDivElement;
  onMount(() => {
    const observer = new ResizeObserver(() => {
      const paragraph = reading.querySelector('p');
      if (!paragraph) return;
      const css = getComputedStyle(paragraph);
      const frame = getComputedStyle(reading);
      const width =
        reading.clientWidth - parseFloat(frame.paddingLeft) - parseFloat(frame.paddingRight);
      const height =
        reading.clientHeight - parseFloat(frame.paddingTop) - parseFloat(frame.paddingBottom) - 48;
      setSize({
        columns: Math.max(15, Math.floor(width / (parseFloat(css.fontSize) * 0.61))),
        rows: Math.max(2, Math.floor(height / parseFloat(css.lineHeight))),
      });
    });
    observer.observe(reading);
    onCleanup(() => observer.disconnect());
  });
  const chunks = createMemo(() =>
    paginateText(props.text || 'Пока нет данных.', size().columns, size().rows),
  );
  createEffect(
    on(
      () => [props.resetKey ?? props.text, size()],
      () => setPage(0),
    ),
  );
  return (
    <div
      ref={(element) => {
        reading = element;
      }}
      class={styles['experiment__reading']}
    >
      <p>{chunks()[page()]}</p>
      <nav aria-label="Страницы текста">
        <button disabled={page() === 0} onClick={() => setPage(page() - 1)}>
          ←
        </button>
        <span>
          {page() + 1} / {chunks().length}
        </span>
        <button disabled={page() >= chunks().length - 1} onClick={() => setPage(page() + 1)}>
          →
        </button>
      </nav>
    </div>
  );
}
export function ExperimentPage() {
  const [revision, setRevision] = createSignal(0);
  const [pendingPreset, setPendingPreset] = createSignal<ExperimentInput>();
  let fileInput: HTMLInputElement | undefined;
  const [screen, setScreen] = createSignal<'start' | 'progress' | 'result'>('start');
  const [selected, setSelected] = createSignal(0);
  const [settingsOpen, setSettingsOpen] = createSignal(false);
  const [settings, setSettings] = createSignal(readSettings());
  const [available, setAvailable] = createSignal<CliStatus[]>([]);
  const [names, setNames] = createSignal(['Person A', 'Person B']);
  const [urls, setUrls] = createSignal<string[][]>([[''], ['']]);
  const [contexts, setContexts] = createSignal<string[][]>([[''], ['']]);
  const [task, setTask] = createSignal(['']);
  const [events, setEvents] = createSignal<ProgressEvent[]>([]);
  const [result, setResult] = createSignal<ExperimentResult>();
  const [running, setRunning] = createSignal(false);
  const [error, setError] = createSignal('');
  const [savedPath, setSavedPath] = createSignal('');
  const [activity, setActivity] = createSignal('Ожидаем запуск');
  const [detail, setDetail] = createSignal('analysis');
  const [reasoningStages, setReasoningStages] = createSignal<StageTrace[]>([]);
  const recordTrace = (next: StageTrace) =>
    setReasoningStages((items) => {
      const index = items.findIndex(
        (item) => item.stage === next.stage && item.person === next.person,
      );
      return index < 0 ? [...items, next] : items.map((item, i) => (i === index ? next : item));
    });
  let controller: AbortController | undefined;
  onMount(() => {
    void cliStatus()
      .then(setAvailable)
      .catch(() => setError('Не удалось проверить установленные CLI.'));
  });
  onCleanup(() => controller?.abort());
  const applyPreset = (input: ExperimentInput) => {
    batch(() => {
      setNames(input.people.map((person) => person.label));
      setUrls(
        input.people.map((person) =>
          person['public-urls'].length ? [...person['public-urls']] : [''],
        ),
      );
      setContexts(input.people.map((person) => [person.context]));
      setTask([input.task]);
      setScreen('start');
      setSelected(0);
      setError('');
      setRevision((value) => value + 1);
    });
  };
  const receivePreset = (data: string) => {
    try {
      const input = parsePersonaPreset(data);
      if (running()) setPendingPreset(input);
      else applyPreset(input);
    } catch (failure) {
      setError(String(failure));
    }
  };
  onMount(() => {
    let disposed = false;
    let stop: (() => void) | undefined;
    void listenPresets(receivePreset, setError)
      .then((unlisten) => {
        if (disposed) unlisten();
        else stop = unlisten;
      })
      .catch((failure) => setError(String(failure)));
    onCleanup(() => {
      disposed = true;
      stop?.();
    });
  });
  const currentInput = (): ExperimentInput => {
    const person = (index: number): PersonInput => ({
      label: names()[index] ?? '',
      context: contexts()[index]?.join('') ?? '',
      'public-urls': (urls()[index] ?? []).map((url) => url.trim()).filter(Boolean),
    });
    return { task: task().join(''), people: [person(0), person(1)] };
  };
  const openPreset = async () => {
    if (!isDesktop()) {
      fileInput?.click();
      return;
    }
    try {
      const data = await importPreset();
      if (data !== null) receivePreset(data);
    } catch (failure) {
      setError(String(failure));
    }
  };
  const savePreset = async () => {
    try {
      await exportPreset(serializePersonaPreset(currentInput()));
    } catch (failure) {
      setError(String(failure));
    }
  };
  const persist = (value: ModelSettings) => {
    setSettings(value);
    try {
      localStorage.setItem(storageKey, JSON.stringify(value));
    } catch {
      setError('Настройки применены, но не сохранены.');
    }
  };
  const start = async () => {
    setError('');
    setSavedPath('');
    const input = currentInput();
    try {
      validateExperiment(input);
    } catch (failure) {
      setError(String(failure));
      return;
    }
    controller = new AbortController();
    setRunning(true);
    setEvents([]);
    setSelected(0);
    setReasoningStages([]);
    setResult(undefined);
    setScreen('progress');
    try {
      const output = await runExperiment(
        input,
        settings(),
        cliProviders((event) =>
          untrack(() => {
            setActivity(
              `${event.kind === 'reasoning' ? 'Модель рассуждает' : event.kind === 'search' ? 'Поиск источников' : event.kind === 'answer' ? 'Ответ получен' : 'CLI работает'} · ${(event['elapsed-ms'] / 1000).toFixed(1)} с`,
            );
            const current = events().at(-1);
            if (event.reasoning && current && current.stage !== 'complete') {
              const previous = reasoningStages().find(
                (item) => item.stage === current.stage && item.person === current.person,
              );
              if (previous) {
                const block = event.reasoning;
                const blocks = previous.trace.reasoning ?? [];
                const reasoning = blocks.some((item) => item.id === block.id)
                  ? blocks.map((item) => (item.id === block.id ? block : item))
                  : [...blocks, block];
                recordTrace({
                  ...previous,
                  trace: { ...previous.trace, 'reasoning-ms': event['elapsed-ms'], reasoning },
                });
              }
            }
          }),
        ),
        {
          signal: controller.signal,
          onTrace: recordTrace,
          onProgress: (event) => {
            if (
              event.stage !== 'complete' &&
              !reasoningStages().some(
                (item) => item.stage === event.stage && item.person === event.person,
              )
            ) {
              recordTrace({
                stage: event.stage,
                person: event.person,
                trace: {
                  provider: settings().components[event.stage],
                  'reasoning-ms': 0,
                  'terminal-ms': 0,
                  summary: '',
                  reasoning: [],
                },
              });
            }
            setEvents((items) => [...items, event]);
          },
        },
      );
      setResult(output);
      setScreen('result');
      setDetail('analysis');
      try {
        setSavedPath(await saveExperiment(output));
      } catch {
        setError('Результат готов, но не удалось сохранить его на диск. Скачайте JSON.');
      }
    } catch (failure) {
      setActivity(controller.signal.aborted ? 'Эксперимент остановлен' : 'Ошибка этапа');
      setError(
        controller.signal.aborted
          ? 'Эксперимент остановлен. Можно вернуться к вводу и запустить заново.'
          : String(failure),
      );
    } finally {
      setRunning(false);
    }
  };
  const progress = () => events().at(-1)?.completed ?? 0;
  const persona = () => result()?.people[selected()];
  const text = () => {
    const current = persona();
    const output = result();
    if (!current || !output) return '';
    if (detail() === 'answer') return current.attempt.answer;
    if (detail() === 'archetype')
      return `${current.profile.archetype}\n\n${current.profile.skills.map((skill, index) => `${index + 1}. ${skill}`).join('\n\n')}`;
    if (detail() === 'sources')
      return current.report.evidence
        .map(
          (item) =>
            `${item.kind === 'fact' ? 'ФАКТ' : 'ГИПОТЕЗА'} · ${item.summary}\n${item['source-url'] || 'Контекст пользователя'}`,
        )
        .join('\n\n');
    const analysis = output.comparison.people[selected()];
    return `${output.comparison.summary}\n\nСИЛЬНЫЕ СТОРОНЫ\n${analysis?.strengths.join('\n')}\n\nОГРАНИЧЕНИЯ\n${analysis?.weaknesses.join('\n')}\n\nНАБЛЮДЕНИЯ\n${analysis?.observations.join('\n')}\n\nРАЗЛИЧИЯ\n${output.comparison['key-differences'].join('\n')}`;
  };
  const download = () => {
    const blob = new Blob([JSON.stringify(result(), null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const link = document.createElement('a');
    link.href = url;
    link.download = 'proteus-experiment.json';
    link.click();
    URL.revokeObjectURL(url);
  };
  return (
    <main class={styles['experiment']} data-palette={selected() === 0 ? 'turquoise' : 'violet'}>
      <Show
        when={screen() === 'start'}
        fallback={
          <div class={styles['experiment__workspace']}>
            <aside class={styles['experiment__sidebar']}>
              <div class={styles['experiment__brand']}>
                <img src="/proteus-logo.svg" alt="" />
                <strong>PROTEUS</strong>
              </div>
              <span class={styles['experiment__eyebrow']}>SELECT PERSON</span>
              <For each={names()}>
                {(name, index) => (
                  <button
                    class={styles['experiment__person']}
                    data-active={selected() === index()}
                    data-tone={index() === 0 ? 'a' : 'b'}
                    onClick={() => setSelected(index())}
                  >
                    <span class={styles['experiment__avatar']} aria-hidden="true">
                      <img src={index() === 0 ? '/person-a.svg' : '/person-b.svg'} alt="" />
                    </span>
                    <span>
                      {name}
                      <small>PERSON {index() === 0 ? 'A' : 'B'}</small>
                    </span>
                  </button>
                )}
              </For>
              <div class={styles['experiment__sidebar-bottom']}>
                <small>
                  ONE TASK.
                  <br />
                  MANY MINDS.
                </small>
                <button
                  disabled={running()}
                  onClick={() => {
                    setScreen('start');
                    setError('');
                  }}
                >
                  ← Новый запуск
                </button>
                <button
                  aria-label="Настройки PROTEUS"
                  disabled={running()}
                  onClick={() => setSettingsOpen(true)}
                >
                  ⚙ Настройки
                </button>
              </div>
            </aside>
            <section class={styles['experiment__main']}>
              <Show
                when={screen() === 'progress'}
                fallback={
                  <>
                    <header class={styles['experiment__result-banner']}>
                      <span>EXPERIMENT COMPLETE</span>
                      <h2>BSC · {persona()?.score.bsc.toFixed(3) ?? '—'}</h2>
                      <p>Отношение двух BSC: × {result()?.comparison.ratio?.toFixed(2) ?? '—'}</p>
                      <p>Экспериментальная метрика · не оценка интеллекта человека</p>
                    </header>
                    <div class={styles['experiment__result-body']}>
                      <div class={styles['experiment__result-heading']}>
                        <h3>{persona()?.profile.label}</h3>
                        <span>
                          {persona()?.score['reasoning-minutes'].toFixed(3)} мин. / оценка{' '}
                          {persona()?.report['estimated-bsc']}
                        </span>
                      </div>
                      <nav class={styles['experiment__tabs']} aria-label="Разделы анализа">
                        <For
                          each={[
                            ['analysis', 'Анализ'],
                            ['answer', 'Ответ'],
                            ['archetype', 'Архетип'],
                            ['sources', 'Источники'],
                            ['traces', 'Intelligence traces'],
                          ]}
                        >
                          {(item) => (
                            <button
                              aria-pressed={detail() === item[0]}
                              onClick={() => setDetail(item[0] ?? 'analysis')}
                            >
                              {item[1]}
                            </button>
                          )}
                        </For>
                      </nav>
                      <Show when={detail() === 'traces'} fallback={<PagedText text={text()} />}>
                        <TraceReader
                          stages={reasoningStages()}
                          running={false}
                          person={selected()}
                        />
                      </Show>
                    </div>
                    <footer class={styles['experiment__footer']}>
                      <span title={savedPath()}>
                        {savedPath() ? '✓ Сохранено локально' : 'Результат готов'}
                      </span>
                      <button onClick={download}>↓ JSON</button>
                    </footer>
                  </>
                }
              >
                <header class={styles['experiment__progress-heading']}>
                  <span>LIVE EXPERIMENT / {progress()} OF 7</span>
                  <h2>
                    Intelligence traces<span class={styles['experiment__blink']}>_</span>
                  </h2>
                  <p>{activity()}</p>
                </header>
                <div class={styles['experiment__traces']}>
                  <TraceReader stages={reasoningStages()} running={running()} person={selected()} />
                </div>
                <nav class={styles['experiment__trace-pages']}>
                  <span>{events().at(-1)?.message}</span>
                  <button
                    disabled={!running()}
                    onClick={() => {
                      controller?.abort();
                      setActivity('Останавливаем CLI…');
                    }}
                  >
                    Остановить
                  </button>
                </nav>
                <footer class={styles['experiment__progress']}>
                  <span>PROGRESS {Math.round((progress() / 7) * 100)}%</span>
                  <progress max={7} value={progress()} />
                </footer>
              </Show>
            </section>
          </div>
        }
      >
        <form
          class={styles['experiment__start']}
          onSubmit={(event) => {
            event.preventDefault();
            void start();
          }}
        >
          <header class={styles['experiment__hero']}>
            <div class={styles['experiment__topline']}>
              <div class={styles['experiment__preset-actions']}>
                <button type="button" onClick={() => void openPreset()}>
                  Импорт .proteus
                </button>
                <button type="button" onClick={() => void savePreset()}>
                  Экспорт .proteus
                </button>
              </div>
              <button
                type="button"
                aria-label="Настройки PROTEUS"
                onClick={() => setSettingsOpen(true)}
              >
                ⚙ CONFIG
              </button>
            </div>
            <img src="/proteus-logo.svg" alt="Трёхглавый символ PROTEUS" />
            <div>
              <h1>PROTEUS</h1>
              <p>ONE TASK. MANY MINDS.</p>
            </div>
          </header>
          <div class={styles['experiment__duel']}>
            <For each={['A', 'B']}>
              {(letter, index) => (
                <section class={styles['experiment__input-card']} data-tone={letter.toLowerCase()}>
                  <header>
                    <span>
                      PLAYER {index() + 1 < 10 ? '0' : ''}
                      {index() + 1}
                    </span>
                    <strong>PERSON {letter}</strong>
                    <span>◆</span>
                  </header>
                  <div class={styles['experiment__identity']}>
                    <label>
                      Имя
                      <input
                        aria-label={`Имя персоны ${letter}`}
                        maxLength={80}
                        value={names()[index()]}
                        onInput={(event) =>
                          setNames(
                            names().map((name, i) =>
                              i === index() ? event.currentTarget.value : name,
                            ),
                          )
                        }
                      />
                    </label>
                    <PublicLinks
                      letter={letter}
                      revision={revision()}
                      urls={urls()[index()] ?? ['']}
                      onChange={(links) =>
                        setUrls(urls().map((value, i) => (i === index() ? links : value)))
                      }
                    />
                  </div>
                  <PagedField
                    label={`Контекст ${letter}`}
                    revision={revision()}
                    pages={contexts()[index()] ?? ['']}
                    onChange={(pages) =>
                      setContexts(contexts().map((value, i) => (i === index() ? pages : value)))
                    }
                  />
                </section>
              )}
            </For>
            <span class={styles['experiment__versus']}>VS</span>
          </div>
          <div class={styles['experiment__task']}>
            <span>
              MISSION
              <br />
              <small>Одна задача для обоих</small>
            </span>
            <PagedField
              compact
              label="Общая задача"
              pages={task()}
              onChange={setTask}
              revision={revision()}
            />
          </div>
          <footer class={styles['experiment__launch']}>
            <span>
              {available().length
                ? available()
                    .map((cli) => `${cli.provider.toUpperCase()} ${cli.available ? '●' : '○'}`)
                    .join(' / ')
                : 'DESKTOP CLI CONNECTION'}
            </span>
            <button type="submit" aria-label="Начать эксперимент">
              START ▶
            </button>
            <small>PUBLIC DATA ONLY</small>
          </footer>
        </form>
      </Show>
      <input
        ref={(element) => {
          fileInput = element;
        }}
        type="file"
        hidden
        accept=".proteus"
        aria-label="Файл пресета"
        onChange={async (event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = '';
          if (!file) return;
          if (!file.name.toLowerCase().endsWith('.proteus') || file.size > 131072) {
            setError('Выберите файл .proteus до 128 КиБ.');
            return;
          }
          try {
            receivePreset(await file.text());
          } catch (failure) {
            setError(String(failure));
          }
        }}
      />
      <Show when={pendingPreset()}>
        <div class={styles['experiment__pending']} role="status">
          <span>
            {running() ? 'Пресет ожидает завершения эксперимента.' : 'Пресет готов к открытию.'}
          </span>
          <button
            disabled={running()}
            onClick={() => {
              const input = pendingPreset();
              if (input) applyPreset(input);
              setPendingPreset(undefined);
            }}
          >
            Открыть пресет
          </button>
          <button aria-label="Отложить пресет" onClick={() => setPendingPreset(undefined)}>
            ×
          </button>
        </div>
      </Show>
      <Show when={error()}>
        <div class={styles['experiment__error']} role="alert">
          <p>{error()}</p>
          <button aria-label="Закрыть сообщение" onClick={() => setError('')}>
            ×
          </button>
        </div>
      </Show>
      <Show when={settingsOpen()}>
        <div
          class={styles['experiment__modal']}
          onKeyDown={(event) => {
            if (event.key === 'Escape') setSettingsOpen(false);
          }}
        >
          <ModelSettingsPanel
            settings={settings()}
            onChange={persist}
            onClose={() => setSettingsOpen(false)}
          />
        </div>
      </Show>
    </main>
  );
}
