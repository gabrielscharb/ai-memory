# Optional titles in Markdown link extraction

CommonMark inline links may place an optional title after the link destination.
Titles may be delimited by double quotes, single quotes, or parentheses. The
wiki graph stores the destination as the page edge; the title is presentation
metadata and must not become part of the path.

The extractor therefore stops the destination at separating whitespace,
validates the optional title delimiter, and resumes scanning after the inline
link's final closing parenthesis. A destination followed only by whitespace
before the closing parenthesis remains valid.

This correction is intentionally limited to optional titles on the existing
single-line ordinary Markdown-link path. Balanced destination parentheses,
angle-delimited destinations, link-text grouping and multi-line inline links
are separate parser concerns.

Reference: CommonMark 0.31.2, section 6.3 Links, especially Example 482:
https://spec.commonmark.org/0.31.2/#links
