import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';

export function PagedText(props: { text: string; resetKey?: string; onInteraction?: () => void }) {
  return (
    <TextWorkspace
      label="Текст"
      value={props.text}
      resetKey={props.resetKey ?? props.text}
      onInteraction={props.onInteraction}
    />
  );
}
