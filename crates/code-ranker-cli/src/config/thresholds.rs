//! Threshold-literal pre-quoting: a text pass run on a `code-ranker.toml` before
//! the TOML parser sees it, so a bare `K`/`M`/`G`-suffixed threshold (`hk = 300K`)
//! is accepted. Separate from the data model ([`super::model`]) and the
//! value-parsing ([`super::model::parse_number`]) — this is purely a source-text
//! rewrite, depending on nothing but `std`.

/// TOML rejects a bare `300K` (a `K`/`M`/`G` suffix makes it neither a number nor
/// a string), so without help a user must write `hk = "300K"`. This pre-pass lets
/// them write `hk = 300K` by quoting bare suffixed numbers **only inside a
/// `*thresholds*` table**, before the text reaches the TOML parser. Plain and
/// underscored integers stay native; already-quoted values and everything outside
/// a thresholds table are left untouched. The matching CLI form (`--threshold
/// file.hk=300K`) needs no help — it goes straight through
/// [`parse_number`](super::model::parse_number).
pub(crate) fn quote_suffixed_thresholds(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut in_thresholds = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            // Section header (`[t]` or `[[t]]`): a thresholds table enables quoting.
            let name = trimmed.trim_start_matches('[');
            in_thresholds = name
                .split(']')
                .next()
                .is_some_and(|s| s.contains("thresholds"));
        } else if in_thresholds && let Some(quoted) = quote_suffixed_value_line(line) {
            out.push_str(&quoted);
            out.push('\n');
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// If `line` is a `key = <bare-suffixed-number>` assignment, return it with the
/// value quoted (formatting and any trailing comment preserved); else `None`.
fn quote_suffixed_value_line(line: &str) -> Option<String> {
    let eq = line.find('=')?;
    let key = line[..eq].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let after = &line[eq + 1..];
    let (val_seg, comment) = match after.find('#') {
        Some(h) => after.split_at(h),
        None => (after, ""),
    };
    if !is_bare_suffixed_number(val_seg.trim()) {
        return None;
    }
    let lead: String = val_seg.chars().take_while(|c| c.is_whitespace()).collect();
    let trail: String = val_seg
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .collect();
    Some(format!(
        "{}={lead}\"{}\"{trail}{comment}",
        &line[..eq],
        val_seg.trim()
    ))
}

/// Does `v` look like a bare `K`/`M`/`G`-suffixed number (`300K`, `1.5M`,
/// `5_000K`)? Already-quoted values and plain numbers return `false`.
fn is_bare_suffixed_number(v: &str) -> bool {
    let Some(last) = v.chars().last() else {
        return false;
    };
    if !matches!(last, 'k' | 'K' | 'm' | 'M' | 'g' | 'G') {
        return false;
    }
    let body = &v[..v.len() - 1];
    let mut seen_digit = false;
    let mut seen_dot = false;
    for c in body.chars() {
        match c {
            '0'..='9' => seen_digit = true,
            '_' => {}
            '.' if !seen_dot => seen_dot = true,
            _ => return false,
        }
    }
    seen_digit
}

#[cfg(test)]
#[path = "thresholds_test.rs"]
mod tests;
