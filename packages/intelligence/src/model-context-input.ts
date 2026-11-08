import type { IntelligenceRequest } from '@proteus-int/contracts';

const maxInputBytes = 4 * 1024 * 1024;

// Correlation only: native dispatch recomputes this identity and revalidates the
// single-use lease and source grants. A hash never grants access on its own.
export async function modelContextInput(
  request: Pick<IntelligenceRequest, 'systemPrompt' | 'task'>,
): Promise<{ inputSha256: string; inputBytes: number }> {
  const { systemPrompt, task } = request;
  if (systemPrompt.length + task.length > maxInputBytes)
    throw new Error('Model input exceeds the 4 MiB limit');
  for (const text of [systemPrompt, task]) {
    for (const character of text) {
      const point = character.codePointAt(0);
      if (point !== undefined && point >= 0xd800 && point <= 0xdfff)
        throw new Error('Model input must contain valid Unicode');
    }
  }
  const bytes = new TextEncoder().encode(JSON.stringify([systemPrompt, task]));
  if (bytes.length > maxInputBytes) throw new Error('Model input exceeds the 4 MiB limit');
  const digest = await crypto.subtle.digest('SHA-256', bytes);
  return {
    inputSha256: [...new Uint8Array(digest)]
      .map((value) => value.toString(16).padStart(2, '0'))
      .join(''),
    inputBytes: bytes.length,
  };
}
