import { createSignal, For, Show } from 'solid-js';
import type { IngestionPreview, SourceRole } from '@proteus-int/contracts';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import { formText } from '../../../shared/lib/form-fields';
import type { SourceImportSelection } from '../model/source-preparation';
import styles from './source-preparation.module.scss';
const roles: readonly { id: SourceRole; label: string }[] = [
  { id: 'own', label: 'Собственные слова персоны' },
  { id: 'context', label: 'Контекст общения' },
  { id: 'about', label: 'Материал о персоне' },
  { id: 'reference', label: 'Справочный материал' },
];
export function SourceImportForm(props: {
  readonly preview: IngestionPreview;
  readonly busy: boolean;
  readonly onError: (cause: unknown) => void;
  readonly onStart: (selection: SourceImportSelection) => Promise<void>;
}) {
  const [details, setDetails] = createSignal(false);
  const [step, setStep] = createSignal(0);
  const [resourceId, setResourceId] = createSignal('');
  const [authorId, setAuthorId] = createSignal('');
  const [role, setRole] = createSignal<SourceRole>('own');
  const resource = () => props.preview.resources.find((item) => item.id === resourceId());
  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        if (step() !== 2) return;
        const fields = new FormData(event.currentTarget);
        void props
          .onStart({
            resourceId: resourceId(),
            role: role(),
            targetAuthorId: authorId() || null,
            allowedContextAuthorIds: fields
              .getAll('contextAuthor')
              .filter((value): value is string => typeof value === 'string'),
            allowUnknownContext: fields.has('unknown'),
            period: {
              from: formText(fields, 'from') ? `${formText(fields, 'from')}T00:00:00.000Z` : null,
              to: formText(fields, 'to') ? `${formText(fields, 'to')}T23:59:59.999Z` : null,
              includeUndated: fields.has('undated'),
            },
            allowLocalImport: fields.has('local'),
            allowModelProcessing: fields.has('model'),
            allowExport: fields.has('export'),
          })
          .catch(props.onError);
      }}
    >
      <fieldset disabled={props.busy} hidden={details()}>
        <legend>
          {['Ресурс и автор', 'Контекст и период', 'Разрешения'][step()]} · {step() + 1} / 3
        </legend>
        <div class={styles.sourcePreparation__step} hidden={step() !== 0}>
          <label>
            Ресурс
            <select
              required
              value={resourceId()}
              onChange={(event) => {
                setResourceId(event.currentTarget.value);
                setAuthorId('');
              }}
            >
              <option value="">Выберите ресурс</option>
              <For each={props.preview.resources}>
                {(item) => <option value={item.id}>{item.label}</option>}
              </For>
            </select>
          </label>
          <Show when={resource()} keyed>
            {(selected) => (
              <>
                <p>Найдено документов: {selected.estimatedDocuments ?? 'неизвестно'}</p>
                <label>
                  Роль материала
                  <select
                    value={role()}
                    onChange={(event) => {
                      const selected = roles.find((item) => item.id === event.currentTarget.value);
                      if (selected) setRole(selected.id);
                    }}
                  >
                    <For each={roles}>
                      {(item) => <option value={item.id}>{item.label}</option>}
                    </For>
                  </select>
                </label>
                <label>
                  Автор персоны
                  <select
                    required={role() === 'own'}
                    value={authorId()}
                    onChange={(event) => setAuthorId(event.currentTarget.value)}
                  >
                    <option value="">Автор не указан</option>
                    <For each={selected.authorIds}>{(id) => <option value={id}>{id}</option>}</For>
                  </select>
                </label>
              </>
            )}
          </Show>
        </div>
        <div class={styles.sourcePreparation__step} hidden={step() !== 1}>
          <Show when={resource()} keyed>
            {(selected) => (
              <>
                <label>
                  Собеседники в контексте (необязательно)
                  <select multiple name="contextAuthor" size={3}>
                    <For each={selected.authorIds.filter((id) => id !== authorId())}>
                      {(id) => <option value={id}>{id}</option>}
                    </For>
                  </select>
                </label>
                <div class={styles.sourcePreparation__range}>
                  <label>
                    С даты
                    <input type="date" name="from" />
                  </label>
                  <label>
                    По дату
                    <input type="date" name="to" />
                  </label>
                </div>
                <label class={styles.sourcePreparation__check}>
                  <input type="checkbox" name="unknown" />
                  Включить материалы с неизвестным автором
                </label>
                <label class={styles.sourcePreparation__check}>
                  <input type="checkbox" name="undated" />
                  Включить материалы без даты
                </label>
              </>
            )}
          </Show>
        </div>
        <div class={styles.sourcePreparation__step} hidden={step() !== 2}>
          <button type="button" onClick={() => setDetails(true)}>
            Ограничения источника: {props.preview.limitations.length}
          </button>
          <label class={styles.sourcePreparation__check}>
            <input type="checkbox" name="local" required={step() === 2} />
            Разрешаю локальный импорт выбранного материала
          </label>
          <label class={styles.sourcePreparation__check}>
            <input type="checkbox" name="model" />
            Разрешаю обработку моделями
          </label>
          <label class={styles.sourcePreparation__check}>
            <input type="checkbox" name="export" />
            Разрешаю экспорт исходного материала
          </label>
          <p>
            Разрешение действует 24 часа. Подготовка корпуса выполняется локально; изучение моделью
            запускается отдельно.
          </p>
          <button type="submit" disabled={!resource()}>
            Импортировать выбранное
          </button>
        </div>
        <nav class={styles.sourcePreparation__actions} aria-label="Шаги подготовки источника">
          <button type="button" disabled={step() === 0} onClick={() => setStep(step() - 1)}>
            Назад к выбору
          </button>
          <Show when={step() < 2}>
            <button
              type="button"
              disabled={!resource() || (role() === 'own' && !authorId())}
              onClick={() => setStep(step() + 1)}
            >
              {step() === 0 ? 'Выбрать контекст и период' : 'Перейти к разрешениям'}
            </button>
          </Show>
        </nav>
      </fieldset>
      <Show when={details()}>
        <TextWorkspace
          label="Ограничения источника"
          value={props.preview.limitations.join('\n') || 'Ограничения предпросмотра не указаны.'}
        />
        <button type="button" onClick={() => setDetails(false)}>
          Вернуться к разрешениям
        </button>
      </Show>
    </form>
  );
}
