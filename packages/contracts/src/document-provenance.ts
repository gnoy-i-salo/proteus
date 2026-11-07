import type { DocumentRevision, ProvenanceLimitation } from './corpus';

const knownLimitations: readonly ProvenanceLimitation[] = [
  'originalObjectIdUnavailable',
  'originalBytesUnavailable',
  'attachmentsUnavailable',
  'relationsIncomplete',
];
const provenanceFields = [
  'acquisition',
  'adapterVersion',
  'sourceFormat',
  'originalObjectId',
  'originalByteLength',
  'hashAlgorithm',
  'limitations',
];

// Structural consistency only: provenance claims still come from the native
// acquisition adapter. SourceDocument.id and source text are never substitutes
// for an unavailable provider object ID or its original bytes.
export function validateRevisionProvenance(revision: DocumentRevision): void {
  const value: unknown = revision.provenance;
  if (
    !value ||
    typeof value !== 'object' ||
    Array.isArray(value) ||
    Object.keys(value).length !== provenanceFields.length ||
    Object.keys(value).some((key) => !provenanceFields.includes(key))
  )
    throw new Error('Неверная схема происхождения документа.');
  const provenance = value as Record<string, unknown>;
  if (
    typeof provenance.acquisition !== 'string' ||
    !['api', 'archive', 'selectedUrl', 'savedPage'].includes(provenance.acquisition) ||
    typeof provenance.adapterVersion !== 'string' ||
    !provenance.adapterVersion.length ||
    typeof provenance.sourceFormat !== 'string' ||
    !provenance.sourceFormat.length ||
    provenance.hashAlgorithm !== 'sha256'
  )
    throw new Error('Неверные сведения о получении документа.');
  const limitations = validateLimitations(provenance.limitations);
  validateOriginalMetadata(provenance, limitations);
  const legacy =
    provenance.sourceFormat === 'proteusEncryptedSourceRecord' ||
    provenance.adapterVersion.startsWith('legacy-source-record-');
  if (
    legacy &&
    (provenance.sourceFormat !== 'proteusEncryptedSourceRecord' ||
      !/^legacy-source-record-v[0-3]$/u.test(provenance.adapterVersion) ||
      provenance.originalObjectId !== null ||
      provenance.originalByteLength !== null ||
      knownLimitations.some((limitation) => !limitations.has(limitation)))
  )
    throw new Error('Старый документ не подтверждает полноту исходных метаданных.');
}

function validateLimitations(value: unknown): ReadonlySet<ProvenanceLimitation> {
  if (!Array.isArray(value)) throw new Error('Укажите ограничения происхождения документа.');
  const decoded = value.map((entry: unknown) => {
    const limitation = knownLimitations.find((item) => item === entry);
    if (!limitation) throw new Error('Неизвестное ограничение происхождения документа.');
    return limitation;
  });
  if (new Set(decoded).size !== decoded.length)
    throw new Error('Ограничения происхождения не должны повторяться.');
  return new Set(decoded);
}

function validateOriginalMetadata(
  provenance: Record<string, unknown>,
  limitations: ReadonlySet<ProvenanceLimitation>,
): void {
  const id = provenance.originalObjectId;
  const bytes = provenance.originalByteLength;
  if (
    id === null
      ? !limitations.has('originalObjectIdUnavailable')
      : typeof id !== 'string' || !id.length || limitations.has('originalObjectIdUnavailable')
  )
    throw new Error('Неизвестный исходный ID должен быть отмечен явно.');
  if (
    bytes === null
      ? !limitations.has('originalBytesUnavailable')
      : typeof bytes !== 'number' ||
        !Number.isSafeInteger(bytes) ||
        bytes < 0 ||
        limitations.has('originalBytesUnavailable')
  )
    throw new Error('Неизвестный размер исходных данных должен быть отмечен явно.');
}
