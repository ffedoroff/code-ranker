// The ONLY mention of the plugins crate in the whole CLI: a link-anchor, not a
// use. The CLI talks to plugins solely through `code_ranker_plugin_api` (the
// trait, `registry()`, `toml_merge`, `list_override`) and never names a language.
// But a binary only links a dependency it references, and the plugins
// self-register via `inventory::submit!` — so without this anchor the crate would
// be dropped and the registry would come up empty. `as _` pulls it in for linking
// while binding no name, so nothing can accidentally reach into it.
extern crate code_ranker_plugins as _;

mod analyze;
mod check;
mod cli;
mod compose;
mod config;
mod docs;
mod export;
mod git;
mod logger;
mod pipeline;
mod plugin;
mod recommend;
mod report;
mod templates;

use clap::Parser;
use code_ranker_plugin_api::log;

use cli::{Cli, Command, OutputMode};

fn main() {
    let cli = Cli::parse();
    // Apply the verbosity before emitting anything: every later line (here, in the
    // stages, and in the plugins) reads this one switch. `--output.mode` is global,
    // so it is honoured wherever it appears on the command line.
    log::set_level(match cli.output_mode {
        OutputMode::Quiet => log::QUIET,
        OutputMode::Summary => log::SUMMARY,
        OutputMode::Verbose => log::VERBOSE,
    });
    let cmd = format!(
        "code-ranker {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );
    // The run skeleton (`▶` startup + `✓ … — <time>` finish) is only meaningful for
    // the analysis commands — `check` / `report` do real work worth timing. `docs`
    // is a plain doc dump to stdout, so it stays quiet (no `▶` / `✓`); errors still
    // surface on every command.
    let timed = matches!(cli.command, Command::Check { .. } | Command::Report { .. });
    // Startup line (verbose only): the exact command this run was invoked with. The
    // config it resolved is logged next, by `config::load`. The matching summary-tier
    // `✓ … — <time>` finish line is emitted by this timer.
    let timer = timed.then(|| {
        logger::verbose(&format!("▶ {cmd}"));
        logger::Timer::start(&cmd)
    });
    let res = match cli.command {
        Command::Check {
            analyze,
            cycle_rules,
            thresholds,
            focus_path,
            focus,
            baseline,
            output_format,
            top,
            exit_zero,
            suggest_config,
        } => check::run_check(
            &analyze,
            &cycle_rules,
            &thresholds,
            &focus_path,
            &focus,
            baseline.as_deref(),
            output_format,
            top,
            exit_zero,
            suggest_config,
        ),
        Command::Report {
            analyze,
            baseline,
            output_json,
            output_html,
            output_json_path,
            output_html_path,
            output_sarif,
            output_sarif_path,
            output_codequality,
            output_codequality_path,
            output_prompt,
            output_scorecard,
            output_prompt_path,
            output_scorecard_path,
            focus,
            language,
            focus_path,
            severity,
            top,
            index,
            export_full_config,
            prompt_id,
        } => match export_full_config {
            // `--export-full-config PATH`: dump the effective config and exit; no analysis.
            Some(path) => export::export_full_config(&analyze, &path),
            None => report::run_report(
                &analyze,
                baseline.as_deref(),
                report::ReportOutputs {
                    json: output_json,
                    html: output_html,
                    sarif: output_sarif,
                    codequality: output_codequality,
                    prompt: output_prompt,
                    scorecard: output_scorecard,
                    json_path: output_json_path,
                    html_path: output_html_path,
                    sarif_path: output_sarif_path,
                    codequality_path: output_codequality_path,
                    prompt_path: output_prompt_path,
                    scorecard_path: output_scorecard_path,
                },
                report::ReportReco {
                    focus,
                    language,
                    focus_path,
                    severity,
                    top,
                    index,
                    prompt_id,
                },
            ),
        },
        // `docs <subject>`: print a reference doc to stdout. No analysis — it builds
        // the principle/metric/category specs from config + plugin and serves the
        // playbook, an index, a category, a metric card, or a principle doc. See
        // `docs.rs`.
        Command::Docs {
            language,
            subject,
            config,
        } => docs::run(language.as_deref(), subject.as_deref(), &config),
    };
    match res {
        Ok(()) => {
            if let Some(t) = timer {
                t.finish();
            }
        }
        // Print the error ourselves (one stamped `error:` line) and exit non-zero.
        // `main` returns `()` rather than `Result` so the runtime does NOT also print
        // its own `Error: …` line — that double-print is exactly what we avoid here.
        Err(e) => {
            logger::error(&format!("error: {e:#}"));
            std::process::exit(1);
        }
    }
}
