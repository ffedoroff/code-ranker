//! Benchmark: syn (cyclomatic only) vs rust-code-analysis (full metrics).
//! Run: cargo run --example bench --release -- <path-to-rust-project>
use std::time::{Duration, Instant};

use rust_code_analysis::{FuncSpace, ParserTrait, RustParser, SpaceKind, metrics};
use syn::visit::{self, Visit};
use syn::{
    Arm, BinOp, ExprBinary, ExprForLoop, ExprIf, ExprLoop, ExprMatch, ExprTry, ExprWhile,
    ImplItemFn, ItemFn,
};
use walkdir::WalkDir;

// ── syn visitor ──────────────────────────────────────────────────────────────

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
    fn visit_expr_match(&mut self, n: &'ast ExprMatch) {
        visit::visit_expr_match(self, n);
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

#[derive(Default)]
struct SynFile {
    fns: Vec<(String, u32)>,
}

impl<'ast> Visit<'ast> for SynFile {
    fn visit_item_fn(&mut self, n: &'ast ItemFn) {
        let mut v = CyclomaticVisitor::new();
        v.visit_block(&n.block);
        self.fns.push((n.sig.ident.to_string(), v.score));
    }
    fn visit_impl_item_fn(&mut self, n: &'ast ImplItemFn) {
        let mut v = CyclomaticVisitor::new();
        v.visit_block(&n.block);
        self.fns.push((n.sig.ident.to_string(), v.score));
    }
}

struct SynResult {
    elapsed: Duration,
    files: usize,
    fns: usize,
    total_cc: u64,
    top: Vec<(String, u32)>,
}

fn run_syn(files: &[walkdir::DirEntry]) -> SynResult {
    let t0 = Instant::now();
    let mut total_fns = 0usize;
    let mut total_cc = 0u64;
    let mut top: Vec<(String, u32)> = Vec::new();

    for entry in files {
        let path = entry.path();
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(ast) = syn::parse_file(&src) else {
            continue;
        };
        let mut collector = SynFile::default();
        collector.visit_file(&ast);
        for (name, cc) in collector.fns {
            total_cc += cc as u64;
            top.push((format!("{}::{}", path.display(), name), cc));
            total_fns += 1;
        }
    }

    let elapsed = t0.elapsed();
    top.sort_by_key(|&(_, score)| std::cmp::Reverse(score));
    SynResult {
        elapsed,
        files: files.len(),
        fns: total_fns,
        total_cc,
        top,
    }
}

// ── rust-code-analysis ────────────────────────────────────────────────────────

struct RcaFn {
    label: String,
    cc: f64,
    cog: f64,
    sloc: f64,
    mi: f64,
    halstead_bugs: f64,
    nexits: f64,
    nargs: f64,
}

struct RcaResult {
    elapsed: Duration,
    files: usize,
    fns: usize,
    total_cc: f64,
    top: Vec<RcaFn>,
}

fn collect_rca_fns(space: &FuncSpace, label_prefix: &str, out: &mut Vec<RcaFn>) {
    if matches!(space.kind, SpaceKind::Function) {
        let name = space.name.as_deref().unwrap_or("?");
        let bare = name.split("::").last().unwrap_or(name);
        let m = &space.metrics;
        out.push(RcaFn {
            label: format!("{}::{}", label_prefix, bare),
            cc: m.cyclomatic.cyclomatic(),
            cog: m.cognitive.cognitive(),
            sloc: m.loc.sloc(),
            mi: m.mi.mi_original(),
            halstead_bugs: m.halstead.bugs(),
            nexits: m.nexits.exit(),
            nargs: m.nargs.fn_args() + m.nargs.closure_args(),
        });
    }
    for child in &space.spaces {
        collect_rca_fns(child, label_prefix, out);
    }
}

fn run_rca(files: &[walkdir::DirEntry]) -> RcaResult {
    let t0 = Instant::now();
    let mut all: Vec<RcaFn> = Vec::new();
    let mut file_count = 0usize;

    for entry in files {
        let path = entry.path();
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let parser = RustParser::new(src.into_bytes(), path, None);
        let Some(space) = metrics(&parser, path) else {
            continue;
        };
        file_count += 1;
        collect_rca_fns(&space, &path.display().to_string(), &mut all);
    }

    let elapsed = t0.elapsed();
    let total_fns = all.len();
    let total_cc: f64 = all.iter().map(|f| f.cc).sum();
    all.sort_by(|a, b| b.cc.partial_cmp(&a.cc).unwrap());
    RcaResult {
        elapsed,
        files: file_count,
        fns: total_fns,
        total_cc,
        top: all,
    }
}

// ── main ─────────────────────────────────────────────────────────────────────

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/path/to/project".into());

    let files: Vec<_> = WalkDir::new(&root)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "rs"))
        .collect();

    println!("Target: {root}");
    println!("Files:  {}\n", files.len());

    // ── syn ──────────────────────────────────────────────────────────────────
    let syn_res = run_syn(&files);

    println!("┌─ syn (cyclomatic only) ────────────────────────────────────────");
    println!("│  {:>4}  function", "CC");
    for (label, cc) in syn_res.top.iter().take(15) {
        println!("│  {:>4}  {}", cc, label);
    }
    println!("│");
    println!(
        "│  Files: {}  Functions: {}  Total CC: {}  Avg CC: {:.2}",
        syn_res.files,
        syn_res.fns,
        syn_res.total_cc,
        syn_res.total_cc as f64 / syn_res.fns.max(1) as f64
    );
    println!("│  Time: {:.1}ms", ms(syn_res.elapsed));
    println!();

    // ── rust-code-analysis ───────────────────────────────────────────────────
    let rca_res = run_rca(&files);

    println!("┌─ rust-code-analysis (full metrics) ───────────────────────────");
    println!(
        "│  {:>4}  {:>4}  {:>4}  {:>5}  {:>6}  {:>4}  {:>4}  function",
        "CC", "COG", "SLOC", "MI", "H.bugs", "exit", "args"
    );
    for f in rca_res.top.iter().take(15) {
        println!(
            "│  {:>4.0}  {:>4.0}  {:>4.0}  {:>5.1}  {:>6.4}  {:>4.0}  {:>4.0}  {}",
            f.cc, f.cog, f.sloc, f.mi, f.halstead_bugs, f.nexits, f.nargs, f.label
        );
    }
    println!("│");
    println!(
        "│  Files: {}  Functions: {}  Total CC: {:.0}  Avg CC: {:.2}",
        rca_res.files,
        rca_res.fns,
        rca_res.total_cc,
        rca_res.total_cc / rca_res.fns.max(1) as f64
    );
    println!("│  Time: {:.1}ms", ms(rca_res.elapsed));
    println!();

    // ── comparison ───────────────────────────────────────────────────────────
    let speedup = ms(rca_res.elapsed) / ms(syn_res.elapsed);
    println!("┌─ Comparison ───────────────────────────────────────────────────");
    println!("│  {:20}  {:>10}  {:>10}", "", "syn", "rca");
    println!(
        "│  {:20}  {:>9.1}ms  {:>9.1}ms  (rca ×{:.1} {})",
        "Time:",
        ms(syn_res.elapsed),
        ms(rca_res.elapsed),
        if speedup >= 1.0 {
            speedup
        } else {
            1.0 / speedup
        },
        if speedup >= 1.0 { "slower" } else { "faster" }
    );
    println!(
        "│  {:20}  {:>10}  {:>10}",
        "Functions:", syn_res.fns, rca_res.fns
    );
    println!(
        "│  {:20}  {:>10}  {:>10.0}",
        "Total CC:", syn_res.total_cc, rca_res.total_cc
    );
    println!(
        "│  {:20}  {:>10.2}  {:>10.2}",
        "Avg CC:",
        syn_res.total_cc as f64 / syn_res.fns.max(1) as f64,
        rca_res.total_cc / rca_res.fns.max(1) as f64
    );
}
