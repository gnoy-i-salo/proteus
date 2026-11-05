import { createSignal } from 'solid-js';
import type { PaletteId } from '@proteus-int/contracts';
import { LaboratoryPage } from '../pages/laboratory';
import { TitleBar } from '../widgets/title-bar';
import '../shared/styles/global.scss';
import styles from './app.module.scss';
export function App() {
  const [palette, setPalette] = createSignal<PaletteId>('turquoise');
  return (
    <div class={styles['app']} data-palette={palette()}>
      <TitleBar />
      <LaboratoryPage palette={palette()} onPaletteChange={setPalette} />
    </div>
  );
}
