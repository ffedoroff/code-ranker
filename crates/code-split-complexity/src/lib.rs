use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use code_split_core::{Complexity, GraphBuilder, Halstead, Loc, Maintainability, NodeKind};
use rust_code_analysis::{
    FuncSpace, JavascriptParser, ParserTrait, PythonParser, RustParser, TsxParser,
    TypescriptParser, metrics,
};
use walkdir::WalkDir;

/// Walk all source files under `root` whose extension is in `extensions`,
/// compute complexity metrics via rust-code-analysis, and annotate the
/// file-level nodes in the graph (`File` nodes, and — before the Rust
/// module→file collapse — file-backed `Module` nodes with `line == None`).
/// Returns the number of nodes annotated.
pub fn analyze(root: &Path, builder: &mut GraphBuilder) -> Result<usize> {
    analyze_extensions(root, builder, &["rs"])
}

/// Same as `analyze` but for Python source files.
pub fn analyze_python(root: &Path, builder: &mut GraphBuilder) -> Result<usize> {
    analyze_extensions(root, builder, &["py"])
}

/// Same as `analyze` but for JavaScript / TypeScript source files.
pub fn analyze_js(root: &Path, builder: &mut GraphBuilder) -> Result<usize> {
    analyze_extensions(root, builder, &["js", "jsx", "ts", "tsx"])
}

fn analyze_extensions(
    root: &Path,
    builder: &mut GraphBuilder,
    extensions: &[&str],
) -> Result<usize> {
    let mut file_index: HashMap<String, usize> = HashMap::new();

    for (i, node) in builder.nodes().iter().enumerate() {
        match node.kind {
            // `File` nodes (Python/JS) and file-backed `Module` nodes (Rust,
            // `line == None`) both represent a whole source file. Inline modules
            // (`line.is_some()`) share the enclosing file's path and must not
            // receive file-level metrics.
            NodeKind::File => {
                file_index.insert(node.path.clone(), i);
            }
            NodeKind::Module if node.line.is_none() => {
                file_index.entry(node.path.clone()).or_insert(i);
            }
            _ => {}
        }
    }

    let mut annotated = 0usize;

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().is_file()
                && e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .is_some_and(|x| extensions.contains(&x))
        })
    {
        let path = entry.path();
        let Ok(src) = std::fs::read(path) else {
            continue;
        };
        let canonical = path.to_string_lossy().into_owned();

        let Some(space) = parse_metrics(path, src) else {
            continue;
        };

        if let Some(&idx) = file_index.get(&canonical) {
            builder.nodes_mut()[idx].complexity = Some(complexity_from(&space));
            annotated += 1;
        }
    }

    Ok(annotated)
}

fn parse_metrics(path: &Path, src: Vec<u8>) -> Option<FuncSpace> {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => metrics(&RustParser::new(src, path, None), path),
        Some("py") => metrics(&PythonParser::new(src, path, None), path),
        Some("js") | Some("jsx") => metrics(&JavascriptParser::new(src, path, None), path),
        Some("ts") => metrics(&TypescriptParser::new(src, path, None), path),
        Some("tsx") => metrics(&TsxParser::new(src, path, None), path),
        _ => None,
    }
}

fn complexity_from(s: &FuncSpace) -> Complexity {
    let m = &s.metrics;
    let sloc = m.loc.sloc();
    let vol = m.halstead.volume();

    Complexity {
        cyclomatic: m.cyclomatic.cyclomatic(),
        cognitive: m.cognitive.cognitive(),
        exits: m.nexits.exit(),
        // fn_args > 0 → args = fn_args; otherwise use closure_args
        args: if m.nargs.fn_args() > 0.0 {
            m.nargs.fn_args()
        } else {
            m.nargs.closure_args()
        },
        functions: m.nom.functions(),
        closures: m.nom.closures(),
        coupling: None, // filled later in annotate_hk
        maintainability: Some(Maintainability {
            mi: m.mi.mi_original(),
            mi_sei: m.mi.mi_sei(),
        }),
        loc: if sloc > 0.0 {
            Some(Loc {
                source: sloc,
                logical: m.loc.lloc(),
                comments: m.loc.cloc(),
                blank: m.loc.blank(),
            })
        } else {
            None
        },
        halstead: if vol > 0.0 {
            Some(Halstead {
                length: m.halstead.length(),
                vocabulary: (m.halstead.u_operators() + m.halstead.u_operands()),
                volume: vol,
                effort: m.halstead.effort(),
                time: m.halstead.time(),
                bugs: m.halstead.bugs(),
            })
        } else {
            None
        },
    }
}
