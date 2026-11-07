import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';
import styles from './failure-reason.module.scss';
export function FailureReason(props: {
  readonly reason: string;
  readonly label: string;
  readonly retained: string;
}) {
  return (
    <section aria-label={props.label}>
      <div class={styles.failureReason} role="alert">
        <div class={styles.failureReason__reader}>
          <TextWorkspace label="Причина" value={props.reason} compact />
        </div>
        <p>{props.retained}</p>
      </div>
    </section>
  );
}
