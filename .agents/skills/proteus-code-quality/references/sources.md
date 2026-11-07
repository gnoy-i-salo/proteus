# Source decisions

Reviewed on 2026-11-07. This skill and examples are written specifically for PROTEUS; third-party instruction files and code are not copied or installed. Links retain attribution and license information. If later copying implementation or substantial text, include the corresponding license/notices with that copy.

- [Apollo GraphQL rust-best-practices](https://github.com/apollographql/skills/blob/main/skills/rust-best-practices/SKILL.md), [MIT license](https://github.com/apollographql/skills/blob/main/LICENSE), copyright Apollo Graph, Inc. Used as a reference for ownership, errors and evidence-based lint selection. PROTEUS does not adopt blanket prohibitions on cloning/dynamic dispatch, one-assertion-per-test, or the whole lint profile.
- [Anthropic code-simplifier](https://github.com/anthropics/claude-plugins-official/blob/main/plugins/code-simplifier/agents/code-simplifier.md), [Apache-2.0 license](https://github.com/anthropics/claude-plugins-official/blob/main/plugins/code-simplifier/LICENSE). Used for the idea of scoped simplification preserving behavior. Its Claude-specific setup, model selection and React conventions are not installed or applied.
- [SolidJS effects](https://docs.solidjs.com/concepts/effects) supports separating derived values from effects. PROTEUS targets its installed SolidJS 1.9; check installed API/version before using newer examples.
- [typescript-eslint typed linting](https://typescript-eslint.io/getting-started/typed-linting/) is the reference for projectService and type-aware configuration. Enabling those checks is a separate repository migration with calibrated fixtures; this skill does not claim they are already enabled.

The user-approved repository naming, architecture and compatibility decisions take precedence over generic guidance in these sources.
