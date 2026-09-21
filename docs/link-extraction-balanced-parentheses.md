# Balanced parentheses in Markdown link destinations

The wiki graph extractor must consume an ordinary inline Markdown link through the closing parenthesis that matches its opening delimiter. Parentheses inside a destination are allowed when balanced, so `[x](items/foo(bar).md)` points to the full `items/foo(bar).md` page rather than a truncated synthetic path.

An unbalanced destination is not a valid inline link and must not create a graph edge. The scanner also respects backslash escapes while locating the matching closing delimiter. This change is limited to delimiter matching; URI-scheme filtering and wikilink scope parsing remain separate concerns.

CommonMark 0.31.2 examples 495-500 define escaped and balanced parentheses in link destinations: https://spec.commonmark.org/0.31.2/#links
