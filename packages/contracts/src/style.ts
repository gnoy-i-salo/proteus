export type CommunicationMode = 'dialogue' | 'comment' | 'formal';

/** Short verbatim author samples with separate export permission. */
export interface StyleExemplar {
  readonly id: string;
  readonly documentId: string;
  readonly mode: CommunicationMode;
  readonly text: string;
  readonly context: string;
  /** Legacy replacement spans for older saved profiles; new samples always use an empty array. */
  readonly redactions: readonly { readonly start: number; readonly end: number }[];
  readonly exportAllowed: boolean;
}

export interface StyleRule {
  readonly id: string;
  readonly instruction: string;
  readonly exemplarIds: readonly string[];
  readonly status: 'observed' | 'hypothesis';
  readonly limitations: readonly string[];
}

export interface PersonaStyleProfile {
  readonly version: 2;
  readonly reasoningPatterns: readonly StyleRule[];
  readonly voiceRules: readonly StyleRule[];
  readonly formatRules: readonly StyleRule[];
  readonly contextModes: readonly {
    readonly mode: CommunicationMode;
    readonly status: 'observed' | 'insufficient';
    readonly limitations: readonly string[];
  }[];
  readonly styleExemplarIds: readonly string[];
  readonly limitations: readonly string[];
}

/** Messages are lossless strings; answer is their ordered join with two LF characters. */
export interface PersonReply {
  readonly version: 2;
  readonly messages: readonly string[];
  readonly styleConflicts: readonly string[];
}
