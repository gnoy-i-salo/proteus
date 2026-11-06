import type {
  Evidence,
  EvidenceStatus,
  PersonInput,
  IntelligenceTrace,
  StyleExemplar,
  SourceCoverage,
} from '@proteus-int/contracts';
import { object, text, texts, structured } from '@proteus-int/intelligence';
import type { IntelligenceProvider } from '@proteus-int/intelligence';
export interface StalkerReport {
  readonly evidence: readonly Evidence[];
  readonly character: string;
  readonly 'thinking-style': string;
  readonly activity: string;
  readonly 'behavior-summary': string;
  readonly 'style-exemplars'?: readonly StyleExemplar[];
  readonly 'source-coverage'?: readonly {
    readonly 'snapshot-id': string;
    readonly coverage: SourceCoverage;
    readonly 'analyzed-documents': number;
    readonly limitations: readonly string[];
  }[];
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
function parseReport(data: Record<string, unknown>): StalkerReport {
  if (!Array.isArray(data.evidence) || data.evidence.length > 30)
    throw new Error('STALKER: invalid evidence');
  const ids = new Set<string>();
  const evidence = data.evidence.map((value): Evidence => {
    const item = object(value);
    if (item.kind !== 'fact' && item.kind !== 'hypothesis')
      throw new Error('STALKER: invalid evidence kind');
    const source = typeof item['source-url'] === 'string' ? item['source-url'] : '';
    if (source) validatePublicUrl(source);
    if (item.kind === 'fact' && !source) throw new Error('STALKER: facts require a public source');
    const id = text(item.id, 'evidence.id');
    if (!/^e\d+$/u.test(id)) throw new Error('STALKER: evidence.id must be opaque, e.g. e1');
    if (ids.has(id)) throw new Error('STALKER: duplicate evidence.id');
    ids.add(id);
    if (
      typeof item.status !== 'string' ||
      !['supported', 'hypothesis', 'unknown', 'conflicting'].includes(item.status)
    )
      throw new Error('STALKER: invalid evidence.status');
    const excerpt = item.excerpt === null ? null : text(item.excerpt, 'evidence.excerpt');
    if (item.status === 'supported' && (!source || !excerpt || item.kind !== 'fact'))
      throw new Error('STALKER: supported evidence requires a fact, source and excerpt');
    return {
      id,
      'source-url': source,
      kind: item.kind,
      summary: text(item.summary, 'evidence.summary'),
      status: item.status as EvidenceStatus,
      verification: source && excerpt ? 'model-reported' : 'unverified',
      excerpt,
      // The adapter does not attest retrieval; never trust a model-supplied timestamp.
      'retrieved-at': null,
      limitations: texts(item.limitations, 'evidence.limitations'),
    };
  });
  return {
    evidence,
    character: text(data.character, 'character'),
    'thinking-style': text(data['thinking-style'], 'thinking-style'),
    activity: text(data.activity, 'activity'),
    'behavior-summary': text(data['behavior-summary'], 'behavior-summary'),
  };
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
    'Use only permitted public material through available search tools. Do not access private or authenticated material. Never invent fetched content. User context is an unverified hypothesis, not a fact. Return {evidence:[{id:"e1", "source-url":string, kind:"fact"|"hypothesis", summary:string, status:"supported"|"hypothesis"|"unknown"|"conflicting", excerpt:string|null, limitations:[string]}], character:string, "thinking-style":string, activity:string, "behavior-summary":string}. IDs must be unique opaque e1, e2 etc. Supported means only source support reported by you; it is NOT independently verified. Supported items need a public source and an exact short supporting excerpt. Never claim retrieval verification. Empty source-url and null excerpt are allowed for hypotheses or unknown information. Keep contradictory evidence as conflicting. Evidence may be empty. Use unknown for unavailable summary fields; do not invent a personality, sensitive traits, IQ, BSC or effort. Summaries must describe behavior without names, handles, URLs, emails or identifying details. Keep the original source URL and short excerpt only in evidence fields for local inspection.',
    { ...input, 'public-urls': urls },
    signal,
    'medium',
    parseReport,
    'public-research',
  );
  return { report: data, trace };
}
// Explicit identifiers and source locations are kept in the local evidence report, never in fabrication input.
export function redactTask(value: string, inputs: readonly PersonInput[]): string {
  const personalUrls = inputs.flatMap((input) => input['public-urls'].map((raw) => new URL(raw)));
  const identifiers = inputs
    .flatMap((input) => [
      input.label,
      ...(input.context.match(/[\w.+-]+@[\w.-]+\.[a-z]{2,}|@[\w.-]+/giu) ?? []),
      ...input['public-urls'].flatMap((raw) => {
        const url = new URL(raw);
        const handle = url.pathname.split('/').filter(Boolean).at(-1);
        return [
          raw,
          ...[handle ?? '']
            .filter((part) => part.length > 2)
            .flatMap((part) => {
              let decoded = part;
              try {
                decoded = decodeURIComponent(part);
              } catch {
                // Keep the original segment if the source URL uses malformed escapes.
              }
              return [decoded, `@${decoded}`];
            }),
        ];
      }),
    ])
    .filter(Boolean)
    .sort((a, b) => b.length - a.length);
  let clean = value.replace(/https?:\/\/[^\s<>]+/giu, (raw) => {
    try {
      const url = new URL(raw.replace(/[.,;!?]+$/u, ''));
      return personalUrls.some(
        (personal) => personal.hostname === url.hostname && personal.pathname === url.pathname,
      )
        ? '[person-source]'
        : raw;
    } catch {
      return raw;
    }
  });
  for (const identifier of identifiers)
    clean = clean.replace(
      new RegExp(
        `(?<![\\p{L}\\p{N}_])${identifier.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&')}(?![\\p{L}\\p{N}_])`,
        'giu',
      ),
      '[person]',
    );
  return clean;
}
export function anonymizeReport(report: StalkerReport, input: PersonInput) {
  const redact = (value: string) =>
    redactTask(
      value.replace(/https?:\/\/\S+|[\w.+-]+@[\w.-]+\.[a-z]{2,}|@[\w.-]+/giu, '[redacted]'),
      [input],
    );
  return {
    character: redact(report.character),
    'thinking-style': redact(report['thinking-style']),
    activity: redact(report.activity),
    'behavior-summary': redact(report['behavior-summary']),
    ...(report['style-exemplars'] ? { 'style-exemplars': report['style-exemplars'] } : {}),
    evidence: report.evidence.map((item) => ({
      id: item.id,
      kind: item.kind,
      status: item.status,
      verification: item.verification,
      summary: redact(item.summary),
      limitations: item.limitations.map(redact),
    })),
  };
}
export { collectSource, parseCollectionAction, validateSourceGrant } from './source-controller';
export type { SourceCollection } from './source-controller';
export { sourceSkill } from './source-skills';
export { analyzeSourceCollections } from './source-analysis';
