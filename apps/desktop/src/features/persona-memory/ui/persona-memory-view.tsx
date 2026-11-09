import { createSignal, For, Show } from 'solid-js';
import type { PersonProfile, PersonaTaskAdaptation } from '@proteus-int/contracts';
import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import { createPersonaMemoryViewModel } from '../model/persona-memory-view';
import type { PersonaMemoryEvidenceTarget } from '../model/persona-memory-view';
import {
  createPersonaMemoryPages,
  evidenceTargetLabel,
  profileSections,
} from '../model/persona-memory-pages';
import type { ProfileSection } from '../model/persona-memory-pages';
import styles from './persona-memory-view.module.scss';

export function PersonaMemoryView(props: {
  readonly profile: PersonProfile;
  readonly baseProfile?: PersonProfile | undefined;
  readonly adaptation?: PersonaTaskAdaptation | undefined;
  readonly onEvidence?: ((target: PersonaMemoryEvidenceTarget) => void) | undefined;
}) {
  const model = createPersonaMemoryViewModel({
    profile: () => props.profile,
    baseProfile: () => props.baseProfile,
    adaptation: () => props.adaptation,
    get onEvidence() {
      return props.onEvidence;
    },
  });
  const pages = createPersonaMemoryPages(model);
  const [section, setSection] = createSignal<ProfileSection>('profile');
  const scope = () => `${props.profile.id}:${props.profile.memory?.manifestId ?? ''}:${section()}`;
  const [selection, setSelection] = createSignal({ scope: '', id: '' });
  const items = () => pages.items(section());
  const item = () =>
    (selection().scope === scope() && items().find((entry) => entry.id === selection().id)) ||
    items()[0]!;
  const [evidenceIndex, setEvidenceIndex] = createSignal(0);
  const evidence = () => item().evidence[evidenceIndex()] ?? item().evidence[0];
  return (
    <section
      class={styles.personaMemoryView}
      data-palette={model.palette()}
      aria-label="Профиль и память"
    >
      <div class={styles.personaMemoryView__selectors}>
        <label>
          Раздел профиля
          <select
            aria-label="Раздел профиля"
            value={section()}
            onChange={(event) => {
              const selected = profileSections.find(([key]) => key === event.currentTarget.value);
              if (selected) {
                setSection(selected[0]);
                setEvidenceIndex(0);
              }
            }}
          >
            <For each={profileSections}>
              {([value, label]) => <option value={value}>{label}</option>}
            </For>
          </select>
        </label>
        <label>
          Запись профиля
          <select
            aria-label="Запись профиля"
            value={item().id}
            onChange={(event) => {
              setSelection({ scope: scope(), id: event.currentTarget.value });
              setEvidenceIndex(0);
            }}
          >
            <For each={items()}>{(entry) => <option value={entry.id}>{entry.label}</option>}</For>
          </select>
        </label>
      </div>
      <TextWorkspace
        label="Текст профиля"
        value={item().text}
        resetKey={`${scope()}:${item().id}`}
      />
      <Show when={evidence()}>
        <div class={styles.personaMemoryView__evidence}>
          <Show
            when={model.evidenceNavigation() === 'available'}
            fallback={<span>Открытие исходников на этом экране недоступно.</span>}
          >
            <select
              aria-label="Основание профиля"
              value={evidenceIndex()}
              onChange={(event) => setEvidenceIndex(Number(event.currentTarget.value))}
            >
              <For each={item().evidence}>
                {(target, index) => <option value={index()}>{evidenceTargetLabel(target)}</option>}
              </For>
            </select>
            <button
              type="button"
              onClick={() => {
                const target = evidence();
                if (target) model.openEvidence(target);
              }}
            >
              Открыть основание
            </button>
          </Show>
        </div>
      </Show>
    </section>
  );
}
