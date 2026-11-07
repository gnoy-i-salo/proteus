import type { ProviderId } from '@proteus-int/contracts';
export const componentIds = ['stalker', 'person-fabric', 'person', 'brain-analyzator'] as const;
export type ComponentId = (typeof componentIds)[number];
export interface ModelSettings {
  readonly globalModel: ProviderId;
  readonly components: Readonly<Record<ComponentId, ProviderId>>;
  readonly aliases: Readonly<Record<ComponentId, string>>;
}
export function isProviderId(value: unknown): value is ProviderId {
  return value === 'codex' || value === 'kimi';
}
export function createModelSettings(model: ProviderId = 'codex'): ModelSettings {
  return {
    globalModel: model,
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
export function setModelAlias(
  settings: ModelSettings,
  component: ComponentId,
  alias: string,
): ModelSettings {
  return { ...settings, aliases: { ...settings.aliases, [component]: alias.trim() } };
}
