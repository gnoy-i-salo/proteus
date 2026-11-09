import { structured } from '@proteus-int/intelligence';
import type { PersonPort } from './person-port';
import { parseReply } from './parse-reply';
import { personInstructions, personTask } from './person-prompt';

export const personRuntime: PersonPort = {
  async solve(profile, task, provider, signal) {
    if (profile.memory) throw new Error('Для профиля с памятью требуется нативный цикл Person.');
    const { data, trace, invocationId } = await structured(
      provider,
      'PERSON',
      personInstructions(profile),
      personTask(profile, task),
      signal,
      'high',
      parseReply,
      'persona-reply',
      profile.sourceContext === true,
    );
    return {
      ...(invocationId ? { invocationId } : {}),
      answer: data.messages.join('\n\n'),
      reply: { version: 2, messages: data.messages, styleConflicts: data.conflicts },
      trace: { ...trace, summary: data.summary },
    };
  },
};
