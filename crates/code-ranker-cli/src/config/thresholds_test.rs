use super::*;

#[test]
fn suffix_quoting_is_scoped_to_thresholds_tables() {
    // A bare-suffixed value outside a thresholds table is NOT touched (it would
    // still be invalid TOML there — we only help where suffixes are meaningful).
    let outside = quote_suffixed_thresholds("[other]\nx = 300K\n");
    assert!(outside.contains("x = 300K"), "untouched outside: {outside}");
    let inside = quote_suffixed_thresholds("[plugins.base.rules.thresholds.file]\nhk = 300K\n");
    assert!(inside.contains("hk = \"300K\""), "quoted inside: {inside}");
    // Already-quoted and plain values are left as-is.
    let q =
        quote_suffixed_thresholds("[plugins.base.rules.thresholds.file]\na = \"5M\"\nb = 200\n");
    assert!(q.contains("a = \"5M\"") && q.contains("b = 200"), "{q}");
}

#[test]
fn quote_skips_malformed_keys_and_values() {
    // Inside a thresholds table, only a clean `key = bare-suffixed-number` is
    // rewritten; every other shape is passed through untouched.
    let out = quote_suffixed_thresholds(
        "[plugins.base.rules.thresholds.file]\n\
         a-b = 300K\n\
         bad key = 300K\n\
         weird = 1a2K\n\
         under = 5_000K\n\
         empty =\n",
    );
    // Hyphenated key and an underscore-grouped body are valid → quoted.
    assert!(out.contains("a-b = \"300K\""), "hyphen key quoted: {out}");
    assert!(
        out.contains("under = \"5_000K\""),
        "underscore body quoted: {out}"
    );
    // Key with a space, a non-numeric body, and an empty value are all left bare.
    assert!(
        out.contains("bad key = 300K"),
        "spaced key untouched: {out}"
    );
    assert!(
        out.contains("weird = 1a2K"),
        "non-numeric body untouched: {out}"
    );
    assert!(out.contains("empty =\n"), "empty value untouched: {out}");
}
