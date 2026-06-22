//! Embed the `languages/` principle/metric doc corpus into the binary.
//!
//! Walks the repo-root `languages/` tree and generates `$OUT_DIR/corpus.rs` — a
//! `CORPUS: &[(rel_path, contents)]` slice built from `include_str!`, so the tool
//! can serve a principle's Markdown (e.g. `--doc HK`) from the binary itself with
//! no filesystem at runtime. Dependency-free (no `include_dir` crate).
//!
//! The corpus lives at the repo root (`../../languages`), OUTSIDE this crate, so it
//! is NOT in the published crate tarball. A workspace build (the prebuilt binaries
//! shipped via the installer / npm / PyPI / Docker / GitHub Release) finds it and
//! embeds the full corpus; an ISOLATED build (`cargo publish` verify, or
//! `cargo install code-ranker` from crates.io source) won't — so the corpus is
//! resolved best-effort and absence yields an EMPTY corpus (never a build failure).
//! `--doc` then reports "not embedded" on such builds; everything else works.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    // Best-effort: a missing corpus (isolated/published build) is NOT an error —
    // it must never break `cargo publish`/`cargo install`. See module docs.
    match Path::new(&manifest).join("../../languages").canonicalize() {
        Ok(corpus) => {
            // Re-run when the tree changes (added/removed files) and on any file edit.
            println!("cargo:rerun-if-changed={}", corpus.display());
            collect(&corpus, &corpus, &mut entries);
            entries.sort();
        }
        Err(_) => {
            println!(
                "cargo:warning=languages/ corpus not found (isolated build, e.g. \
                 `cargo install code-ranker` from crates.io) — embedding an empty corpus; \
                 `--doc` will report \"not embedded\". Prebuilt binaries embed the full corpus."
            );
        }
    }

    let mut code = String::from(
        "/// Embedded doc corpus: (`<lang>/<ID>.md` relative path, file contents).\n\
         pub static CORPUS: &[(&str, &str)] = &[\n",
    );
    for (rel, abs) in &entries {
        println!("cargo:rerun-if-changed={}", abs.display());
        code.push_str(&format!(
            "    ({rel:?}, include_str!({:?})),\n",
            abs.display().to_string()
        ));
    }
    code.push_str("];\n");

    let out = Path::new(&env::var("OUT_DIR").expect("OUT_DIR")).join("corpus.rs");
    fs::write(&out, code).expect("write corpus.rs");
}

/// Collect every `*.md` under `dir`, keyed by its `/`-joined path relative to `root`.
fn collect(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
            let rel = path
                .strip_prefix(root)
                .expect("under corpus root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            out.push((rel, path));
        }
    }
}
