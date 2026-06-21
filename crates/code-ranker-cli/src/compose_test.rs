use super::*;

const BASE: &str = "# P — Principle

TL;DR line.

## Alpha

Alpha body.

## Beta

Beta intro.
Bigger picture: the tail.
- one
- two

## References

- Book.
";

#[test]
fn whole_section_includes_and_inline_sections_in_manifest_order() {
    let manifest =
        "<!-- doc:base \"Alpha\" -->\n\n## Mine\n\nMy body.\n\n<!-- doc:base \"References\" -->\n";
    let out = compose(manifest, BASE, "Rust").unwrap();
    assert!(
        out.contains("# P — Principle (in Rust)"),
        "H1 suffixed: {out}"
    );
    assert!(out.contains("TL;DR line."), "base head inherited");
    // Order: Alpha, then the inline Mine, then References.
    let a = out.find("## Alpha").unwrap();
    let m = out.find("## Mine").unwrap();
    let r = out.find("## References").unwrap();
    assert!(a < m && m < r, "manifest order preserved: {out}");
    // Beta is not referenced → absent (manifest is authoritative).
    assert!(
        !out.contains("## Beta"),
        "unreferenced base section dropped: {out}"
    );
    assert!(out.contains("My body."));
}

#[test]
fn manifest_own_head_is_used_verbatim_over_the_base_head() {
    let manifest =
        "# P — Principle (in Rust)\n\nRust-specific TL;DR.\n\n<!-- doc:base \"Alpha\" -->\n";
    let out = compose(manifest, BASE, "Rust").unwrap();
    assert!(out.contains("# P — Principle (in Rust)"), "own H1: {out}");
    assert!(out.contains("Rust-specific TL;DR."), "own preamble kept");
    assert!(
        !out.contains("TL;DR line."),
        "base head NOT used when manifest has its own"
    );
    assert!(out.contains("## Alpha"));
}

#[test]
fn manifest_without_head_inherits_the_base_head() {
    let out = compose("<!-- doc:base \"Alpha\" -->\n", BASE, "Rust").unwrap();
    assert!(
        out.contains("# P — Principle (in Rust)"),
        "base H1 suffixed"
    );
    assert!(out.contains("TL;DR line."), "base preamble inherited");
}

#[test]
fn from_keeps_section_text_from_the_phrase_onward() {
    let out = compose(
        "<!-- doc:base \"Beta\" from \"Bigger picture:\" -->\n",
        BASE,
        "Rust",
    )
    .unwrap();
    assert!(
        out.contains("Bigger picture: the tail."),
        "slice starts at phrase"
    );
    assert!(out.contains("- two"), "runs to section end");
    assert!(
        !out.contains("Beta intro."),
        "text before the phrase dropped: {out}"
    );
    assert!(
        !out.contains("## Beta"),
        "the heading is dropped when slicing"
    );
}

#[test]
fn to_keeps_section_text_up_to_and_including_the_phrase() {
    let out = compose(
        "<!-- doc:base \"Beta\" to \"Beta intro.\" -->\n",
        BASE,
        "Rust",
    )
    .unwrap();
    assert!(
        out.contains("## Beta"),
        "slice from section start keeps the heading"
    );
    assert!(out.contains("Beta intro."), "phrase is inclusive");
    assert!(
        !out.contains("Bigger picture:"),
        "text after the phrase dropped: {out}"
    );
}

#[test]
fn from_and_to_bound_the_slice_on_both_ends() {
    let out = compose(
        "<!-- doc:base \"Beta\" from \"Bigger picture:\" to \"one\" -->\n",
        BASE,
        "Rust",
    )
    .unwrap();
    assert!(out.contains("Bigger picture: the tail."));
    assert!(out.contains("- one"));
    assert!(!out.contains("- two"), "stops at the `to` phrase: {out}");
}

#[test]
fn newline_escape_anchors_to_a_line_start() {
    // `\n` in the phrase matches the line break before `Bigger picture:`.
    let out = compose(
        "<!-- doc:base \"Beta\" from \"\\nBigger picture:\" -->\n",
        BASE,
        "Rust",
    )
    .unwrap();
    assert!(out.contains("Bigger picture: the tail."));
    assert!(!out.contains("Beta intro."));
}

#[test]
fn missing_section_is_a_hard_error() {
    let err = compose("<!-- doc:base \"Nope\" -->\n", BASE, "Rust").unwrap_err();
    assert!(err.to_string().contains("absent from base"), "{err}");
}

#[test]
fn missing_from_phrase_is_a_hard_error() {
    let err = compose("<!-- doc:base \"Beta\" from \"absent\" -->\n", BASE, "Rust").unwrap_err();
    assert!(err.to_string().contains("from phrase not in"), "{err}");
}

#[test]
fn unknown_clause_is_a_hard_error() {
    let err = compose("<!-- doc:base \"Beta\" sideways \"x\" -->\n", BASE, "Rust").unwrap_err();
    assert!(err.to_string().contains("unknown doc:base clause"), "{err}");
}
