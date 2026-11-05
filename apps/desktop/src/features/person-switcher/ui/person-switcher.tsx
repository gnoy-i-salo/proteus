import type { PaletteId } from '@proteus-int/contracts';
import styles from './person-switcher.module.scss';
export function PersonSwitcher(props: {
  palette: PaletteId;
  onChange: (palette: PaletteId) => void;
}) {
  return (
    <div class={styles['person-switcher']} role="group" aria-label="Выбор персоны">
      <button
        class={styles['person-switcher__button']}
        aria-pressed={props.palette === 'turquoise'}
        onClick={() => props.onChange('turquoise')}
      >
        01 / PERSON A <span>Бирюзовая палитра</span>
      </button>
      <button
        class={styles['person-switcher__button']}
        aria-pressed={props.palette === 'violet'}
        onClick={() => props.onChange('violet')}
      >
        02 / PERSON B <span>Фиолетовая палитра</span>
      </button>
    </div>
  );
}
