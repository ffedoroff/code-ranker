//! Manifest-based Markdown composition: assemble a language doc from a base.
//!
//! The doc corpus inherits like config does (`defaults.toml ⊕ <lang>.toml`): a
//! language-neutral `base/<ID>.md` holds the theory/algorithm/history/references
//! as `## ` sections, and each `languages/<lang>/<ID>.md` is a **manifest** that
//! lists, in order, the sections of the final doc. A section is either pulled from
//! the base by reference or written inline (a new or rewritten section):
//!
//! ```text
//! <!-- doc:base "Canonical sources" -->                       whole base section
//! <!-- doc:base "ADP at the package level" from "\nBigger picture:" -->   a slice
//! <!-- doc:base "X" from "P1" to "P2" -->                     the slice P1..=P2
//!
//! ## In Rust                                                  an inline section
//! (its body, written here verbatim)
//! ```
//!
//! Output order = manifest order; a base section not referenced is simply absent
//! (the manifest is authoritative — the language controls the full structure). The
//! H1 + preamble is the manifest's own when it writes one (a leading `# ` line),
//! else the base head auto-suffixed `(in <Lang>)`.
//!
//! `from`/`to` are optional slice anchors into the named base section: `from` keeps
//! the section text from that phrase onward, `to` keeps it up to and including that
//! phrase. A `\n` in a phrase anchors it to a line start. A `doc:base` naming a
//! missing section — or a `from`/`to` phrase not found — is a hard error (an
//! authoring typo must fail loudly, not silently no-op).

use anyhow::{Result, bail};
use std::collections::BTreeMap;

/// Split a Markdown doc into its leading head (H1 + any preamble before the first
/// `## `) and a `heading → block` map (each `## …` line through to just before the
/// next heading).
fn split_base(md: &str) -> (String, BTreeMap<String, String>) {
    let mut head = String::new();
    let mut sections: BTreeMap<String, String> = BTreeMap::new();
    let mut cur: Option<String> = None;
    for line in md.lines() {
        if let Some(title) = line.strip_prefix("## ") {
            cur = Some(title.trim().to_string());
            sections.entry(cur.clone().unwrap()).or_default();
        }
        let target = match &cur {
            Some(h) => sections.get_mut(h).expect("section just inserted"),
            None => &mut head,
        };
        target.push_str(line);
        target.push('\n');
    }
    (head, sections)
}

/// A `doc:base` include: a base section name with optional `from`/`to` slice
/// anchors (phrases within that section's text).
struct Include {
    section: String,
    from: Option<String>,
    to: Option<String>,
}

/// One manifest item, in document order: literal Markdown written in the manifest
/// (an inline `## ` section, or blank lines), or a base-section include.
enum Item {
    Literal(String),
    Include(Include),
}

/// Parse a manifest into its ordered items. A line `<!-- doc:base "S" … -->` is an
/// include; everything else accumulates as literal Markdown.
fn parse_manifest(md: &str) -> Result<Vec<Item>> {
    let mut items: Vec<Item> = Vec::new();
    let mut lit = String::new();
    for line in md.lines() {
        if let Some(inc) = parse_include(line)? {
            if !lit.is_empty() {
                items.push(Item::Literal(std::mem::take(&mut lit)));
            }
            items.push(Item::Include(inc));
        } else {
            lit.push_str(line);
            lit.push('\n');
        }
    }
    if !lit.is_empty() {
        items.push(Item::Literal(lit));
    }
    Ok(items)
}

/// Parse a `<!-- doc:base "S" [from "P1"] [to "P2"] -->` line. Returns `Ok(None)`
/// for any non-`doc:base` line (kept as literal); errors on a malformed directive.
fn parse_include(line: &str) -> Result<Option<Include>> {
    let t = line.trim();
    let Some(inner) = t
        .strip_prefix("<!-- doc:base")
        .and_then(|s| s.strip_suffix("-->"))
    else {
        return Ok(None);
    };
    let (section, mut rest) = take_quoted(inner.trim())
        .ok_or_else(|| anyhow::anyhow!("doc:base needs a quoted section name: {line:?}"))?;
    let (mut from, mut to) = (None, None);
    rest = rest.trim();
    while !rest.is_empty() {
        let (kw, after) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        let (phrase, r) = take_quoted(after.trim_start())
            .ok_or_else(|| anyhow::anyhow!("doc:base {kw} needs a quoted phrase: {line:?}"))?;
        match kw {
            "from" => from = Some(unescape(&phrase)),
            "to" => to = Some(unescape(&phrase)),
            other => bail!("unknown doc:base clause {other:?} in {line:?}"),
        }
        rest = r.trim();
    }
    Ok(Some(Include { section, from, to }))
}

/// Take a leading `"…"` quoted token, returning `(contents, rest_after_close)`.
fn take_quoted(s: &str) -> Option<(String, &str)> {
    let rest = s.trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some((rest[..end].to_string(), &rest[end + 1..]))
}

/// Interpret `\n` in a phrase as a real newline, so a `from`/`to` anchor can pin to
/// a line start (e.g. `from "\nBigger picture:"`).
fn unescape(s: &str) -> String {
    s.replace("\\n", "\n")
}

/// Resolve an include against the base sections: the whole `## section` block, or
/// the slice bounded by `from` (inclusive start) and `to` (inclusive end).
fn resolve(inc: &Include, sections: &BTreeMap<String, String>) -> Result<String> {
    let block = sections.get(&inc.section).ok_or_else(|| {
        anyhow::anyhow!(
            "doc:base names a section absent from base: {:?}",
            inc.section
        )
    })?;
    if inc.from.is_none() && inc.to.is_none() {
        return Ok(block.clone());
    }
    let start = match &inc.from {
        Some(p) => block.find(p.as_str()).ok_or_else(|| {
            anyhow::anyhow!("doc:base from phrase not in {:?}: {:?}", inc.section, p)
        })?,
        None => 0,
    };
    let end = match &inc.to {
        Some(p) => {
            let rel = block[start..].find(p.as_str()).ok_or_else(|| {
                anyhow::anyhow!("doc:base to phrase not in {:?}: {:?}", inc.section, p)
            })?;
            start + rel + p.len()
        }
        None => block.len(),
    };
    Ok(block[start..end].to_string())
}

/// Compose a language doc from its `manifest` and the neutral `base`. `lang_display`
/// is the H1 suffix language (e.g. `Rust`).
///
/// The head (H1 + preamble before the first section) is the manifest's own when it
/// supplies one (a leading `# ` line — used verbatim, so a language can word its own
/// TL;DR); otherwise the base head, auto-suffixed `(in <Lang>)`.
pub(crate) fn compose(manifest: &str, base: &str, lang_display: &str) -> Result<String> {
    let (base_head, sections) = split_base(base);
    let (man_head, man_body) = split_head(manifest);
    let head = if man_head.lines().any(|l| l.starts_with("# ")) {
        man_head
    } else {
        suffix_h1(&base_head, &format!(" (in {lang_display})"))
    };
    let mut out = ensure_trailing_blank(&head);
    for item in parse_manifest(man_body)? {
        let block = match item {
            Item::Literal(t) => t,
            Item::Include(inc) => resolve(&inc, &sections)?,
        };
        if !block.trim().is_empty() {
            out.push_str(&ensure_trailing_blank(&block));
        }
    }
    Ok(tidy_blanks(&out))
}

/// Split a manifest into its head (everything before the first `## ` section or
/// `<!-- doc:base … -->` include) and the remaining body.
fn split_head(manifest: &str) -> (String, &str) {
    let mut offset = 0;
    for line in manifest.lines() {
        let t = line.trim_start();
        if t.starts_with("## ") || t.starts_with("<!-- doc:base") {
            return (manifest[..offset].to_string(), &manifest[offset..]);
        }
        offset += line.len() + 1; // + the '\n' `lines()` stripped
    }
    (manifest.to_string(), "")
}

/// Append `" suffix"` to the document's first `# ` (H1) line.
fn suffix_h1(head: &str, suffix: &str) -> String {
    let mut done = false;
    head.lines()
        .map(|line| {
            if !done && line.starts_with("# ") {
                done = true;
                format!("{}{suffix}", line.trim_end())
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// Ensure a block ends with exactly one blank line, so concatenated sections keep
/// a paragraph break between them.
fn ensure_trailing_blank(s: &str) -> String {
    format!("{}\n\n", s.trim_end())
}

/// Collapse runs of 3+ blank lines to a single blank line; trim a trailing run to
/// one final newline.
fn tidy_blanks(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut blanks = 0usize;
    for line in s.lines() {
        if line.trim().is_empty() {
            blanks += 1;
            if blanks <= 1 {
                out.push('\n');
            }
        } else {
            blanks = 0;
            out.push_str(line);
            out.push('\n');
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

#[cfg(test)]
#[path = "compose_test.rs"]
mod tests;
