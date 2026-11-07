# Telegram Desktop JSON export fixtures

Synthetic fixtures for the bounded streaming reader (`telegram-archive-json.rs`).
All chat and message content is invented; no real account data.

## Verified exporter shape

Primary source (official Telegram Desktop exporter):

- File: `Telegram/SourceFiles/export/output/export_output_json.cpp`
- Repository: https://github.com/telegramdesktop/tdesktop
- Revision verified: commit `fa588bb82443fb23d1b9f9fe9c6baa5bc9710f91` (2026-10-29), branch `dev`
- Raw URL: https://raw.githubusercontent.com/telegramdesktop/tdesktop/fa588bb82443fb23d1b9f9fe9c6baa5bc9710f91/Telegram/SourceFiles/export/output/export_output_json.cpp

Structure confirmed from that source (not copied implementation):

1. Single-peer export (`Settings::onlySinglePeer()`): root object with keys
   `name` (omitted for `saved_messages`/`replies`/`verification_codes`, otherwise
   string or `null`), `type` (string or `null`), `id` (raw JSON number),
   `messages` (array). Written by `JsonWriter::writeDialogStart` reused for the
   single-peer root.
2. Account export: root object always starts with `about`, followed by optional
   sections `personal_information`, `profile_pictures`, `stories`,
   `profile_music`, `contacts`, `frequent_contacts`, `other_data`, `sessions`,
   `web_sessions`, and dialog sections `chats` / `left_chats` (key chosen by
   `JsonWriter::validateDialogsMode`). Each dialog section is an object
   `{ "about": string, "list": [chat...] }` (`JsonWriter::writeChatsStart`).
3. Chat object: optional `name` (string or `null`), then `type` (string or
   `null`), then `id` (raw JSON number), then `messages` array. Header keys
   always precede `messages` in official output; the reader rejects any other
   ordering instead of inventing chat identity.
4. Message objects always carry `id` (raw JSON number) and `type`
   (`"message"`, `"service"` or `"unsupported"`); the reader treats message
   bodies as opaque JSON and only requires well-formed objects with exact raw
   byte spans.

## Files

- `single-chat.json` — single-peer export, `personal_chat` type, two messages.
- `account-export.json` — account export with `about`, `personal_information`,
  a large `contacts` section, `sessions`, and two chats in `chats.list`
  (one of them with an empty `messages` array).
- `account-export-left-chats.json` — account export with both `chats` and
  `left_chats` sections, one chat each.

Malformed and adversarial inputs (truncated JSON, duplicate keys, wrong
ordering, unknown roots, invalid UTF-8, oversized values) are generated inline
by the test module to keep this directory limited to stable positive fixtures.
