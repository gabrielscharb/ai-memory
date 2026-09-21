# Link extraction and code regions

Outgoing page edges are extracted only from Markdown content outside code spans and code blocks. Literal examples such as `[[notes/example]]` or `[label](notes/example.md)` must remain examples; they must not create graph neighbors, dependencies, or dangling-link lint findings.

The extractor uses pulldown-cmark source ranges to identify inline code, fenced code (including nested block quotes and longer delimiters), and indented code. Text outside those protected ranges keeps the existing wikilink and Markdown-link normalization rules.

Regression coverage includes single- and multi-backtick spans, Markdown links in code, longer nested fences, block-quoted fences, indented code, and a real link after each code example to ensure extraction resumes correctly.
