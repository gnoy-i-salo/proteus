import { TextWorkspace } from '../../../shared/ui/text-workspace/text-workspace';

export function PagedField(props: {
  label: string;
  pages: string[];
  onChange: (pages: string[]) => void;
  compact?: boolean;
  revision: number;
}) {
  return (
    <TextWorkspace
      label={props.label}
      value={props.pages.join('')}
      onChange={(value) => props.onChange([value])}
      compact={props.compact}
      resetKey={props.revision}
    />
  );
}
