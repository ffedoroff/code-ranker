/// Quick benchmark: measure cyclomatic complexity via syn::Visit on a real project.
/// Run: cargo run --example bench_complexity --release -- <path-to-rust-project>
use std::time::Instant;
use syn::{
    Arm, BinOp, ExprBinary, ExprForLoop, ExprIf, ExprLoop, ExprTry, ExprWhile, ImplItemFn, ItemFn,
    visit::{self, Visit},
};

// ---------------------------------------------------------------------------
// Visitor
// ---------------------------------------------------------------------------

struct CyclomaticVisitor {
    score: u32,
}

impl CyclomaticVisitor {
    fn new() -> Self {
        Self { score: 1 }
    }
}

impl<'ast> Visit<'ast> for CyclomaticVisitor {
    fn visit_expr_if(&mut self, n: &'ast ExprIf) {
        self.score += 1;
        visit::visit_expr_if(self, n);
    }
    fn visit_expr_while(&mut self, n: &'ast ExprWhile) {
        self.score += 1;
        visit::visit_expr_while(self, n);
    }
    fn visit_expr_for_loop(&mut self, n: &'ast ExprForLoop) {
        self.score += 1;
        visit::visit_expr_for_loop(self, n);
    }
    fn visit_expr_loop(&mut self, n: &'ast ExprLoop) {
        self.score += 1;
        visit::visit_expr_loop(self, n);
    }
    fn visit_arm(&mut self, n: &'ast Arm) {
        self.score += 1;
        visit::visit_arm(self, n);
    }
    fn visit_expr_binary(&mut self, n: &'ast ExprBinary) {
        if matches!(n.op, BinOp::And(_) | BinOp::Or(_)) {
            self.score += 1;
        }
        visit::visit_expr_binary(self, n);
    }
    fn visit_expr_try(&mut self, n: &'ast ExprTry) {
        self.score += 1;
        visit::visit_expr_try(self, n);
    }
}

fn cyclomatic(body: &syn::Block) -> u32 {
    let mut v = CyclomaticVisitor::new();
    v.visit_block(body);
    v.score
}

// ---------------------------------------------------------------------------
// File-level collector
// ---------------------------------------------------------------------------

#[derive(Default)]
struct FileStats {
    functions: Vec<(String, u32)>,
}

impl<'ast> Visit<'ast> for FileStats {
    fn visit_item_fn(&mut self, n: &'ast ItemFn) {
        let name = n.sig.ident.to_string();
        let score = cyclomatic(&n.block);
        self.functions.push((name, score));
        // do NOT recurse into nested functions to keep scores independent
    }
    fn visit_impl_item_fn(&mut self, n: &'ast ImplItemFn) {
        let name = n.sig.ident.to_string();
        let score = cyclomatic(&n.block);
        self.functions.push((name, score));
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/path/to/project".into());

    let files: Vec<_> = walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .collect();

    println!("Scanning {} .rs files in {root}", files.len());

    let t0 = Instant::now();
    let mut total_fns = 0usize;
    let mut total_complexity = 0u32;
    let mut top: Vec<(String, u32)> = Vec::new();

    for entry in &files {
        let path = entry.path();
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(ast) = syn::parse_file(&src) else {
            continue;
        };

        let mut stats = FileStats::default();
        stats.visit_file(&ast);

        for (name, score) in &stats.functions {
            total_complexity += score;
            top.push((format!("{}::{}", path.display(), name), *score));
        }
        total_fns += stats.functions.len();
    }

    let elapsed = t0.elapsed();

    // top 10 most complex
    top.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    println!("\nTop 10 most complex functions:");
    for (name, score) in top.iter().take(10) {
        println!("  {:3}  {}", score, name);
    }

    println!("\n--- Summary ---");
    println!("Files:              {}", files.len());
    println!("Functions/methods:  {total_fns}");
    println!("Total complexity:   {total_complexity}");
    println!(
        "Avg per function:   {:.1}",
        total_complexity as f64 / total_fns.max(1) as f64
    );
    println!(
        "Time:               {:.1}ms",
        elapsed.as_secs_f64() * 1000.0
    );
}
