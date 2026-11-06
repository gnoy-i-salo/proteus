import type { ExperimentInput } from '@proteus-int/contracts';
import { validatePublicUrl } from '@proteus-int/stalker';
export function validateExperiment(input: ExperimentInput): void {
  if (!input.task.trim() || input.task.length > 4000)
    throw new Error('Опишите общую задачу (до 4000 символов).');
  if (input.people.length !== 2) throw new Error('Нужны две персоны.');
  for (const person of input.people) {
    if (!person.label.trim() || person.label.length > 80)
      throw new Error('Укажите имя персоны (до 80 символов).');
    if (!person.context.trim() && !person['public-urls'].length)
      throw new Error('Добавьте публичную ссылку или контекст каждой персоны.');
    if (person.context.length > 4000 || person['public-urls'].length > 5)
      throw new Error('Максимум 5 ссылок и 4000 символов контекста на персону.');
    person['public-urls'].forEach(validatePublicUrl);
  }
}
