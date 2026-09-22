# Balanced brackets in Markdown link text

Ordinary CommonMark inline-link text may contain balanced square brackets, and punctuation brackets may be backslash-escaped. The wiki graph extractor must locate the matching outer `]` before reading the destination, rather than treating the first `]` as the end of the label.

This keeps links such as `[outer [nested]](items/one.md)` and `[escaped \] bracket](items/two.md)` connected to their intended wiki pages. If an outer `[` is unmatched, scanning resumes from the next byte so a valid inner inline link is still discoverable.

The change is limited to link-text delimiter matching. Destination parsing, URI filtering, project scope, and storage behavior remain separate concerns.

CommonMark 0.31.2 examples 512-515 cover balanced and escaped brackets in link text: https://spec.commonmark.org/0.31.2/#links
