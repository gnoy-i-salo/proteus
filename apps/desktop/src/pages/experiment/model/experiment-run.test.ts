import type * as SolidClient from 'solid-js';
import { createRoot } from 'solid-js';
import { expect, it, vi } from 'vitest';
import type {
  ExperimentInput,
  ExperimentProtocol,
  SourceGrant,
  StageLifecycle,
} from '@proteus-int/contracts';
import { createModelSettings } from '@proteus-int/intelligence';
import type { StageTrace } from '@proteus-int/pipeline';
import { createExperimentRun } from './experiment-run';
import type { ExperimentRunPort } from './experiment-run';
import { mergeLiveReasoning } from './live-trace';
vi.mock('solid-js', () => vi.importActual<typeof SolidClient>('solid-js/dist/solid.js'));
const input: ExperimentInput = {
  task: 'Task',
  people: [
    { label: 'A', context: ' A exact\r\n', publicUrls: [] },
    { label: 'B', context: ' B exact\r\n', publicUrls: [] },
  ],
};
const protocol: ExperimentProtocol = {
  version: 3,
  task: 'Task',
  repetitions: 1,
  participantOrder: [['a', 'b', 'neutral']],
  rubric: { version: 2, criteria: [] },
  modelSettings: {},
  formatAttempts: 2,
  stageTimeoutMs: 600000,
  promptVersion: 'bsc-v2.4-camel-case',
  createdAt: '2026-11-07T00:00:00Z',
};
const grant: SourceGrant = {
  provider: 'telegram',
  resourceId: 'source-id',
  authorId: null,
  from: null,
  to: null,
  allowModelProcessing: true,
  operations: ['read'],
  budget: { maxSteps: 1, maxDocuments: 1, maxBytes: 100, maxDurationMs: 100 },
  id: 'grant-id',
  accountId: 'account-id',
  snapshotId: 'snapshot-id',
  expiresAt: 'old-date',
};
const stage: StageTrace = {
  id: 'stage-id',
  stage: 'person',
  person: 0,
  label: 'Person A',
  attempts: [{ index: 1, status: 'format-rejected', wallTimeMs: 10, error: 'format' }],
  trace: { provider: 'codex', reasoningMs: 10, terminalMs: 0, summary: '', reasoning: [] },
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}
const tick = async () => {
  await Promise.resolve();
  await Promise.resolve();
};
function createPort() {
  const unlisten = vi.fn();
  const neverRun = vi.fn().mockRejectedValue(new Error('No model call expected'));
  return {
    cliStatus: vi.fn<ExperimentRunPort['cliStatus']>().mockResolvedValue([]),
    isDesktop: () => true,
    listenConnectionErrors: vi
      .fn<ExperimentRunPort['listenConnectionErrors']>()
      .mockResolvedValue(unlisten),
    validateExperiment: vi.fn<ExperimentRunPort['validateExperiment']>(),
    resumeSourceGrant: vi.fn<ExperimentRunPort['resumeSourceGrant']>().mockResolvedValue(undefined),
    runExperiment: vi
      .fn<ExperimentRunPort['runExperiment']>()
      .mockRejectedValue(new Error('synthetic end')),
    cliProviders: vi.fn<ExperimentRunPort['cliProviders']>().mockReturnValue({
      codex: { id: 'codex', run: neverRun },
      kimi: { id: 'kimi', run: neverRun },
    }),
    nativeSourcePort: {
      execute: vi.fn().mockRejectedValue(new Error('No source call expected')),
      reserveModelCall: vi.fn().mockRejectedValue(new Error('No reservation expected')),
    },
    saveExperiment: vi.fn<ExperimentRunPort['saveExperiment']>().mockResolvedValue('/saved/path'),
    downloadFile: vi.fn<ExperimentRunPort['downloadFile']>(),
    unlisten,
  };
}
function setup(configure?: (port: ReturnType<typeof createPort>) => void) {
  const port = createPort();
  configure?.(port);
  const reportError = vi.fn();
  const onStarted = vi.fn();
  const onCompleted = vi.fn();
  const settings = createModelSettings();
  let grants: readonly [readonly SourceGrant[], readonly SourceGrant[]] = [[], []];
  let available = true;
  let dispose!: () => void;
  const model = createRoot((stop) => {
    dispose = stop;
    return createExperimentRun(
      {
        currentInput: () => input,
        settings: () => settings,
        repetitions: () => 1,
        sourceGrants: () => grants,
        sourceModelsAvailable: () => available,
        reportError,
        onStarted,
        onCompleted,
      },
      port,
    );
  });
  return {
    model,
    port,
    reportError,
    onStarted,
    onCompleted,
    settings,
    dispose,
    setGrants: (value: typeof grants) => {
      grants = value;
    },
    setAvailable: (value: boolean) => {
      available = value;
    },
  };
}

it('passes the current input/settings to pipeline and requires explicit protocol approval', async () => {
  const approved = vi.fn();
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation(async (_input, _settings, _providers, options) => {
      await options?.onProtocol?.(protocol);
      approved();
      throw new Error('synthetic end');
    }),
  );
  const running = fixture.model.start();
  await tick();
  expect(fixture.model.screen()).toBe('progress');
  expect(fixture.model.running()).toBe(true);
  expect(fixture.model.pendingProtocol()).toBe(protocol);
  expect(approved).not.toHaveBeenCalled();
  expect(fixture.port.runExperiment).toHaveBeenCalledWith(
    input,
    fixture.settings,
    expect.any(Object),
    expect.objectContaining({ repetitions: 1, sourceGrants: [[], []] }),
  );
  fixture.model.approveProtocol();
  await running;
  expect(approved).toHaveBeenCalledOnce();
  expect(fixture.onStarted).toHaveBeenCalledOnce();
  expect(fixture.model.pendingProtocol()).toBeUndefined();
  expect(fixture.model.running()).toBe(false);
  fixture.dispose();
});

it('cancels protocol approval without continuing the pipeline', async () => {
  const continued = vi.fn();
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation(async (_input, _settings, _providers, options) => {
      await options?.onProtocol?.(protocol);
      continued();
      throw new Error('synthetic end');
    }),
  );
  const running = fixture.model.start();
  await tick();
  fixture.model.stop();
  await running;
  expect(continued).not.toHaveBeenCalled();
  expect(fixture.model.pendingProtocol()).toBeUndefined();
  expect(fixture.model.activity()).toBe('Эксперимент остановлен');
  expect(fixture.reportError).toHaveBeenLastCalledWith(
    expect.stringContaining('Можно вернуться к вводу'),
  );
  fixture.dispose();
});

it('checks source model support and input validity before starting native work', async () => {
  const fixture = setup();
  fixture.setGrants([[grant], []]);
  fixture.setAvailable(false);
  await fixture.model.start();
  expect(fixture.port.validateExperiment).not.toHaveBeenCalled();
  expect(fixture.port.runExperiment).not.toHaveBeenCalled();
  expect(fixture.reportError).toHaveBeenLastCalledWith(expect.stringContaining('выберите Codex'));
  fixture.setAvailable(true);
  fixture.port.validateExperiment.mockImplementationOnce(() => {
    throw new Error('invalid input');
  });
  await fixture.model.start();
  expect(fixture.port.runExperiment).not.toHaveBeenCalled();
  expect(fixture.model.running()).toBe(false);
  fixture.dispose();
});

it('aborts owner work while grants resume and stops before calling pipeline', async () => {
  const pending = deferred<void>();
  const fixture = setup((port) => port.resumeSourceGrant.mockReturnValue(pending.promise));
  fixture.setGrants([[grant], []]);
  const running = fixture.model.start();
  await tick();
  expect(fixture.port.resumeSourceGrant).toHaveBeenCalledWith('grant-id');
  fixture.dispose();
  pending.resolve();
  await running;
  expect(fixture.port.runExperiment).not.toHaveBeenCalled();
  expect(fixture.model.running()).toBe(false);
  expect(fixture.port.unlisten).toHaveBeenCalledOnce();
});

it('unsubscribes a connection listener that registers after owner disposal', async () => {
  const pending = deferred<() => void>();
  const fixture = setup((port) => port.listenConnectionErrors.mockReturnValue(pending.promise));
  fixture.dispose();
  pending.resolve(fixture.port.unlisten);
  await tick();
  expect(fixture.port.unlisten).toHaveBeenCalledOnce();
});

it('preserves progress, stage upserts and the current live reasoning merge', async () => {
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation((_input, _settings, _providers, options) => {
      options?.onProgress?.({
        id: stage.id,
        stage: 'person',
        person: 0,
        completed: 2,
        total: 13,
        message: 'Person A',
      });
      options?.onTrace?.(stage);
      options?.onExecution?.({
        ...liveStage,
        calls: [
          { ...liveStage.calls[0]!, callId: 'first-call', status: 'failed', wallTimeMs: 10 },
          ...liveStage.calls,
        ],
      });
      port.cliProviders.mock.calls[0]?.[0]({
        id: 'run-id',
        kind: 'reasoning',
        elapsedMs: 5,
        reasoning: { id: 'r', kind: 'summary', text: ' exact\r\n👩🏽‍🔬 ' },
      });
      return Promise.reject(new Error('synthetic end'));
    }),
  );
  await fixture.model.start();
  expect(fixture.model.progress()).toBe(2);
  expect(fixture.model.total()).toBe(13);
  expect(fixture.model.reasoningStages()).toHaveLength(1);
  expect(fixture.model.reasoningStages()[0]?.trace).toMatchObject({
    reasoningMs: 15,
    wallTimeMs: 15,
    reasoning: [{ id: '2:r', kind: 'summary', text: ' exact\r\n👩🏽‍🔬 ' }],
  });
  fixture.dispose();
});

it('replaces a repeated public stream block without rewriting previous text or stage metadata', () => {
  const first = mergeLiveReasoning(stage, { id: 'r', kind: 'summary', text: ' first\r\n' }, 5);
  const second = mergeLiveReasoning(first, { id: 'r', kind: 'summary', text: ' updated\r\n👩🏽‍🔬' }, 8);
  expect(first.trace.reasoning?.[0]?.text).toBe(' first\r\n');
  expect(second.trace.reasoning).toEqual([{ id: '2:r', kind: 'summary', text: ' updated\r\n👩🏽‍🔬' }]);
  expect(second.trace.wallTimeMs).toBe(18);
  expect(second.attempts).toBe(stage.attempts);
});

it('blocks a known missing selected CLI before starting any pipeline work', async () => {
  const fixture = setup((port) =>
    port.cliStatus.mockResolvedValue([
      { provider: 'codex', available: false, path: null },
      { provider: 'kimi', available: true, path: '/synthetic/kimi' },
    ]),
  );
  await tick();
  await fixture.model.start();
  expect(fixture.port.runExperiment).not.toHaveBeenCalled();
  expect(fixture.onStarted).not.toHaveBeenCalled();
  expect(fixture.reportError).toHaveBeenLastCalledWith(
    expect.stringContaining('Codex CLI не найден'),
  );
  expect(fixture.model.screen()).toBe('start');
  fixture.dispose();
});

it('returns from pending approval to editable input without continuing or presenting cancellation as failure', async () => {
  const continued = vi.fn();
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation(async (_input, _settings, _providers, options) => {
      await options?.onProtocol?.(protocol);
      continued();
      throw new Error('synthetic end');
    }),
  );
  const running = fixture.model.start();
  await tick();
  expect(fixture.model.pendingProtocol()).toBe(protocol);
  fixture.model.returnToInput();
  await tick();
  expect(fixture.model.screen()).toBe('start');
  expect(fixture.model.pendingProtocol()).toBeUndefined();
  await running;
  expect(continued).not.toHaveBeenCalled();
  expect(fixture.model.running()).toBe(false);
  expect(fixture.reportError).toHaveBeenLastCalledWith('');
  fixture.dispose();
});

it('ignores uncorrelated CLI reasoning instead of attaching it to the latest stage', async () => {
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation((_input, _settings, _providers, options) => {
      options?.onProgress?.({
        id: stage.id,
        stage: 'person',
        person: 0,
        completed: 2,
        total: 13,
        message: 'Person A',
      });
      options?.onTrace?.(stage);
      port.cliProviders.mock.calls[0]?.[0]({
        id: 'another-call',
        kind: 'reasoning',
        elapsedMs: 500,
        reasoning: { id: 'late', kind: 'thinking', text: 'Wrong stage' },
      });
      return Promise.reject(new Error('synthetic end'));
    }),
  );
  await fixture.model.start();
  expect(fixture.model.reasoningStages()[0]?.trace.reasoning).toEqual([]);
  fixture.dispose();
});

it('retains the CLI cancellation acknowledgement failure after user stop', async () => {
  const pending = deferred<void>();
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation(async () => {
      await pending.promise;
      throw new Error('Cancelled. Не удалось подтвердить остановку CLI.');
    }),
  );
  const running = fixture.model.start();
  fixture.model.stop();
  pending.resolve();
  await running;
  expect(fixture.reportError).toHaveBeenLastCalledWith(
    expect.stringContaining('Не удалось подтвердить остановку CLI.'),
  );
  fixture.dispose();
});

const liveStage: StageLifecycle = {
  schemaVersion: 1,
  runId: 'execution',
  stageId: 'unique-stage',
  logicalStageId: stage.id,
  component: 'person',
  person: 0,
  label: stage.label,
  status: 'running',
  startedAt: '2026-11-07T00:00:00Z',
  finishedAt: null,
  error: null,
  calls: [
    {
      callId: 'run-id',
      operation: 'generation',
      trigger: 'initial',
      formatAttempt: 1,
      status: 'running',
      startedAt: '2026-11-07T00:00:00Z',
      returnedAt: null,
      finishedAt: null,
      wallTimeMs: null,
      error: null,
      validationError: null,
    },
  ],
};

it('advances the host clock during silence, records real correlated events separately and freezes after failure', async () => {
  vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'performance'] });
  const pending = deferred<void>();
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation(async (_input, _settings, _providers, options) => {
      options?.onExecution?.(liveStage);
      await pending.promise;
      options?.onExecution?.({
        ...liveStage,
        status: 'failed',
        error: 'source refused',
        finishedAt: '2026-11-07T00:00:02Z',
      });
      options?.onExecution?.({
        ...liveStage,
        stageId: 'skipped-id',
        logicalStageId: 'next',
        status: 'skipped',
        calls: [],
      });
      throw new Error('source refused');
    }),
  );
  try {
    const running = fixture.model.start();
    expect(fixture.model).toHaveProperty('runStatus', expect.any(Function));
    expect(fixture.model.runStatus()).toBe('running');
    expect(fixture.model.activeStageId()).toBe(stage.id);
    vi.advanceTimersByTime(2000);
    expect(fixture.model.elapsedMs()).toBe(2000);
    expect(fixture.model.lastCliEventAgeMs()).toBeNull();
    fixture.port.cliProviders.mock.calls[0]?.[0]({ id: 'run-id', kind: 'started', elapsedMs: 25 });
    vi.advanceTimersByTime(1000);
    expect(fixture.model.elapsedMs()).toBe(3000);
    expect(fixture.model.lastCliEventAgeMs()).toBe(1000);
    pending.resolve();
    await running;
    expect(fixture.model.runStatus()).toBe('failed');
    expect(fixture.model.activeStageId()).toBeUndefined();
    expect(fixture.model.failedStageId()).toBe(stage.id);
    expect(fixture.model.execution()).toHaveLength(2);
    expect(fixture.model.failureReason()).toBe('source refused');
    vi.advanceTimersByTime(1000);
    expect(fixture.model.elapsedMs()).toBe(3000);
  } finally {
    fixture.dispose();
    pending.resolve();
    vi.useRealTimers();
  }
});

it('exposes preparing and stopping while a source grant resumes before any CLI call', async () => {
  const pending = deferred<void>();
  const fixture = setup((port) => port.resumeSourceGrant.mockReturnValue(pending.promise));
  fixture.setGrants([[grant], []]);
  const running = fixture.model.start();
  expect(fixture.model).toHaveProperty('runStatus', expect.any(Function));
  expect(fixture.model.runStatus()).toBe('preparing');
  expect(fixture.model.activeStageId()).toBeUndefined();
  fixture.model.stop();
  expect(fixture.model.runStatus()).toBe('stopping');
  pending.resolve();
  await running;
  expect(fixture.model.runStatus()).toBe('cancelled');
  expect(fixture.port.runExperiment).not.toHaveBeenCalled();
  fixture.dispose();
});

it('keeps one owner for an active run and ignores a second start', async () => {
  const pending = deferred<void>();
  const fixture = setup((port) =>
    port.runExperiment.mockImplementation(async () => {
      await pending.promise;
      throw new Error('synthetic end');
    }),
  );
  const first = fixture.model.start();
  const second = fixture.model.start();
  expect(fixture.port.runExperiment).toHaveBeenCalledOnce();
  pending.resolve();
  await Promise.all([first, second]);
  fixture.dispose();
});

it('cleans running state if the owner start callback fails', async () => {
  const fixture = setup();
  fixture.onStarted.mockImplementation(() => {
    throw new Error('owner failed');
  });
  await expect(fixture.model.start()).resolves.toBeUndefined();
  expect(fixture.model.running()).toBe(false);
  expect(fixture.model.runStatus()).toBe('failed');
  expect(fixture.reportError).toHaveBeenLastCalledWith('Error: owner failed');
  fixture.dispose();
});
