# Markdown link URI schemes

Ordinary Markdown link destinations follow URI-reference syntax. A destination whose first segment is an RFC 3986 scheme (`ssh:`, `urn:`, `vscode:`, `git+ssh:`, a Windows drive prefix such as `C:`) is external to the wiki graph even when it contains no `://`. It must not create a page dependency, graph neighbor, or dangling-link warning.

Wikilinks retain their separate `[workspace/]project:path` scope grammar. A legacy local page whose filename contains a colon can still be addressed as an explicit relative path such as `./a:b.md`, which is not a URI scheme because the colon is not in the first path segment.
