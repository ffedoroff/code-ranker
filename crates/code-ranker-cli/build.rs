//! Embed the `languages/` principle/metric doc corpus into the binary.
//!
//! Walks the repo-root `languages/` tree and generates `$OUT_DIR/corpus.rs` — a
//! `CORPUS: &[(rel_path, contents)]` slice built from `include_str!`, so the tool
//! can serve a principle's Markdown (e.g. `--doc HK`) from the binary itself with
//! no filesystem at runtime. Dependency-free (no `include_dir` crate).

use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let corpus = Path::new(&manifest)
        .join("../../languages")
        .canonicalize()
        .expect("languages/ corpus directory exists");

    // Re-run when the tree changes (added/removed files) and on any file edit.
    println!("cargo:rerun-if-changed={}", corpus.display());

    let mut entries: Vec<(String, PathBuf)> = Vec::new();
    collect(&corpus, &corpus, &mut entries);
    entries.sort();

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
