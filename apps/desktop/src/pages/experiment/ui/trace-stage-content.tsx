import { createMemo, For, Show } from 'solid-js';
import type { ExperimentResult, StageTrace } from '@proteus-int/pipeline';
import {
  projectStageArtifact,
  stageArtifactRequests,
  stageArtifactStageId,
} from '../model/stage-artifacts';
import {
  publishedStageText,
  stageCallsText,
  stageResultText,
} from '../model/stage-artifact-content';
import type { TraceContentView, TraceNavigation } from '../model/trace-navigation';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import styles from './trace-reader.module.scss';

const views: readonly (readonly [TraceContentView, string])[] = [
  ['reasoning', 'Reasoning'],
  ['published', 'Опубликованный текст'],
  ['artifact', 'Результат этапа'],
  ['calls', 'Вызовы и ошибки'],
  ['summary', 'Сводка этапа'],
];

export function TraceStageContent(props: {
  readonly item: StageTrace;
  readonly navigation: TraceNavigation;
  readonly running: boolean;
  readonly result: ExperimentResult | undefined;
}) {
  const artifact = createMemo(() => {
    const result = props.result;
    if (!result) return undefined;
    const request = stageArtifactRequests(result).find(
      (value) => stageArtifactStageId(value) === props.item.id,
    );
    return request ? projectStageArtifact(result, request) : undefined;
  });
  const view = () => props.navigation.contentView();
  const label = () =>
    view() === 'reasoning' ? 'Текст' : views.find(([key]) => key === view())![1];
  const text = () => {
    switch (view()) {
      case 'summary':
        return props.item.trace.summary || 'Сводка этапа ещё не получена.';
      case 'published':
        return publishedStageText(artifact());
      case 'artifact':
        return stageResultText(artifact());
      case 'calls':
        return stageCallsText(artifact(), props.item);
      case 'reasoning':
        return (
          props.item.trace.reasoning?.map((block) => block.text).join('\n\n') ||
          (props.running && !props.item.trace.summary
            ? 'Ожидаем текст reasoning от CLI…'
            : 'CLI не передал текст reasoning для этого этапа.')
        );
    }
  };
  return (
    <>
      <nav class={styles.traceReader__contentViews} aria-label="Содержимое этапа">
        <select
          class={styles.traceReader__contentSelect}
          aria-label="Представление этапа"
          value={view()}
          onChange={(event) => {
            const selected = views.find(([key]) => key === event.currentTarget.value);
            if (selected) props.navigation.setContentView(selected[0]);
          }}
        >
          <For each={views}>{([key, name]) => <option value={key}>{name}</option>}</For>
        </select>
        <For each={views}>
          {([key, name]) => (
            <button
              type="button"
              aria-pressed={view() === key}
              onClick={() => props.navigation.setContentView(key)}
            >
              {name}
            </button>
          )}
        </For>
      </nav>
      <Show when={view() === 'reasoning'}>
        <Show when={props.item.trace.reasoningTruncated}>
          <p class={styles.traceReader__badge}>Текст reasoning сокращён до лимита сохранения.</p>
        </Show>
      </Show>
      <div class={styles.traceReader__reader}>
        <TextWorkspace
          label={label()}
          value={text()}
          resetKey={`${props.item.id}:${view()}`}
          position={() => props.navigation.positionFor(props.item.id, view())}
          onPositionChange={(position) =>
            props.navigation.savePosition(props.item.id, position, view())
          }
          onInteraction={() => props.navigation.pause()}
        />
      </div>
    </>
  );
}
