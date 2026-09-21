# Page-path validation

`PagePath` is a validated relative wiki path. Every construction route,
including Serde deserialization, must use `PagePath::new` so empty paths,
absolute paths, Windows drive prefixes, backslashes, and empty/dot segments
cannot enter a value that downstream code treats as already validated.
The serialized representation remains a plain string, including when nested
in a `LinkTarget` or a page record.

Structural validity and portability are separate checks. `ensure_portable`
remains a write-time gate: deserialization must not additionally reject
historical names such as `CON.md` or `notes/a:b.md` that the constructor can
read. This fix neither migrates the store nor rewrites existing files.

Regression tests must compare constructor and deserializer acceptance,
exercise nested domain values, preserve string round trips, and cover the
legacy-read/write-portability distinction. This closes a typed-value
validation bypass; it is not a claim of a demonstrated remote exploit.

Serde's custom deserialization interface:
https://serde.rs/impl-deserialize.html
