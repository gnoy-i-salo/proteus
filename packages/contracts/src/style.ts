export type CommunicationMode = 'dialogue' | 'comment' | 'formal';

/** Short sanitized author samples, distinct from the original local corpus. */
export interface StyleExemplar {
  readonly id: string;
  readonly 'document-id': string;
  readonly mode: CommunicationMode;
  readonly text: string;
  readonly context: string;
  readonly redactions: readonly { readonly start: number; readonly end: number }[];
  readonly 'export-allowed': boolean;
}

export interface StyleRule {
  readonly id: string;
  readonly instruction: string;
  readonly 'exemplar-ids': readonly string[];
  readonly status: 'observed' | 'hypothesis';
  readonly limitations: readonly string[];
}

export interface PersonaStyleProfile {
  readonly version: 1;
  readonly 'reasoning-patterns': readonly StyleRule[];
  readonly 'voice-rules': readonly StyleRule[];
  readonly 'format-rules': readonly StyleRule[];
  readonly 'context-modes': readonly {
    readonly mode: CommunicationMode;
    readonly status: 'observed' | 'insufficient';
    readonly limitations: readonly string[];
  }[];
  readonly 'style-exemplar-ids': readonly string[];
  readonly limitations: readonly string[];
}

/** Messages are lossless strings; answer is their ordered join with two LF characters. */
export interface PersonReply {
  readonly version: 1;
  readonly messages: readonly string[];
  readonly 'style-conflicts': readonly string[];
}
