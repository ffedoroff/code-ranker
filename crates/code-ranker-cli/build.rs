//! Embed the `plugins/` principle/metric doc corpus into the binary.
//!
//! Walks the repo-root `plugins/` tree and generates `$OUT_DIR/corpus.rs` — a
//! `CORPUS: &[(rel_path, contents)]` slice built from `include_str!`, so the tool
//! can serve a principle's Markdown (e.g. `--doc HK`) from the binary itself with
//! no filesystem at runtime. Dependency-free (no `include_dir` crate).
//!
//! The single source of truth lives at the repo root (`../../plugins`), OUTSIDE
//! this crate. So that `cargo install code-ranker` from crates.io still embeds the
//! corpus, the publish workflow copies that tree into a package-local `plugins/`
//! right before `cargo publish` (mirroring the per-crate README copy) — and this
//! build script prefers that package-local copy, falling back to the repo-root tree
//! for workspace/dev builds. If NEITHER exists (an unexpected isolated build) the
//! corpus resolves best-effort to EMPTY (never a build failure); `--doc` then reports
//! "not embedded" while everything else works.

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    // Prefer the package-local copy (present in the published tarball), else the
    // repo-root tree (workspace/dev builds). Best-effort: a missing corpus is NOT
    // an error — it must never break `cargo publish`/`cargo install`. See module docs.
    let local = Path::new(&manifest).join("plugins");
    let root = Path::new(&manifest).join("../../plugins");
    let resolved = local.canonicalize().or_else(|_| root.canonicalize());
    match resolved {
        Ok(corpus) => {
            // Re-run when the tree changes (added/removed files) and on any file edit.
            println!("cargo:rerun-if-changed={}", corpus.display());
            collect(&corpus, &corpus, &mut entries);
            entries.sort();
        }
        Err(_) => {
            println!(
                "cargo:warning=plugins/ corpus not found at ./plugins or ../../plugins \
                 — embedding an empty corpus; `--doc` will report \"not embedded\". Published \
                 builds carry a package-local copy (see crates-io.yml); workspace builds use the \
                 repo-root tree."
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
