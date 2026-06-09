mod analyze;
mod check;
mod cli;
mod config;
mod git;
mod logger;
mod pipeline;
mod plugin;
mod presets;
mod recommend;
mod report;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cmd = format!(
        "code-ranker {}",
        std::env::args().skip(1).collect::<Vec<_>>().join(" ")
    );
    // Startup line: the exact command this run was invoked with. The config it
    // resolved is logged next, by `config::load`. The matching `✓ … — <time>`
    // finish line is emitted by this timer.
    logger::info(&format!("▶ {cmd}"));
    let t = logger::Timer::start(&cmd);
    let res = match cli.command {
        Command::Check {
            analyze,
            cycle_rules,
            thresholds,
            baseline,
            output_format,
            top,
            exit_zero,
            suggest_config,
        } => check::run_check(
            &analyze,
            &cycle_rules,
            &thresholds,
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
            output_prompt,
            output_scorecard,
            output_prompt_path,
            output_scorecard_path,
            preset,
            severity,
            top,
            index,
        } => report::run_report(
            &analyze,
            baseline.as_deref(),
            report::ReportOutputs {
                json: output_json,
                html: output_html,
                prompt: output_prompt,
                scorecard: output_scorecard,
                json_path: output_json_path,
                html_path: output_html_path,
                prompt_path: output_prompt_path,
                scorecard_path: output_scorecard_path,
            },
            report::ReportReco {
                preset,
                severity,
                top,
                index,
            },
        ),
    };
    match &res {
        Ok(_) => {
            t.finish();
        }
        Err(e) => logger::info(&format!("error: {e:#}")),
    }
    res
}
