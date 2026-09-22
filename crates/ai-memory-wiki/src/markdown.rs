//! YAML-frontmatter aware markdown parser and emitter.
//!
//! We deliberately do *not* use `gray_matter` here: it parses fine but
//! loses comments and key ordering on re-serialise, which is exactly the
//! "duplicate frontmatter on already-frontmatter'd files" class of bug
//! basic-memory hit (#528). Going through `serde_yaml` directly keeps the
//! round-trip predictable.

use std::collections::BTreeSet;

use ai_memory_core::{LinkTarget, PagePath};
use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};

use crate::error::WikiResult;

/// A parsed markdown document with detached frontmatter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Markdown {
    /// Frontmatter as JSON for cheap querying (and stable serialisation).
    /// `Null` when the source had no frontmatter at all.
    pub frontmatter: serde_json::Value,
    /// Body excluding the frontmatter block (and the closing `---\n`).
    pub body: String,
}

/// Parse markdown text into [`Markdown`].
///
/// Recognises only the canonical `---\n<yaml>\n---\n` block at the very
/// start of the document. Anything else is treated as body.
///
/// A leading UTF-8 BOM is dropped either way. It only means "this file is
/// UTF-8" while it sits at offset zero; carried into `body` it is a
/// zero-width no-break space in front of the first line, which hides an H1
/// from [`derive_title`] and rides into the body a later re-emit writes
/// back after the frontmatter fence.
///
/// # Errors
/// Returns [`WikiError::Yaml`] if the frontmatter block exists but does
/// not parse as YAML.
pub fn parse(input: &str) -> WikiResult<Markdown> {
    let trimmed = input.strip_prefix('\u{FEFF}').unwrap_or(input);
    if let Some(rest) = trimmed.strip_prefix("---\n")
        && let Some(end) = rest.find("\n---\n")
    {
        let fm_str = &rest[..end];
        let body = rest[end + 5..].to_string();
        let fm_yaml: serde_yaml::Value = serde_yaml::from_str(fm_str)?;
        let fm_json: serde_json::Value = serde_json::to_value(fm_yaml)?;
        return Ok(Markdown {
            frontmatter: fm_json,
            body,
        });
    }
    Ok(Markdown {
        frontmatter: serde_json::Value::Null,
        body: trimmed.to_string(),
    })
}

/// Emit a [`Markdown`] back to a string. Frontmatter is serialised through
/// `serde_yaml` (so it round-trips deterministically); a `Null` or empty
/// object frontmatter is omitted entirely.
///
/// # Errors
/// Returns [`WikiError::Yaml`] if frontmatter cannot be serialised.
pub fn emit(md: &Markdown) -> WikiResult<String> {
    let has_fm = match &md.frontmatter {
        serde_json::Value::Null => false,
        serde_json::Value::Object(m) => !m.is_empty(),
        _ => true,
    };
    let mut out = String::with_capacity(md.body.len() + 32);
    if has_fm {
        let yaml = serde_yaml::to_string(&md.frontmatter)?;
        out.push_str("---\n");
        out.push_str(&yaml);
        if !yaml.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("---\n");
    }
    out.push_str(&md.body);
    Ok(out)
}

/// Derive a page title.
///
/// Priority: frontmatter.title (string) → first `# ` heading in body →
/// path stem with the `.md` suffix stripped.
#[must_use]
pub fn derive_title(frontmatter: &serde_json::Value, body: &str, path: &PagePath) -> String {
    if let Some(t) = frontmatter.get("title").and_then(serde_json::Value::as_str) {
        let trimmed = t.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    for line in body.lines() {
        if let Some(rest) = line.strip_prefix("# ") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    let s = path.as_str();
    let stem = s.rsplit_once('/').map_or(s, |(_, name)| name);
    stem.strip_suffix(".md").unwrap_or(stem).to_string()
}

/// A normalised link key: `(workspace, project, path)`. `workspace` and
/// `project` are `None` for a link that resolves within the source page's
/// own project. Collected in a `BTreeSet` so output is deduped + stable.
type LinkKey = (Option<String>, Option<String>, String);

/// Extract internal wiki links from a markdown body.
///
/// Supports `[[wiki links]]`, `[[wiki links|labels]]`, cross-project
/// `[[project:path]]` / `[[workspace/project:path]]` wikilinks, and
/// ordinary markdown links such as `[label](../decisions/foo.md#anchor)`.
/// External URLs, anchors, images, and non-markdown assets are ignored.
/// Returned values are normalised to wiki-root-relative [`LinkTarget`]s.
#[must_use]
pub fn extract_links(body: &str, page_path: &PagePath) -> Vec<LinkTarget> {
    let mut out: BTreeSet<LinkKey> = BTreeSet::new();
    let mut cursor = 0;
    let mut block_start = None;

    // Ordinary `[text](dest "title")` links are read straight from the
    // parser's own `Tag::Link` events: CommonMark's grammar for link text
    // (nested brackets, code spans), destinations (balanced parens,
    // `<...>` form), and optional titles is exactly what pulldown-cmark
    // already implements to render this same body elsewhere. A second,
    // hand-rolled implementation of that grammar here just accumulates its
    // own set of CommonMark edge cases to chase one at a time.
    //
    // `[[wikilinks]]` aren't CommonMark syntax, so they still need a
    // dedicated scan — but it reuses the parser's source ranges to skip
    // code spans/blocks, so an example wikilink written inside `` `code` ``
    // doesn't turn into a graph edge, dangling-link warning, or retrieval
    // neighbor.
    for (event, range) in Parser::new(body).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => block_start = Some(range.start),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(start) = block_start.take() {
                    extract_wikilinks_from_text(&body[cursor..start], page_path, &mut out);
                    cursor = range.end;
                }
            }
            Event::Code(_) => {
                extract_wikilinks_from_text(&body[cursor..range.start], page_path, &mut out);
                cursor = range.end;
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                // `*Unknown` link types are the parser's broken-link-callback
                // hook for an unresolved reference (`[text][no-such-ref]`);
                // without a callback registered, `dest_url` is empty and
                // this is not a real link.
                if matches!(
                    link_type,
                    LinkType::ReferenceUnknown
                        | LinkType::CollapsedUnknown
                        | LinkType::ShortcutUnknown
                ) {
                    continue;
                }
                if let Some(path) = normalize_link_target(&dest_url, page_path, false) {
                    out.insert((None, None, path));
                }
            }
            _ => {}
        }
    }
    extract_wikilinks_from_text(&body[cursor..], page_path, &mut out);

    out.into_iter()
        .filter_map(|(workspace, project, path)| {
            PagePath::new(path).ok().map(|path| LinkTarget {
                workspace,
                project,
                path,
                relation: None,
            })
        })
        .collect()
}

/// Body wikilinks plus typed `relations:` frontmatter edges — the full
/// outgoing link set every page-write path stores. One entry point so a
/// new writer cannot forget the typed half.
pub fn extract_all_links(
    frontmatter: &serde_json::Value,
    body: &str,
    page_path: &PagePath,
) -> Vec<LinkTarget> {
    let mut links = extract_links(body, page_path);
    links.extend(extract_relation_links(frontmatter));
    links
}

/// Upper bound (bytes) on an untrusted frontmatter value echoed into a
/// log line. Frontmatter is agent/operator-authored and, on a shared
/// server, one caller's page is parsed and logged by a process others
/// read the logs of; a relation key or target is meant to be a short
/// identifier, so bounding the logged form keeps a crafted or oversized
/// value from bloating or polluting the log without losing diagnostic
/// value. Mirrors the bounding every other untrusted-content sink uses.
const RELATION_LOG_FIELD_MAX_BYTES: usize = 200;

/// Bound an untrusted frontmatter value for safe logging.
fn log_bounded(value: &str) -> String {
    ai_memory_core::truncate_utf8_bytes(value, RELATION_LOG_FIELD_MAX_BYTES)
}

/// Extract typed relation edges from a page's `relations:` frontmatter
/// (2.0 item 3):
///
/// ```yaml
/// relations:
///   fixes: ["gotchas/build.md"]
///   contradicts: ["decisions/0007.md", "other-project:notes/x.md"]
/// ```
///
/// Values use the same target grammar as wikilinks (`path`,
/// `project:path`, `workspace/project:path`). Keys outside the closed
/// [`Relation`] vocabulary are skipped (a typo must not silently mint a
/// new edge kind); malformed paths are skipped likewise.
pub fn extract_relation_links(frontmatter: &serde_json::Value) -> Vec<LinkTarget> {
    let Some(relations) = frontmatter.get("relations").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, targets) in relations {
        let Some(relation) = ai_memory_core::Relation::parse(key) else {
            tracing::warn!(key = %log_bounded(key), "unknown relation key in frontmatter; skipping");
            continue;
        };
        let Some(list) = targets.as_array() else {
            continue;
        };
        for target in list.iter().filter_map(|v| v.as_str()) {
            let (workspace, project, raw_path) = match target.split_once(':') {
                None => (None, None, target),
                Some((scope, path)) => match scope.split_once('/') {
                    None => (None, Some(scope.to_string()), path),
                    Some((ws, proj)) => (Some(ws.to_string()), Some(proj.to_string()), path),
                },
            };
            // Same terminal normalization as wikilinks: extension-less
            // targets gain `.md`; anything with a non-md extension is
            // not a page and is skipped.
            let raw_path = raw_path.trim();
            let last = raw_path.rsplit_once('/').map_or(raw_path, |(_, s)| s);
            let normalized = if last.contains('.') {
                if raw_path.ends_with(".md") {
                    raw_path.to_string()
                } else {
                    tracing::warn!(target = %log_bounded(target), "relation target is not a page; skipping");
                    continue;
                }
            } else {
                format!("{raw_path}.md")
            };
            let Ok(path) = PagePath::new(normalized) else {
                tracing::warn!(target = %log_bounded(target), "unparseable relation target; skipping");
                continue;
            };
            out.push(LinkTarget {
                workspace,
                project,
                path,
                relation: Some(relation),
            });
        }
    }
    out
}

/// Split an optional `[workspace/]project:` scope qualifier off the front
/// of a wikilink target. Returns `(workspace, project, path_part)`. URL and
/// scheme-prefixed targets carry no scope (the `:` belongs to the scheme);
/// [`normalize_link_target`] rejects those downstream.
fn split_scope(target: &str) -> LinkKey {
    let lower = target.to_ascii_lowercase();
    if target.contains("://")
        || lower.starts_with("mailto:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("tel:")
    {
        return (None, None, target.to_string());
    }
    if let Some((scope, rest)) = target.split_once(':') {
        let scope = scope.trim();
        let scope_ok = !scope.is_empty()
            && scope
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'));
        if scope_ok {
            let (workspace, project) = match scope.split_once('/') {
                Some((ws, proj)) => (Some(ws.trim().to_string()), proj.trim()),
                None => (None, scope),
            };
            if !project.is_empty() {
                return (
                    workspace,
                    Some(project.to_string()),
                    rest.trim().to_string(),
                );
            }
        }
    }
    (None, None, target.to_string())
}

fn extract_wikilinks_from_text(text: &str, page_path: &PagePath, out: &mut BTreeSet<LinkKey>) {
    for line in text.lines() {
        extract_wikilinks(line, page_path, out);
    }
}

fn extract_wikilinks(line: &str, page_path: &PagePath, out: &mut BTreeSet<LinkKey>) {
    let mut rest = line;
    while let Some(start) = rest.find("[[") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        let raw = &after_start[..end];
        // Strip the `|label` first, then peel any cross-project scope so the
        // remaining path normalises the same way a bare wikilink does.
        let unlabelled = raw.split_once('|').map_or(raw, |(target, _)| target).trim();
        let (workspace, project, path_part) = split_scope(unlabelled);
        if let Some(path) = normalize_link_target(&path_part, page_path, true) {
            out.insert((workspace, project, path));
        }
        rest = &after_start[end + 2..];
    }
}

/// True when `target` starts with an RFC 3986 URI scheme (`scheme:`) — for
/// example `ssh:`, `urn:`, `vscode:`, `git+ssh:`. Ordinary Markdown link
/// destinations use full URI-reference syntax, so any leading scheme marks
/// an external target even without a `//` authority. A single-letter
/// scheme (`c:`) also matches, which is intentional: it catches a Windows
/// drive-letter destination too. `./a:b.md` avoids this on purpose — a
/// leading `./` is not itself a valid scheme character, so it stays a
/// relative page path.
fn has_uri_scheme(target: &str) -> bool {
    let Some((scheme, _)) = target.split_once(':') else {
        return false;
    };
    let mut chars = scheme.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

fn normalize_link_target(raw: &str, page_path: &PagePath, wikilink: bool) -> Option<String> {
    // Shared by both callers. For a Markdown link this is a no-op: the
    // parser already strips a `<...>` destination wrapper before `dest_url`
    // gets here. For a wikilink, `raw` is hand-scanned text, so an
    // incidental `<...>` around it is still worth trimming defensively.
    let target = raw.trim().trim_matches('<').trim_matches('>');
    if target.is_empty() || target.starts_with('#') || target.contains("://") {
        return None;
    }
    let lower = target.to_ascii_lowercase();
    if lower.starts_with("mailto:")
        || lower.starts_with("data:")
        || lower.starts_with("javascript:")
        || lower.starts_with("tel:")
    {
        return None;
    }
    // Ordinary Markdown destinations use URI-reference syntax: a leading
    // RFC 3986 scheme is external even without `://` (for example `ssh:`
    // or `urn:`). Wikilinks keep their own separate `project:path` scope
    // grammar, already peeled off by `split_scope` before this is called.
    if !wikilink && has_uri_scheme(target) {
        return None;
    }

    let target = target.split_once('#').map_or(target, |(path, _)| path);
    let target = target
        .split_once('?')
        .map_or(target, |(path, _)| path)
        .trim();
    if target.is_empty() || target.contains('\\') {
        return None;
    }

    let mut target = target.to_string();
    let last_segment = target.rsplit_once('/').map_or(target.as_str(), |(_, s)| s);
    if last_segment.contains('.') {
        if !target.ends_with(".md") {
            return None;
        }
    } else if wikilink || !last_segment.is_empty() {
        target.push_str(".md");
    }

    resolve_relative(page_path, &target, wikilink)
}

fn resolve_relative(page_path: &PagePath, target: &str, root_relative: bool) -> Option<String> {
    let mut parts: Vec<&str> = if root_relative || target.starts_with('/') {
        Vec::new()
    } else {
        page_path
            .as_str()
            .rsplit_once('/')
            .map_or_else(Vec::new, |(dir, _)| {
                dir.split('/').filter(|part| !part.is_empty()).collect()
            })
    };

    for part in target.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> PagePath {
        PagePath::new("notes/here.md").unwrap()
    }

    #[test]
    fn untrusted_relation_values_are_bounded_before_logging() {
        // A crafted, oversized relation key/target must not reach the log
        // unbounded (security-audit: untrusted frontmatter -> log sink).
        let short = "fixes";
        assert_eq!(log_bounded(short), short, "short values pass through");

        let huge = "x".repeat(10_000);
        let bounded = log_bounded(&huge);
        assert!(
            bounded.len() <= RELATION_LOG_FIELD_MAX_BYTES,
            "logged value must be bounded: {} bytes",
            bounded.len()
        );

        // Never split a UTF-8 code point mid-truncation.
        let multibyte = "é".repeat(10_000);
        let bounded = log_bounded(&multibyte);
        assert!(bounded.len() <= RELATION_LOG_FIELD_MAX_BYTES);
        assert!(std::str::from_utf8(bounded.as_bytes()).is_ok());

        // A malicious relation block still yields no edges and does not
        // panic — the bound is applied on the skip path.
        let fm = serde_json::json!({
            "relations": { "x".repeat(5_000): ["ok.md"] }
        });
        assert!(extract_relation_links(&fm).is_empty());
    }

    #[test]
    fn relations_frontmatter_becomes_typed_edges() {
        let fm = serde_json::json!({
            "relations": {
                "fixes": ["gotchas/build.md", "concepts/writer"],
                "contradicts": ["other-proj:decisions/0007.md"],
                "causes": ["ws/proj:notes/x.md"],
            }
        });
        let mut links = extract_relation_links(&fm);
        links.sort();
        assert_eq!(links.len(), 4);
        let fixes: Vec<_> = links
            .iter()
            .filter(|l| l.relation == Some(ai_memory_core::Relation::Fixes))
            .collect();
        assert_eq!(fixes.len(), 2);
        // extension-less target gains .md
        assert!(
            fixes
                .iter()
                .any(|l| l.path.as_str() == "concepts/writer.md")
        );
        let contra = links
            .iter()
            .find(|l| l.relation == Some(ai_memory_core::Relation::Contradicts))
            .unwrap();
        assert_eq!(contra.project.as_deref(), Some("other-proj"));
        let causes = links
            .iter()
            .find(|l| l.relation == Some(ai_memory_core::Relation::Causes))
            .unwrap();
        assert_eq!(causes.workspace.as_deref(), Some("ws"));
        assert_eq!(causes.project.as_deref(), Some("proj"));
    }

    #[test]
    fn unknown_relation_keys_and_bad_targets_are_skipped() {
        let fm = serde_json::json!({
            "relations": {
                "blames": ["notes/a.md"],
                "fixes": ["../escape.md", "notes/data.json", "notes/ok.md"],
            }
        });
        let links = extract_relation_links(&fm);
        assert_eq!(links.len(), 1, "{links:?}");
        assert_eq!(links[0].path.as_str(), "notes/ok.md");
    }

    #[test]
    fn pages_without_relations_extract_nothing() {
        assert!(extract_relation_links(&serde_json::json!({})).is_empty());
        assert!(extract_relation_links(&serde_json::json!({"relations": "not a map"})).is_empty());
    }

    #[test]
    fn extract_all_links_merges_body_and_frontmatter() {
        let fm = serde_json::json!({"relations": {"fixes": ["gotchas/g.md"]}});
        let links = extract_all_links(&fm, "see [[notes/n.md]]", &page());
        assert_eq!(links.len(), 2);
        assert!(links.iter().any(|l| l.relation.is_none()));
        assert!(
            links
                .iter()
                .any(|l| l.relation == Some(ai_memory_core::Relation::Fixes))
        );
    }

    #[test]
    fn extract_links_bare_wikilink_is_local() {
        let links = extract_links("see [[decisions/0001.md]] and [[other]]", &page());
        assert!(links.iter().all(|l| !l.is_cross_project()));
        assert!(
            links
                .iter()
                .any(|l| l.path.as_str() == "decisions/0001.md" && l.project.is_none())
        );
        // bare name gets `.md` appended, still local
        assert!(links.iter().any(|l| l.path.as_str() == "other.md"));
    }

    #[test]
    fn extract_links_cross_project_wikilink() {
        let links = extract_links("dep on [[infra:runbooks/02.md]]", &page());
        let l = links.iter().find(|l| l.is_cross_project()).expect("xproj");
        assert_eq!(l.workspace, None);
        assert_eq!(l.project.as_deref(), Some("infra"));
        assert_eq!(l.path.as_str(), "runbooks/02.md");
    }

    #[test]
    fn extract_links_cross_workspace_wikilink_with_label() {
        let links = extract_links("[[zommehq/zomme:decisions/adr-1.md|the ADR]]", &page());
        let l = links.iter().find(|l| l.is_cross_project()).expect("xws");
        assert_eq!(l.workspace.as_deref(), Some("zommehq"));
        assert_eq!(l.project.as_deref(), Some("zomme"));
        assert_eq!(l.path.as_str(), "decisions/adr-1.md");
    }

    #[test]
    fn extract_links_url_wikilink_is_not_a_scope() {
        // `https://...` must not be parsed as project "https".
        let links = extract_links("[[https://example.com]] [[mailto:a@b.com]]", &page());
        assert!(links.is_empty(), "URLs/schemes are not links: {links:?}");
    }

    #[test]
    fn parses_frontmatter_and_body() {
        let src = "---\ntitle: Hello\ntags:\n  - a\n  - b\n---\nThe body.\n";
        let md = parse(src).unwrap();
        assert_eq!(md.frontmatter["title"], "Hello");
        assert_eq!(md.frontmatter["tags"][0], "a");
        assert_eq!(md.body, "The body.\n");
    }

    #[test]
    fn parses_bom_prefixed_frontmatter() {
        let src = "\u{FEFF}---\ntitle: Hello\n---\nBody\n";
        let md = parse(src).unwrap();
        assert_eq!(md.frontmatter["title"], "Hello");
        assert_eq!(md.body, "Body\n");
    }

    /// A page a Windows editor saved with a UTF-8 BOM and no frontmatter:
    /// the mark belongs to the file, not to the first line. Left in `body`
    /// it sits in front of the `#`, so the H1 stops being a heading and the
    /// page is indexed under its filename instead of its title.
    #[test]
    fn parses_bom_prefixed_body_without_frontmatter() {
        let src = "\u{FEFF}# Hand written\n\nBody.\n";
        let md = parse(src).unwrap();
        assert!(md.frontmatter.is_null());
        assert_eq!(md.body, "# Hand written\n\nBody.\n");
        assert_eq!(
            derive_title(
                &md.frontmatter,
                &md.body,
                &PagePath::new("notes/hand-written.md").unwrap(),
            ),
            "Hand written"
        );
    }

    #[test]
    fn malformed_frontmatter_returns_error() {
        let src = "---\ntitle: [unterminated\n---\nBody\n";
        assert!(parse(src).is_err());
    }

    #[test]
    fn unterminated_frontmatter_marker_is_body() {
        let src = "---\ntitle: Hello\nBody\n";
        let md = parse(src).unwrap();
        assert!(md.frontmatter.is_null());
        assert_eq!(md.body, src);
    }

    #[test]
    fn parses_body_without_frontmatter() {
        let src = "Just a body, no frontmatter.\n";
        let md = parse(src).unwrap();
        assert!(md.frontmatter.is_null());
        assert_eq!(md.body, src);
    }

    #[test]
    fn round_trip_emit_then_parse() {
        let original = Markdown {
            frontmatter: serde_json::json!({ "title": "X", "tags": ["a"] }),
            body: "Line 1\nLine 2\n".into(),
        };
        let emitted = emit(&original).unwrap();
        let parsed = parse(&emitted).unwrap();
        assert_eq!(parsed.frontmatter["title"], "X");
        assert_eq!(parsed.body, original.body);
    }

    #[test]
    fn round_trip_preserves_slot_kind_frontmatter() {
        let original = Markdown {
            frontmatter: serde_json::json!({
                "title": "Project context",
                "slot_kind": "invariant",
            }),
            body: "Stable project context.\n".into(),
        };
        let emitted = emit(&original).unwrap();
        let parsed = parse(&emitted).unwrap();
        assert_eq!(parsed.frontmatter["slot_kind"], "invariant");
        assert_eq!(parsed.body, original.body);
    }

    #[test]
    fn emit_omits_empty_frontmatter() {
        let md = Markdown {
            frontmatter: serde_json::Value::Object(serde_json::Map::new()),
            body: "Hello\n".into(),
        };
        assert_eq!(emit(&md).unwrap(), "Hello\n");
    }

    #[test]
    fn title_priority_frontmatter_then_heading_then_stem() {
        let path = PagePath::new("notes/foo.md").unwrap();
        // Frontmatter wins.
        let fm = serde_json::json!({ "title": "Explicit" });
        assert_eq!(derive_title(&fm, "# Other\nbody", &path), "Explicit");
        // Heading wins over stem.
        assert_eq!(
            derive_title(&serde_json::Value::Null, "# From Body\n", &path),
            "From Body"
        );
        // Stem fallback.
        assert_eq!(
            derive_title(&serde_json::Value::Null, "no heading", &path),
            "foo"
        );
    }

    #[test]
    fn extracts_internal_wiki_and_markdown_links() {
        let path = PagePath::new("concepts/current.md").unwrap();
        let body = "See [[decisions/0001-single-sqlite-file|SQLite]] and \
                    [gotcha](../gotchas/hooks.md#details). Also \
                    [external](https://example.com) and ![image](../img/logo.png).";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["decisions/0001-single-sqlite-file.md", "gotchas/hooks.md"]
        );
    }

    #[test]
    fn extract_links_ignores_fenced_code_blocks() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "```\n[[notes/ignored]]\n```\n[[notes/kept]]\n";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/kept.md"]);
    }

    #[test]
    fn extract_links_ignores_commonmark_code_regions() {
        let path = PagePath::new("notes/a.md").unwrap();
        for body in [
            "Use `[[notes/inline]]` literally, then [[notes/kept]].",
            "Use `` `[[notes/double]]` `` literally, then [[notes/kept]].",
            "Use `[fake](notes/markdown-code.md)` literally, then [[notes/kept]].",
            "````\n```\n[[notes/inside-long-fence]]\n````\n[[notes/kept]]",
            "> ```\n> [[notes/quoted-fence]]\n> ```\n\n[[notes/kept]]",
            "    [[notes/indented-code]]\n\n[[notes/kept]]",
        ] {
            let links = extract_links(body, &path);
            let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
            assert_eq!(paths, vec!["notes/kept.md"], "body: {body:?}");
        }
    }

    #[test]
    fn markdown_links_with_uri_schemes_are_not_graph_edges() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "[ssh](ssh:host/page.md) [urn](urn:example:thing.md) \
                    [vscode](vscode:notes/page.md) [git](git+ssh:host/page.md) \
                    [drive](C:/notes/page.md) [legacy](./a:b.md) [[notes/kept]]";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/a:b.md", "notes/kept.md"]);
    }

    #[test]
    fn markdown_links_with_balanced_parentheses_keep_full_destination() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "[one](items/foo(bar).md) [nested](items/a(b(c)d)e.md) \
                    [bad](items/unbalanced(one.md) [[notes/kept]]";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "notes/items/a(b(c)d)e.md",
                "notes/items/foo(bar).md",
                "notes/kept.md",
            ]
        );
    }

    #[test]
    fn markdown_links_with_balanced_or_escaped_brackets_in_text_are_extracted() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "[outer [nested]](items/one.md) [escaped \\] bracket](items/two.md) [outer [inner](items/inner.md)";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "notes/items/inner.md",
                "notes/items/one.md",
                "notes/items/two.md",
            ]
        );
    }

    #[test]
    fn markdown_link_text_code_spans_are_opaque_to_bracket_matching() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "[open `[` code](items/open.md) [close `]` code](items/close.md) [paired ``[x]`` code](items/paired.md)";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "notes/items/close.md",
                "notes/items/open.md",
                "notes/items/paired.md",
            ]
        );
    }

    #[test]
    fn extract_links_angle_destination_keeps_parenthesis() {
        let path = PagePath::new("notes/here.md").unwrap();
        let links = extract_links("[x](<items/foo)bar.md>)", &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/items/foo)bar.md"]);
    }

    #[test]
    fn extract_links_with_optional_titles_keep_destination() {
        let path = PagePath::new("notes/here.md").unwrap();
        let body = r#"[double](items/double.md "Double title") [single](items/single.md 'Single title') [paren](items/paren.md (Paren title)) [spaces](items/spaces.md   )"#;
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|link| link.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "notes/items/double.md",
                "notes/items/paren.md",
                "notes/items/single.md",
                "notes/items/spaces.md",
            ]
        );
    }

    /// Regression guard: an earlier hand-rolled paren-balance tracker used
    /// to `break` the whole-line scan on an unclosed destination, silently
    /// dropping every later link on the same line along with the malformed
    /// one. Parser-event extraction can't have that failure mode — a failed
    /// link is just an event the parser never emits, the rest of the
    /// document is unaffected.
    #[test]
    fn malformed_link_does_not_drop_later_links_on_the_same_line() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "[bad](unterminated(oops.md) and later [good](good.md)";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/good.md"]);
    }

    /// Documents an intentional behavior change, not a bug: CommonMark
    /// requires a bare (non-`<...>`-wrapped) destination to have no
    /// unescaped whitespace. A destination with a literal space and no
    /// `<...>` wrapper or title is therefore not a valid link at all, so
    /// the old permissive scanner "extracting" it was itself the bug —
    /// spec-compliant parsing intentionally drops it. Use `<...>` around a
    /// destination that needs a literal space.
    #[test]
    fn bare_destination_with_unescaped_space_and_no_title_is_not_a_link() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "[spaced file](items/my file.md) [[notes/kept]]";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/kept.md"]);

        let escaped = "[spaced file](<items/my file.md>) [[notes/kept]]";
        let links = extract_links(escaped, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/items/my file.md", "notes/kept.md"]);
    }

    /// Reference-style links (`[text][ref]` plus a `[ref]: dest` definition)
    /// were never supported by the old hand-rolled scanner at all — it only
    /// recognized the immediate `](` inline form. The parser resolves these
    /// natively, and correctly treats an unresolved reference as plain text
    /// rather than a link.
    #[test]
    fn reference_style_links_resolve_through_their_definition() {
        let path = PagePath::new("notes/a.md").unwrap();
        let body = "See [the doc][ref] and [an unresolved one][missing].\n\n[ref]: items/target.md";
        let links = extract_links(body, &path);
        let paths: Vec<&str> = links.iter().map(|l| l.path.as_str()).collect();
        assert_eq!(paths, vec!["notes/items/target.md"]);
    }
}
