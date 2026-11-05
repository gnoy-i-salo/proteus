import type { ProviderId } from '@proteus-int/contracts';
export const componentIds = ['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const;
export type ComponentId = (typeof componentIds)[number];
export interface ModelSettings {
  readonly 'global-model': ProviderId;
  readonly components: Readonly<Record<ComponentId, ProviderId>>;
}
export function isProviderId(value: unknown): value is ProviderId {
  return value === 'codex' || value === 'kimi';
}
export function createModelSettings(model: ProviderId = 'codex'): ModelSettings {
  return {
    'global-model': model,
    components: {
      stalker: model,
      'person-fabric': model,
      person: model,
      'brain-analyzator': model,
    },
  };
}
export function setGlobalModel(_settings: ModelSettings, model: ProviderId): ModelSettings {
  return createModelSettings(model);
}
export function setComponentModel(
  settings: ModelSettings,
  component: ComponentId,
  model: ProviderId,
): ModelSettings {
  return { ...settings, components: { ...settings.components, [component]: model } };
}
export function parseModelSettings(raw: string | null): ModelSettings {
  try {
    const value: unknown = JSON.parse(raw ?? 'null');
    if (
      !value ||
      typeof value !== 'object' ||
      !('global-model' in value) ||
      !isProviderId(value['global-model']) ||
      !('components' in value)
    )
      return createModelSettings();
    const components = value.components;
    if (!components || typeof components !== 'object') return createModelSettings();
    let result = createModelSettings(value['global-model']);
    for (const component of componentIds) {
      const model: unknown = Reflect.get(components, component);
      if (!isProviderId(model)) return createModelSettings();
      result = setComponentModel(result, component, model);
    }
    return result;
  } catch {
    return createModelSettings();
  }
}
