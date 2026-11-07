import type { ModelSourceDocument, SourceBinding, SourceRole } from './corpus';
import type { SourceGrant, SourceGrantRequest, SourceResult } from './sources';

// Native creates and checks one binding around the existing source grant.
export interface SourceBindingRequest {
  readonly source: SourceGrantRequest;
  readonly personaId: string;
  readonly role: SourceRole;
  readonly allowedContextAuthorIds: readonly string[];
  readonly allowUnknownContext: boolean;
  readonly includeUndated: boolean;
}
export interface CreatedSourceBinding {
  readonly binding: SourceBinding;
  readonly grant: SourceGrant;
}
export type SourceBindingResult = Omit<SourceResult, 'documents'> & {
  readonly documents: readonly ModelSourceDocument[];
};
