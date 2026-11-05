import type {
  Evidence,
  IntelligenceEffort,
  PersonInput,
  IntelligenceTrace,
} from '@proteus-int/contracts';
import { object, text, structured } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface StalkerReport {
  readonly evidence: readonly Evidence[];
  readonly 'estimated-bsc': number;
  readonly effort: IntelligenceEffort;
  readonly character: string;
  readonly 'thinking-style': string;
  readonly activity: string;
  readonly 'behavior-summary': string;
}
export function validatePublicUrl(raw: string): string {
  const url = new URL(raw);
  const host = url.hostname.toLowerCase();
  if (
    !['https:', 'http:'].includes(url.protocol) ||
    url.username ||
    url.password ||
    host === 'localhost' ||
    host.endsWith('.local') ||
    host.endsWith('.localhost') ||
    !host.includes('.') ||
    host.includes(':') ||
    /^(0|10|127|169\.254|192\.168|172\.(1[6-9]|2\d|3[01]))\./u.test(host) ||
    /^\d+\.\d+\.\d+\.\d+$/u.test(host)
  )
    throw new Error('Нужна общедоступная http(s) ссылка без пароля и локального адреса.');
  return url.href;
}
export async function collectPerson(
  input: PersonInput,
  provider: IntelligenceProvider,
  signal?: AbortSignal,
): Promise<{ report: StalkerReport; trace: IntelligenceTrace }> {
  const urls = input['public-urls'].map(validatePublicUrl);
  const { data, trace } = await structured(
    provider,
    'STALKER',
    'Use WebSearch and public URLs when available: social networks, Wikipedia and public forums. Do not access private or authenticated material. Never invent fetched content. User context is an unverified hypothesis. Return {evidence:[{id, "source-url", kind:"fact"|"hypothesis", summary}], "estimated-bsc":positive number, effort:"low"|"medium"|"high", character, "thinking-style", activity, "behavior-summary"}. Facts require a public source actually consulted. Empty source-url is allowed only for hypotheses from user context. BSC is a speculative positive normalization estimate, never a measured IQ. Mark unavailable information as unknown. In summaries omit names, handles, URLs, emails and identifying details; describe behavior only.',
    { ...input, 'public-urls': urls },
    signal,
  );
  const estimate = data['estimated-bsc'];
  if (typeof estimate !== 'number' || !Number.isFinite(estimate) || estimate <= 0)
    throw new Error('STALKER: invalid estimated BSC');
  if (!['low', 'medium', 'high'].includes(String(data.effort)))
    throw new Error('STALKER: invalid effort');
  if (!Array.isArray(data.evidence) || data.evidence.length > 30)
    throw new Error('STALKER: invalid evidence');
  const evidence = data.evidence.map((value): Evidence => {
    const item = object(value);
    if (item.kind !== 'fact' && item.kind !== 'hypothesis')
      throw new Error('STALKER: invalid evidence kind');
    const source = typeof item['source-url'] === 'string' ? item['source-url'] : '';
    if (source) validatePublicUrl(source);
    if (item.kind === 'fact' && !source) throw new Error('STALKER: facts require a public source');
    return {
      id: text(item.id),
      'source-url': source,
      kind: item.kind,
      summary: text(item.summary),
    };
  });
  return {
    report: {
      evidence,
      'estimated-bsc': estimate,
      effort: data.effort as IntelligenceEffort,
      character: text(data.character),
      'thinking-style': text(data['thinking-style']),
      activity: text(data.activity),
      'behavior-summary': text(data['behavior-summary']),
    },
    trace,
  };
}
// Explicit identifiers and source locations are kept in the local evidence report, never in fabrication input.
export function anonymizeReport(report: StalkerReport, input: PersonInput) {
  const identifiers = [
    input.label,
    ...input['public-urls'].flatMap((raw) => {
      const url = new URL(raw);
      return [raw, ...url.pathname.split('/').filter((part) => part.length > 2)];
    }),
  ]
    .filter(Boolean)
    .sort((a, b) => b.length - a.length);
  const redact = (value: string) => {
    let clean = value.replace(
      /https?:\/\/\S+|[\w.+-]+@[\w.-]+\.[a-z]{2,}|@[\w.-]+/giu,
      '[redacted]',
    );
    for (const identifier of identifiers)
      clean = clean.replace(
        new RegExp(identifier.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&'), 'giu'),
        '[person]',
      );
    return clean;
  };
  return {
    character: redact(report.character),
    'thinking-style': redact(report['thinking-style']),
    activity: redact(report.activity),
    'behavior-summary': redact(report['behavior-summary']),
    evidence: report.evidence.map((item) => ({ kind: item.kind, summary: redact(item.summary) })),
  };
}
