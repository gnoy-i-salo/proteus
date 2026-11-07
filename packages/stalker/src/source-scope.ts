import type { SourceGrant, SourceResult } from '@proteus-int/contracts';

export function validateSourceGrant(grant: SourceGrant) {
  if (
    !grant.id ||
    !grant.snapshotId ||
    !grant.accountId ||
    !grant.resourceId ||
    grant.allowModelProcessing !== true ||
    !(Date.parse(grant.expiresAt) > Date.now()) ||
    (grant.from !== null && !Number.isFinite(Date.parse(grant.from))) ||
    (grant.to !== null && !Number.isFinite(Date.parse(grant.to))) ||
    (grant.from !== null && grant.to !== null && Date.parse(grant.from) > Date.parse(grant.to)) ||
    !grant.operations.includes('collect') ||
    Object.values(grant.budget).some((value) => !Number.isSafeInteger(value) || value <= 0) ||
    grant.budget.maxSteps > 100 ||
    grant.budget.maxDocuments > 5000 ||
    grant.budget.maxBytes > 5 * 1024 * 1024 ||
    grant.budget.maxDurationMs > 600000
  )
    throw new Error('STALKER: invalid or expired source grant');
}
export function validateSourceResult(result: SourceResult, grant: SourceGrant) {
  if (
    result.snapshotId !== grant.snapshotId ||
    !Array.isArray(result.documents) ||
    result.documents.length > 20
  )
    throw new Error('STALKER: invalid source scope or result size');
  for (const doc of result.documents) {
    if (
      !doc.id ||
      doc.id.length > 128 ||
      doc.provider !== grant.provider ||
      doc.resourceId !== grant.resourceId ||
      typeof doc.text !== 'string' ||
      doc.text.length > 24000 ||
      !doc.contentHash ||
      (doc.createdAt !== null && !Number.isFinite(Date.parse(doc.createdAt))) ||
      typeof doc.truncated !== 'boolean' ||
      typeof doc.isForwarded !== 'boolean' ||
      (doc.authorLabel !== null && typeof doc.authorLabel !== 'string') ||
      !['reference', 'holdout'].includes(doc.partition) ||
      !['owner', 'other', 'unknown'].includes(doc.authorship) ||
      !['dialogue', 'comment', 'formal'].includes(doc.mode) ||
      (grant.authorId && doc.authorship === 'owner' && doc.authorId !== grant.authorId) ||
      (grant.from && (!doc.createdAt || Date.parse(doc.createdAt) < Date.parse(grant.from))) ||
      (grant.to && (!doc.createdAt || Date.parse(doc.createdAt) > Date.parse(grant.to)))
    )
      throw new Error('STALKER: document outside source scope or invalid document');
    if (doc.retrievedAt !== null && !Number.isFinite(Date.parse(doc.retrievedAt)))
      throw new Error('STALKER: invalid source retrieval receipt');
  }
  if (
    ![result.coverage.documents, result.coverage.bytes, result.coverage.notAnalyzed].every(
      (n) => Number.isSafeInteger(n) && n >= 0,
    ) ||
    typeof result.coverage.complete !== 'boolean' ||
    typeof result.coverage.truncated !== 'boolean' ||
    (result.coverage.totalDocuments !== null &&
      (!Number.isSafeInteger(result.coverage.totalDocuments) ||
        result.coverage.totalDocuments < 0)) ||
    (result.cursor !== null && result.coverage.complete) ||
    (result.cursor !== null &&
      (typeof result.cursor !== 'string' || result.cursor.length > 1024)) ||
    !Array.isArray(result.limitations) ||
    !Array.isArray(result.coverage.unavailable)
  )
    throw new Error('STALKER: invalid source coverage');
}
