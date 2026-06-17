//! Per-language overrides of the global report view/stat lists.
//!
//! The report's column order, card-featured metrics, and JSON `stats` keys come
//! from the global metric catalog (`code-ranker-graph/metrics/builtin.toml`). A
//! language may *patch* those inherited lists from its `<lang>.toml` `[report]`
//! section — add a language-specific metric (e.g. Rust `unsafe`), drop some, swap
//! one in place, or replace the list wholesale — without restating the whole
//! catalog. [`ListPatch`] is the patch primitive; the parsing of the TOML
//! `[report]` section into these types lives in `code-ranker-plugins`.

/// A patch over an inherited ordered string list. Either a wholesale
/// [`replace_all`](Self::replace_all) (a plain TOML array) or a set of in-place
/// edits applied to the inherited base, in this order: `clear` → `remove` →
/// `replace` → `after` / `before` → `prepend` → `add`. The result is
/// de-duplicated, keeping the first occurrence (order-stable).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListPatch {
    /// Plain-array form: replace the inherited list outright (then dedup).
    pub replace_all: Option<Vec<String>>,
    /// Start from an empty list instead of the inherited base.
    pub clear: bool,
    /// Drop every element equal to one of these.
    pub remove: Vec<String>,
    /// Swap an element in place, preserving its position: `(old, new)`.
    pub replace: Vec<(String, String)>,
    /// Insert items immediately **after** an anchor element: `(anchor, items)`.
    /// No-op if the anchor is absent.
    pub after: Vec<(String, Vec<String>)>,
    /// Insert items immediately **before** an anchor element: `(anchor, items)`.
    pub before: Vec<(String, Vec<String>)>,
    /// Insert at the front (before the inherited elements).
    pub prepend: Vec<String>,
    /// Append at the end.
    pub add: Vec<String>,
}

impl ListPatch {
    /// Apply the patch to `base`, returning the resulting order-stable, de-duped list.
    pub fn apply(&self, base: &[String]) -> Vec<String> {
        if let Some(all) = &self.replace_all {
            return dedup(all.clone());
        }
        let mut out: Vec<String> = if self.clear {
            Vec::new()
        } else {
            base.to_vec()
        };
        if !self.remove.is_empty() {
            out.retain(|x| !self.remove.iter().any(|r| r == x));
        }
        for (old, new) in &self.replace {
            if let Some(pos) = out.iter().position(|x| x == old) {
                out[pos] = new.clone();
            }
        }
        for (anchor, items) in &self.after {
            if let Some(pos) = out.iter().position(|x| x == anchor) {
                out.splice(pos + 1..pos + 1, items.iter().cloned());
            }
        }
        for (anchor, items) in &self.before {
            if let Some(pos) = out.iter().position(|x| x == anchor) {
                out.splice(pos..pos, items.iter().cloned());
            }
        }
        if !self.prepend.is_empty() {
            let mut front = self.prepend.clone();
            front.extend(out);
            out = front;
        }
        out.extend(self.add.iter().cloned());
        dedup(out)
    }

    /// True when the patch makes no change (no override declared).
    pub fn is_noop(&self) -> bool {
        self.replace_all.is_none()
            && !self.clear
            && self.remove.is_empty()
            && self.replace.is_empty()
            && self.after.is_empty()
            && self.before.is_empty()
            && self.prepend.is_empty()
            && self.add.is_empty()
    }
}

/// De-duplicate a list, keeping the first occurrence of each element (order-stable).
fn dedup(list: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .filter(|x| seen.insert(x.clone()))
        .collect()
}

/// A language's overrides of the global report lists. Each field patches the
/// inherited list from the metric catalog; an empty (no-op) patch leaves the
/// global default untouched.
#[derive(Debug, Clone, Default)]
pub struct ReportOverride {
    /// The node-table column order (`[tableview].columns`).
    pub columns: ListPatch,
    /// The card-featured metrics (`[cardview].featured`).
    pub card: ListPatch,
    /// The JSON report's aggregate `stats` keys.
    pub stats: ListPatch,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn apply_covers_every_op() {
        let base = v(&["kind", "sloc", "hk", "volume", "effort"]);

        // remove (one or many) + add (appended, de-duped against the base)
        let p = ListPatch {
            remove: v(&["volume", "effort"]),
            add: v(&["unsafe", "hk"]), // hk already present → not duplicated
            ..Default::default()
        };
        assert_eq!(p.apply(&base), v(&["kind", "sloc", "hk", "unsafe"]));

        // replace in place (position preserved)
        let p = ListPatch {
            replace: vec![("sloc".into(), "lloc".into())],
            ..Default::default()
        };
        assert_eq!(
            p.apply(&base),
            v(&["kind", "lloc", "hk", "volume", "effort"])
        );

        // clear + add = a fresh list
        let p = ListPatch {
            clear: true,
            add: v(&["kind", "hk"]),
            ..Default::default()
        };
        assert_eq!(p.apply(&base), v(&["kind", "hk"]));

        // after / before insert relative to an anchor (position preserved)
        let p = ListPatch {
            after: vec![("hk".into(), v(&["tsr"]))],
            ..Default::default()
        };
        assert_eq!(
            p.apply(&base),
            v(&["kind", "sloc", "hk", "tsr", "volume", "effort"])
        );
        let p = ListPatch {
            before: vec![("hk".into(), v(&["tsr"]))],
            ..Default::default()
        };
        assert_eq!(
            p.apply(&base),
            v(&["kind", "sloc", "tsr", "hk", "volume", "effort"])
        );

        // prepend goes to the front
        let p = ListPatch {
            prepend: v(&["unsafe"]),
            ..Default::default()
        };
        assert_eq!(
            p.apply(&base),
            v(&["unsafe", "kind", "sloc", "hk", "volume", "effort"])
        );

        // replace_all wins outright (and de-dups)
        let p = ListPatch {
            replace_all: Some(v(&["a", "b", "a"])),
            ..Default::default()
        };
        assert_eq!(p.apply(&base), v(&["a", "b"]));

        // a no-op patch returns the base unchanged
        assert!(ListPatch::default().is_noop());
        assert_eq!(ListPatch::default().apply(&base), base);
    }
}
