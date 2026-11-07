import { createSignal } from 'solid-js';
import { setGlobalModel, setComponentModel } from '@proteus-int/intelligence';
import type { ModelSettings, ComponentId } from '@proteus-int/intelligence';
import type { ProviderId } from '@proteus-int/contracts';

export function createModelSettingsEditor(options: {
  readonly settings: () => ModelSettings;
  readonly hasSources: () => boolean;
  readonly onChange: (settings: ModelSettings) => void;
}) {
  const [globalProvider, selectGlobalProvider] = createSignal(options.settings().globalModel);
  const [pending, setPending] = createSignal<ModelSettings>();
  const settings = () => pending() ?? options.settings();
  const change = (next: ModelSettings) => {
    const current = options.settings();
    const invalidates =
      next.components.stalker !== current.components.stalker ||
      next.aliases.stalker !== current.aliases.stalker;
    if (options.hasSources() && invalidates) setPending(next);
    else {
      setPending(undefined);
      options.onChange(next);
    }
  };
  return {
    settings,
    globalProvider,
    selectGlobalProvider,
    pending,
    applyGlobal: () => change(setGlobalModel(settings(), globalProvider())),
    changeProvider: (component: ComponentId, provider: ProviderId) =>
      change(setComponentModel(settings(), component, provider)),
    changeAlias: (component: ComponentId, alias: string) =>
      change({ ...settings(), aliases: { ...settings().aliases, [component]: alias } }),
    confirm: () => {
      const next = pending();
      if (!next) return;
      options.onChange(next);
      setPending(undefined);
    },
    discard: () => setPending(undefined),
  };
}
