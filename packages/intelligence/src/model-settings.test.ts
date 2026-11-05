import { describe, expect, it } from 'vitest';
import {
  componentIds,
  createModelSettings,
  setComponentModel,
  setGlobalModel,
  parseModelSettings,
} from './index';
describe('model settings', () => {
  it('changes one component without affecting others or mutating the previous state', () => {
    const original = createModelSettings('codex');
    const changed = setComponentModel(original, 'stalker', 'kimi');
    expect(changed.components.stalker).toBe('kimi');
    expect(changed.components.person).toBe('codex');
    expect(original.components.stalker).toBe('codex');
  });
  it('global selection resets every per-component selection', () => {
    const settings = setComponentModel(createModelSettings('codex'), 'person', 'kimi');
    const changed = setGlobalModel(settings, 'codex');
    expect(componentIds.every((id) => changed.components[id] === 'codex')).toBe(true);
  });
  it('round-trips persisted settings and rejects corrupt or incomplete data', () => {
    const settings = setComponentModel(createModelSettings('kimi'), 'person-fabric', 'codex');
    expect(parseModelSettings(JSON.stringify(settings))).toEqual(settings);
    for (const value of ['{', '{}', 'null', '{"global-model":"other"}']) {
      expect(parseModelSettings(value)).toEqual(createModelSettings());
    }
  });
});
