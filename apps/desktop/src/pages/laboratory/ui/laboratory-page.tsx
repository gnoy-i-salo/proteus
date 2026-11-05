import { For } from 'solid-js';
import type { PaletteId } from '@proteus-int/contracts';
import { ModelSettingsPanel } from '../../../features/model-settings';
import { PersonSwitcher } from '../../../features/person-switcher';
import styles from './laboratory-page.module.scss';
const pipeline = ['STALKER', 'PERSON FABRIC', 'PERSON', 'BRAIN SCORING', 'BRAIN ANALYZATOR'];
export function LaboratoryPage(props: {
  palette: PaletteId;
  onPaletteChange: (palette: PaletteId) => void;
}) {
  return (
    <main class={styles['laboratory']}>
      <section class={styles['laboratory__hero']}>
        <div>
          <p class={styles['laboratory__eyebrow']}>INTELLIGENCE, IN MANY FORMS</p>
          <h1>
            PROTEUS<span>PERSONA LABORATORY</span>
          </h1>
          <p class={styles['laboratory__intro']}>
            Разные характеры. Одна задача.
            <br />
            Пространство для исследования персон.
          </p>
          <span class={styles['laboratory__status']}>◆ STARTER / Система готова к развитию</span>
        </div>
        <img
          class={styles['laboratory__logo']}
          src="/proteus-logo.svg"
          width="240"
          height="200"
          alt="Трёхглавое существо: бирюзовая, серая и фиолетовая головы"
        />
      </section>
      <section class={styles['laboratory__person']} aria-labelledby="person-title">
        <p class={styles['laboratory__eyebrow']}>01 / ACTIVE PERSONA</p>
        <h2 id="person-title">Два взгляда на одну задачу</h2>
        <PersonSwitcher palette={props.palette} onChange={props.onPaletteChange} />
      </section>
      <ModelSettingsPanel />
      <section class={styles['laboratory__pipeline']} aria-label="Пайплайн PROTEUS">
        <p class={styles['laboratory__eyebrow']}>03 / ARCHITECTURE</p>
        <ol>
          <For each={pipeline}>
            {(step, index) => (
              <li>
                <span>0{index() + 1}</span>
                {step}
              </li>
            )}
          </For>
        </ol>
        <p>Контракты модулей подготовлены. Обработка данных и запуск моделей пока не подключены.</p>
      </section>
      <footer class={styles['laboratory__footer']}>
        PROTEUS / v0.1.0 <span>ONE TASK. MANY MINDS.</span>
      </footer>
    </main>
  );
}
