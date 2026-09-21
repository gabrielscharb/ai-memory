# Search query syntax

Bare multi-word searches use OR to favor recall. Common English
stopwords are omitted unless the query consists only of stopwords.
Punctuated identifiers retain the existing whole-token and split-path
alternatives.

Valid explicit SQLite FTS5 expressions are preserved before any
whitespace splitting or punctuation rewriting. For example:

- `"exact phrase" AND deploy` requires adjacent phrase terms.
- `(foo OR bar) AND baz` applies `baz` to the whole parenthesized group.
- `NEAR(alpha beta, 1)` preserves the requested proximity.
- `title:"exact phrase" AND body:deploy` retains column restrictions.

Malformed expressions still use the existing safe fallback rather than
being sent directly to MATCH. Query text remains a bound SQL parameter.
This change does not alter project isolation, permissions, ranking
weights, the index schema, or the default broad-recall search mode.

The store regression tests assert returned document IDs, not only parse
success, because a rewritten expression can be valid but mean something
different. See SQLite's authoritative syntax reference:
https://www.sqlite.org/fts5.html#full_text_query_syntax
