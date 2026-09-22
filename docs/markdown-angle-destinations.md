# Angle-delimited Markdown link destinations

The wiki graph extractor recognizes ordinary inline Markdown links as internal
page edges when their destination resolves to a page path.

CommonMark has two destination forms. In the angle-delimited form, for example
`[label](<items/foo)bar.md>)`, a right parenthesis inside `<...>` is
destination content. The inline link closes only after the matching `>` and
the following `)`.

The extractor therefore finds the unescaped closing angle bracket before it
looks for the inline-link closing parenthesis. The existing normalization then
removes the `<...>` delimiters and resolves the complete internal path.

This correction is intentionally limited to angle-delimited destinations.
Balanced-parenthesis destinations, URI-scheme rejection, optional link titles,
wikilink scope syntax, and code-region handling are separate concerns with
their own regression coverage.

Reference: CommonMark 0.31.2, inline links, example 492 and surrounding cases:
https://spec.commonmark.org/0.31.2/#links
