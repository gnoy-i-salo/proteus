// Preserve the public-text cap before strict wire validation. Only the known
// reasoning text is bounded here; no other content is relabelled or normalized.
export function boundCliReasoningText(raw: object): { value: object; truncated: boolean } {
  if (
    'reasoning' in raw &&
    raw.reasoning &&
    typeof raw.reasoning === 'object' &&
    'text' in raw.reasoning &&
    typeof raw.reasoning.text === 'string'
  ) {
    const bytes = new TextEncoder().encode(raw.reasoning.text);
    let end = Math.min(bytes.length, 48 * 1024);
    while (end > 0 && end < bytes.length && (bytes[end]! & 0xc0) === 0x80) end--;
    if (end < bytes.length)
      return {
        truncated: true,
        value: {
          ...raw,
          reasoning: { ...raw.reasoning, text: new TextDecoder().decode(bytes.subarray(0, end)) },
        },
      };
  }
  return { value: raw, truncated: false };
}
