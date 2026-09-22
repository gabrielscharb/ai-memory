# Wiki link extraction

`ai_memory_wiki::markdown::extract_links` builds the graph edges (and
dangling-link warnings) for a page's body. It recognizes two distinct link
grammars, and deliberately extracts each with a different strategy.

## `[[wikilinks]]` — a dedicated scan

`[[page]]`, `[[page|label]]`, `[[project:path]]`, and
`[[workspace/project:path]]` are not CommonMark syntax, so pulldown-cmark
has no opinion about them. `extract_wikilinks` scans the body's text
directly for `[[...]]` pairs.

To keep an example wikilink written inside a code span or fenced code block
(`` `[[notes/example]]` ``) from becoming a real graph edge, the scan only
runs over the byte ranges the CommonMark parser itself did **not** classify
as `Event::Code` or inside a `Tag::CodeBlock`. `extract_links` walks
`Parser::new(body).into_offset_iter()` once, feeding each non-code span to
`extract_wikilinks_from_text` and skipping the rest.

## Ordinary `[text](dest "title")` links — parser events, not a second parser

Earlier versions hand-rolled a second Markdown parser here: find `[`, find
the matching `]`, expect `(`, find the matching `)`, treat everything
between as the destination. That approach kept needing one-off patches as
real-world pages hit CommonMark link grammar it didn't implement —
destinations with nested parentheses, link text with nested brackets, code
spans inside link text, `<...>`-wrapped destinations, optional titles, and
non-`http(s)` URI schemes each needed their own fix, and a naive
`break`-on-malformed-input patch could silently drop every later link on
the same line along with the one that failed to parse.

pulldown-cmark already implements all of that grammar correctly — it's the
same parser `ai-memory-web` uses to render page bodies to HTML (with a
different `Options` set for GFM extensions, but identical core CommonMark
link-text/destination/title parsing). So `extract_links` reads ordinary
Markdown links straight from the parser's
own `Event::Start(Tag::Link { dest_url, .. })` events during the same pass
used for code-region protection above, and hands `dest_url` to
`normalize_link_target` (URI-scheme rejection, `.md` normalization,
relative-path resolution) exactly as before.

A `*Unknown` `LinkType` (`ReferenceUnknown`, `CollapsedUnknown`,
`ShortcutUnknown`) means the reference has no matching `[ref]: dest`
definition; the parser reports it as an unresolved event with an empty
`dest_url` rather than a link, so `extract_links` skips those.

### Behavior this intentionally does **not** restore

A bare (non-`<...>`-wrapped) destination containing an unescaped space,
with no title following it, is not a valid CommonMark link at all — the
old scanner "extracting" a path like `items/my file.md` from
`[text](items/my file.md)` was itself a bug (too permissive), not a
feature worth preserving. Wrap such a destination in angle brackets
(`[text](<items/my file.md>)`) or use a `[[wikilink]]`, which has no such
restriction.

### Reference-style links now work

`[text][ref]` plus a `[ref]: dest` definition elsewhere in the body was
never supported by the old scanner (it only recognized the immediate
`](dest)` inline form). The parser resolves these natively, so they are
now extracted like any other link.
