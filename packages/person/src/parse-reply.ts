import { decodeResponseFields, text, texts } from '@proteus-int/intelligence';

export function parseReply(data: unknown) {
  const response = decodeResponseFields(data, [['style-conflicts', 'styleConflicts']]);
  const messages: unknown = response.messages ?? [response.answer];
  if (
    !Array.isArray(messages) ||
    messages.length < 1 ||
    messages.length > 30 ||
    messages.some((message) => typeof message !== 'string' || !message.trim()) ||
    messages.join('').length > 200_000
  )
    throw new Error('messages: expected 1–30 nonempty text replies');
  return {
    messages: messages as string[],
    summary: text(response.summary, 'summary'),
    conflicts: texts(response.styleConflicts ?? [], 'styleConflicts'),
  };
}
