use rust_code_analysis::{ParserTrait, RustParser, metrics};
use std::path::Path;

fn dump(space: &rust_code_analysis::FuncSpace, depth: usize) {
    let indent = "  ".repeat(depth);
    println!(
        "{indent}[{:?}] name={:?} lines {}-{} children={}",
        space.kind,
        space.name,
        space.start_line,
        space.end_line,
        space.spaces.len()
    );
    for child in &space.spaces {
        dump(child, depth + 1);
    }
}

fn main() {
    let path_str = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "crates/code-split-complexity/src/lib.rs".to_string());
    let path = Path::new(&path_str);
    let src = std::fs::read_to_string(path).expect("read file");

    // Print what tree-sitter sees for a simple fn
    let simple = "fn foo() { if true { 1 } else { 2 } }";
    let parser2 = RustParser::new(simple.as_bytes().to_vec(), Path::new("x.rs"), None);
    println!("=== simple src, path=x.rs ===");
    match metrics(&parser2, Path::new("x.rs")) {
        Some(s) => {
            println!("got root, children={}", s.spaces.len());
            dump(&s, 0);
        }
        None => println!("None"),
    }

    println!("\n=== real file ===");
    let parser = RustParser::new(src.into_bytes(), path, None);
    match metrics(&parser, path) {
        Some(s) => {
            println!("got root, children={}", s.spaces.len());
            dump(&s, 0);
        }
        None => println!("None"),
    }
}
