import { ExperimentPage } from '../pages/experiment';
import { TitleBar } from '../widgets/title-bar';
import '../shared/styles/global.scss';
import styles from './app.module.scss';
export function App() {
  return (
    <div class={styles['app']}>
      <TitleBar />
      <ExperimentPage />
    </div>
  );
}
