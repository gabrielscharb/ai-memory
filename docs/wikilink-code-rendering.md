# Wikilinks inside code

The web renderer expands `[[page]]` shorthand only outside Markdown code.
Code examples must retain their source text, not display generated Markdown
links inside `<code>` or `<pre><code>`.

Use the same pulldown-cmark options and source offsets as the renderer to
identify protected code regions. This covers equal-length inline backtick
runs, multiline code spans, indented code, fences nested in block quotes or
lists, and fences that close only with a sufficiently long run of the same
character. A fence-like line carrying an info string is not a closing fence.

Outside code, the existing scoped target resolver, label escaping, HTML
escaping, and link/image URL policies still apply. This change concerns web
rendering only; it does not alter stored Markdown or the wiki link index.

Regression tests check both literal code content and working links after the
code block, including Unicode text before byte-offset boundaries. Parsing a
second time is needed only for source containing `[[`; documents without
wikilinks skip preprocessing.

References:
https://spec.commonmark.org/0.31.2/#fenced-code-blocks
https://spec.commonmark.org/0.31.2/#code-spans
