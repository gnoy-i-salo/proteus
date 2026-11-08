# VK HTML message archive fixtures

Synthetic fixtures for the bounded VK HTML message archive page parser
(`vk-archive-html.rs`). All names, texts, dates, links and identifiers are
invented; no real account, dialog or private content is used.

## Format evidence (shape only)

The supported shapes were agreed from public format evidence consulted by the
parent agent: [a public 2019 VK HTML export sample by xTCry](https://gist.github.com/xTCry/700fecd61a43d0fd105b25d9469d0c8a) and
[a public VK HTML converter by jag-k](https://gist.github.com/jag-k/01b99ea0c5c4e07bc2b864a3ceffdaae). Only the structural shape was taken, not
source or private content:

- A message is a `div` whose class list contains the token `message`.
- A direct child `div` with class token `message__header` holds the author
  `a[href]` followed by a comma and the raw Russian date/time presentation.
- The direct following sibling `div` holds the message body: text, `br` and
  nested `div.attachment`, `div.kludges` or forwarded `div.message` blocks.
- Later exports add a `data-id` attribute on `div.message`; the older sample
  has no message ids.
- `message-edited` marks edited messages; `attachment`,
  `attachment__description`, `attachment__link` and `kludges` carry metadata.

These are limited supported variants, not a promise about every historical
VK archive shape. Unknown or malformed structure is rejected explicitly.

Raw HTML is stored as `.txt` fixture data so formatters cannot rewrite source
bytes, especially CRLF. These are input archives, not editable application HTML.

## Files

- `messages-basic.txt` — legacy no-`data-id` shape. CRLF line endings,
  two synthetic authors, emoji and combining marks, HTML entities, an edited
  marker, an attachment link/description, a kludges block, an empty body and
  two byte-identical repeated messages (must stay distinct by ordinal).
  The `head` contains `style`/`script` with decoy markup that must not be
  read as message text.
- `messages-data-id.txt` — newer shape with `data-id`, including ids above
  the JavaScript safe integer (`9007199254740993`), an attachment without a
  link, and one nested forwarded message that must be preserved as separate
  context and never attributed to the enclosing author.

Adversarial inputs (oversize input, event/depth/message caps, duplicate ids,
missing author link or date, truncated messages, unsupported elements,
unknown page shapes, late cancellation) are generated inline by the test
module so this directory stays limited to stable positive fixtures.

## Projection and rejection policy

The original HTML and its SHA256 remain authoritative, including original CRLF,
entities and whitespace. Body projections preserve decoded text including trailing
spaces and line breaks; HTML tokenization normalizes CRLF to LF, `br` adds LF,
and metadata/forward block boundaries add LF. Header text preserves token order.
Dates have no inferred timezone. Metadata labels are trimmed presentation fields.
Legacy messages without IDs require a caller-owned page + ordinal identity.
Duplicate attributes, null characters and tokenizer parse errors are rejected
rather than silently selecting an identity or repairing text. Missing forward
author URLs remain unknown; malformed supplied URLs are errors.

`replyTargetsNotExposed` is always present: absence of a forward marker does not
prove complete reply coverage. Detected forwarded content is separate from the
speaker body and remains unresolved for holdout. No archive acquisition, queue,
preview, UI or retrieval integration is provided by these parser modules alone.
