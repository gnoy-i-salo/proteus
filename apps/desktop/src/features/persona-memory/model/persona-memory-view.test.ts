import type * as SolidClient from 'solid-js';
import { createRoot, createSignal } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type {
  PersonProfile,
  PersonaTaskAdaptation,
  SkillDefinition,
  StyleExemplar,
} from '@proteus-int/contracts';
import {
  createPersonaMemoryViewModel,
  describeEvidenceStatus,
  describeSkillSupport,
  skillEvidenceTargets,
} from './persona-memory-view';
import type { PersonaMemoryEvidenceTarget } from './persona-memory-view';

// Node otherwise selects Solid's SSR build, whose lifecycle effects are intentionally inert.
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));

const skill: SkillDefinition = {
  schemaVersion: 1,
  id: 'skill-1',
  trigger: 'когда просят о плане',
  inputs: ['исходные заметки'],
  steps: ['собрать тезисы', 'проверить даты'],
  outcomeCheck: 'план согласован',
  support: 'observed',
  memoryItemIds: ['memory-item-1', 'memory-item-2'],
  sourceSpans: [
    {
      documentId: 'doc-1',
      revisionId: 'rev-1',
      unit: 'utf8Bytes',
      start: 12,
      end: 48,
    },
  ],
  limitations: ['выборка мала'],
};

const exemplar: StyleExemplar = {
  id: 'exemplar-1',
  documentId: 'doc-1',
  mode: 'dialogue',
  text: 'Точный текст 👩🏽‍🔬\r\nс переносом строки',
  context: 'личная переписка, 2024',
  redactions: [],
  exportAllowed: false,
};

function profile(overrides: Partial<PersonProfile> = {}): PersonProfile {
  return {
    id: 'persona-1',
    label: 'Персона А',
    palette: 'turquoise',
    archetype: 'Базовый архетип',
    skills: ['внимание к деталям'],
    rules: [
      {
        id: 'rule-1',
        instruction: 'Правило с опорой',
        evidenceIds: ['evidence-1'],
        status: 'supported',
        limitations: [],
      },
      {
        id: 'rule-2',
        instruction: 'Осторожное правило',
        evidenceIds: [],
        status: 'hypothesis',
        limitations: ['нужна проверка'],
      },
    ],
    taskSkills: [
      {
        id: 'task-rule-1',
        instruction: 'Навык адаптации',
        evidenceIds: ['evidence-1'],
        status: 'supported',
        limitations: [],
      },
    ],
    sourceStatus: 'evidence-informed',
    memory: {
      schemaVersion: 1,
      manifestId: 'manifest-1',
      memorySnapshotId: 'snapshot-1',
      skills: [skill],
    },
    styleExemplars: [exemplar],
    ...overrides,
  };
}

function setup(current: PersonProfile, onEvidence?: (target: PersonaMemoryEvidenceTarget) => void) {
  let dispose!: () => void;
  const value = createRoot((stop) => {
    dispose = stop;
    const [currentProfile, setProfile] = createSignal(current);
    const model = createPersonaMemoryViewModel({
      profile: currentProfile,
      onEvidence,
    });
    return { model, setProfile };
  });
  return { ...value, dispose };
}

it('derives base rules and task adaptation as separate, visible sections', () => {
  const fixture = setup(profile());
  expect(fixture.model.baseRules().map((rule) => rule.id)).toEqual(['rule-1', 'rule-2']);
  expect(fixture.model.adaptationRules().map((rule) => rule.id)).toEqual(['task-rule-1']);
  expect(fixture.model.baseRules()).not.toBe(fixture.model.adaptationRules());
  expect(fixture.model.archetype()).toBe('Базовый архетип');
  expect(fixture.model.profileSkills()).toEqual(['внимание к деталям']);
  fixture.dispose();
});

it('maps evidence and skill support statuses to honest Russian labels without scoring', () => {
  expect(describeEvidenceStatus('supported')).toBe('Поддержано источником');
  expect(describeEvidenceStatus('hypothesis')).toBe('Гипотеза');
  expect(describeEvidenceStatus('unknown')).toBe('Неизвестно');
  expect(describeEvidenceStatus('conflicting')).toBe('Противоречие');
  expect(describeSkillSupport('observed')).toBe('Наблюдаемый');
  expect(describeSkillSupport('proposed')).toBe('Предлагаемый');
});

it('exposes the frozen memory manifest and snapshot versions when supplied', () => {
  const fixture = setup(profile());
  expect(fixture.model.memorySummary()).toEqual({
    manifestId: 'manifest-1',
    memorySnapshotId: 'snapshot-1',
    schemaVersion: 1,
  });
  expect(fixture.model.proceduralSkills()).toEqual([skill]);
  fixture.dispose();
});

it('describes missing memory, style and exemplars honestly instead of fabricating them', () => {
  const full = profile({ sourceStatus: 'synthetic' });
  // Dropping absent optional extensions models an existing non-RAG profile.
  const stripped = { ...full };
  delete stripped.memory;
  delete stripped.styleProfile;
  delete stripped.styleExemplars;
  const fixture = setup(stripped);
  expect(fixture.model.memorySummary()).toBeNull();
  expect(fixture.model.proceduralSkills()).toEqual([]);
  expect(fixture.model.styleProfile()).toBeNull();
  expect(fixture.model.styleExemplars()).toEqual([]);
  expect(fixture.model.hasMemoryData()).toBe(false);
  expect(fixture.model.sourceStatusLabel()).toBe('Синтетический профиль');
  fixture.dispose();
});

it('keeps exact source text, unicode, CRLF and whitespace of style exemplars', () => {
  const exact = 'Строка 👩🏽‍🔬\r\n  с отступом\tи символом \\u0024';
  const supplied = {
    ...exemplar,
    id: 'exemplar-2',
    text: exact,
    context: 'контекст «досье»',
    documentId: 'doc-2',
  };
  const fixture = setup(profile({ styleExemplars: [supplied] }));
  const [rendered] = fixture.model.styleExemplars();
  expect(rendered?.text).toBe(exact);
  expect(rendered?.context).toBe('контекст «досье»');
  expect(fixture.model.exemplarAttribution(rendered!)).toBe('doc-2');
  fixture.dispose();
});

it('omits attribution when the source document is not actually supplied', () => {
  const anonymous = { ...exemplar, id: 'exemplar-3', documentId: '' };
  const fixture = setup(profile({ styleExemplars: [anonymous] }));
  const [rendered] = fixture.model.styleExemplars();
  expect(fixture.model.exemplarAttribution(rendered!)).toBeNull();
  fixture.dispose();
});

it('builds typed evidence targets only from actual memory item and source document references', () => {
  const targets = skillEvidenceTargets(skill);
  expect(targets).toEqual([
    { kind: 'memoryItem', memoryItemId: 'memory-item-1', skillId: 'skill-1' },
    { kind: 'memoryItem', memoryItemId: 'memory-item-2', skillId: 'skill-1' },
    {
      kind: 'sourceDocument',
      documentId: 'doc-1',
      revisionId: 'rev-1',
      skillId: 'skill-1',
    },
  ]);
  const exemplarTargets = skillEvidenceTargets({
    ...skill,
    memoryItemIds: [],
    sourceSpans: [],
  });
  expect(exemplarTargets).toEqual([]);
});

it('hands the exact typed target to the evidence callback', () => {
  const onEvidence = vi.fn();
  const fixture = setup(profile(), onEvidence);
  expect(fixture.model.evidenceNavigation()).toBe('available');
  fixture.model.openEvidence({
    kind: 'memoryItem',
    memoryItemId: 'memory-item-1',
    skillId: 'skill-1',
  });
  expect(onEvidence).toHaveBeenCalledWith({
    kind: 'memoryItem',
    memoryItemId: 'memory-item-1',
    skillId: 'skill-1',
  });
  fixture.dispose();
});

it('reports unavailable navigation instead of pretending to load evidence without a callback', () => {
  const fixture = setup(profile());
  expect(fixture.model.evidenceNavigation()).toBe('unavailable');
  expect(() =>
    fixture.model.openEvidence({
      kind: 'styleExemplar',
      exemplarId: 'exemplar-1',
      documentId: 'doc-1',
    }),
  ).not.toThrow();
  fixture.dispose();
});

it('reflects profile changes without caching props or reading a global store', () => {
  const onEvidence = vi.fn();
  const fixture = setup(profile(), onEvidence);
  expect(fixture.model.memorySummary()?.manifestId).toBe('manifest-1');
  expect(fixture.model.proceduralSkills()).toHaveLength(1);
  const updated = profile({
    memory: {
      schemaVersion: 1,
      manifestId: 'manifest-2',
      memorySnapshotId: 'snapshot-2',
      skills: [],
    },
    rules: [],
    taskSkills: [],
  });
  fixture.setProfile(updated);
  expect(fixture.model.memorySummary()).toEqual({
    manifestId: 'manifest-2',
    memorySnapshotId: 'snapshot-2',
    schemaVersion: 1,
  });
  expect(fixture.model.proceduralSkills()).toEqual([]);
  expect(fixture.model.baseRules()).toEqual([]);
  expect(fixture.model.adaptationRules()).toEqual([]);
  fixture.model.openEvidence({ kind: 'memoryItem', memoryItemId: 'x', skillId: 'y' });
  expect(onEvidence).toHaveBeenCalledOnce();
  fixture.dispose();
});

it('labels neutral control profiles separately from evidence-informed ones', () => {
  const fixture = setup(profile({ sourceStatus: 'neutral' }));
  expect(fixture.model.sourceStatusLabel()).toBe('Нейтральный контроль');
  expect(fixture.model.isNeutralControl()).toBe(true);
  fixture.setProfile(profile({ sourceStatus: 'evidence-informed' }));
  expect(fixture.model.sourceStatusLabel()).toBe('На основе источников');
  expect(fixture.model.isNeutralControl()).toBe(false);
  fixture.dispose();
});

it('reads separate task guidance without writing it into the frozen profile', () => {
  const base = profile({ taskSkills: [] });
  const before = JSON.stringify(base);
  const adaptation: PersonaTaskAdaptation = {
    schemaVersion: 1,
    manifestId: 'manifest-1',
    task: '  Задача 👩🏽‍🔬\r\n',
    strategies: [
      {
        id: 's1',
        instruction: '  Проверить даты\r\n',
        status: 'hypothesis',
        evidenceIds: ['evidence-1'],
        limitations: [],
      },
    ],
  };
  const options = { profile: () => base, adaptation: () => adaptation };
  const model = createPersonaMemoryViewModel(options);
  expect(model.adaptationRules()).toEqual(adaptation.strategies);
  expect(JSON.stringify(base)).toBe(before);
});

it('does not attach stale guidance or fall back to legacy task skills for another manifest', () => {
  const base = profile();
  const adaptation: PersonaTaskAdaptation = {
    schemaVersion: 1,
    manifestId: 'other-manifest',
    task: 'Задача',
    strategies: [
      {
        id: 's1',
        instruction: 'Проверить даты',
        status: 'hypothesis',
        evidenceIds: [],
        limitations: [],
      },
    ],
  };
  const options = { profile: () => base, adaptation: () => adaptation };
  const model = createPersonaMemoryViewModel(options);
  expect(model.adaptationRules()).toEqual([]);
});

it('reacts to manifest and guidance changes and rejects invalid source claims', () => {
  createRoot((dispose) => {
    const [current, setCurrent] = createSignal(profile({ taskSkills: [] }));
    const valid: PersonaTaskAdaptation = {
      schemaVersion: 1,
      manifestId: 'manifest-1',
      task: 'Задача',
      strategies: [
        {
          id: 's1',
          instruction: 'Стратегия',
          status: 'hypothesis',
          evidenceIds: ['evidence-1'],
          limitations: [],
        },
      ],
    };
    const [adaptation, setAdaptation] = createSignal<PersonaTaskAdaptation | undefined>(valid);
    const model = createPersonaMemoryViewModel({ profile: current, adaptation });
    expect(model.adaptationTask()).toBe('Задача');
    expect(model.adaptationError()).toBeUndefined();
    setCurrent(
      profile({
        memory: {
          schemaVersion: 1,
          manifestId: 'manifest-2',
          memorySnapshotId: 'snapshot-2',
          skills: [],
        },
      }),
    );
    expect(model.adaptationRules()).toEqual([]);
    expect(model.adaptationError()).toContain('не соответствует');
    setCurrent(profile());
    expect(model.adaptationRules()).toEqual(valid.strategies);
    setAdaptation({ ...valid, strategies: [{ ...valid.strategies[0]!, status: 'supported' }] });
    expect(model.adaptationRules()).toEqual([]);
    setAdaptation(undefined);
    expect(model.adaptationError()).toBeUndefined();
    expect(model.adaptationRules()).toEqual(current().taskSkills);
    dispose();
  });
});

it('uses the current evidence handler when navigation becomes available or is revoked', () => {
  createRoot((dispose) => {
    const callback = vi.fn();
    const [enabled, setEnabled] = createSignal(false);
    const model = createPersonaMemoryViewModel({
      profile: () => profile(),
      get onEvidence() {
        return enabled() ? callback : undefined;
      },
    });
    const target: PersonaMemoryEvidenceTarget = {
      kind: 'memoryItem',
      memoryItemId: 'memory-item-1',
      skillId: 'skill-1',
    };
    model.openEvidence(target);
    expect(callback).not.toHaveBeenCalled();
    setEnabled(true);
    expect(model.evidenceNavigation()).toBe('available');
    model.openEvidence(target);
    expect(callback).toHaveBeenCalledExactlyOnceWith(target);
    setEnabled(false);
    expect(model.evidenceNavigation()).toBe('unavailable');
    model.openEvidence(target);
    expect(callback).toHaveBeenCalledOnce();
    dispose();
  });
});

it('shows saved baseline skills separately from adapted legacy skills', () => {
  const base = profile({
    archetype: 'Базовый характер',
    skills: ['Базовый навык'],
    taskSkills: [],
  });
  const adapted = profile({
    archetype: 'Изменённое описание',
    skills: ['Стратегия текущей задачи'],
  });
  const options = { profile: () => adapted, baseProfile: () => base };
  const model = createPersonaMemoryViewModel(options);
  expect(model.archetype()).toBe('Базовый характер');
  expect(model.profileSkills()).toEqual(['Базовый навык']);
  expect(model.adaptationRules()).toEqual(adapted.taskSkills);
});
