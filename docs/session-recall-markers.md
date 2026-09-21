# Session-recall marker normalization

Session-recall routing is an optional, zero-LLM ranking signal. It is off by
default, and this correction does not change its activation flag, ranking
weights, marker vocabulary, or Chinese substring matching.

English recall phrases such as `last session`, `last time`, and
`the other day` are matched after lowercasing and normalizing separators.
Repeated spaces, tabs, line endings, nonbreaking spaces, and punctuation
between marker words now behave like a single separator. A query pasted
with a line break between `last` and `session` therefore receives the same
routing decision as the equivalent one-line query.

Normalization retains word boundaries: `lastly session`, `last timesheet`,
and `blast timer` must not match the corresponding recall phrases. Empty
or separator-only input must not become a session-recall request.

The regression matrix covers all 17 existing English markers with six
separator forms, plus negative boundary and empty-input controls. This is
lexical normalization only, not fuzzy matching or semantic intent detection.
