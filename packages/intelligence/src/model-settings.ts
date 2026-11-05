import type { ProviderId } from '@proteus-int/contracts';
export const componentIds = ['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const;
export type ComponentId = (typeof componentIds)[number];
export interface ModelSettings {
  readonly 'global-model': ProviderId;
  readonly components: Readonly<Record<ComponentId, ProviderId>>;
  readonly aliases: Readonly<Record<ComponentId, string>>;
}
export function isProviderId(value: unknown): value is ProviderId {
  return value === 'codex' || value === 'kimi';
}
export function createModelSettings(model: ProviderId = 'codex'): ModelSettings {
  return {
    'global-model': model,
    aliases: { stalker: '', 'person-fabric': '', person: '', 'brain-analyzator': '' },
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
    if ('aliases' in value && value.aliases && typeof value.aliases === 'object') {
      for (const component of componentIds) {
        const alias: unknown = Reflect.get(value.aliases, component);
        if (typeof alias === 'string' && alias.length <= 120)
          result = setModelAlias(result, component, alias);
      }
    }
    return result;
  } catch {
    return createModelSettings();
  }
}

export function setModelAlias(
  settings: ModelSettings,
  component: ComponentId,
  alias: string,
): ModelSettings {
  return { ...settings, aliases: { ...settings.aliases, [component]: alias.trim() } };
}
