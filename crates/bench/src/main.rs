//! `bibli-bench`: generate synthetic trees on demand and measure the raw
//! filesystem operations the core is built on. `check` compares against
//! `budgets.toml` and fails on regression.
//!
//! Trees are generated under `bench-data/` (git-ignored) and never committed.

mod generate;
mod measure;
mod profiles;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use profiles::Profile;

#[derive(Parser)]
#[command(name = "bibli-bench", version, about)]
struct Cli {
    /// Directory holding the generated trees.
    #[arg(long, default_value = "bench-data", global = true)]
    root: PathBuf,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate synthetic trees (idempotent: existing complete trees are kept).
    Gen {
        /// Profiles to generate; all when omitted.
        profiles: Vec<Profile>,
    },
    /// Measure listing and stat throughput, print JSON.
    List {
        /// Number of repetitions; min and median are reported.
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
        /// Profiles to measure; all when omitted.
        profiles: Vec<Profile>,
    },
    /// Time a search of a generated tree, run by run, printing each run's
    /// time and count: the first run after dropping the page cache
    /// (`echo 3 > /proc/sys/vm/drop_caches`, as root) is the cold one.
    Search {
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
        profile: Profile,
        /// The query, as typed in the address field (`tag:bench`).
        expr: String,
    },
    /// Measure and compare against a budgets file; exit 1 on any regression.
    Check {
        #[arg(long, default_value = "crates/bench/budgets.toml")]
        budgets: PathBuf,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
    },
}

/// Measure `profiles` in order, then the searches, which would slow what
/// followed them (`measure::tag_search`).
fn measure_all(
    root: &std::path::Path,
    profiles: &[Profile],
    runs: u16,
) -> Result<Vec<(Profile, measure::Report)>> {
    let mut reports = Vec::new();
    for &p in profiles {
        generate::generate(root, p)?;
        reports.push((p, measure::run(root, p, usize::from(runs))));
    }
    for (p, r) in &mut reports {
        r.tag_search = measure::tag_search(root, *p, usize::from(runs));
        r.tags_check = measure::tags_check(root, *p, usize::from(runs));
        (r.tags_index_learn, r.tags_index_tally, r.tags_index_change) =
            measure::tags_index(*p, usize::from(runs));
    }
    Ok(reports)
}

fn main() -> Result<()> {
    // The allocator the application ships with (ADR-011).
    bibli_alloc::tune();
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Gen { profiles } => {
            for p in profiles::or_all(profiles) {
                generate::generate(&cli.root, p)?;
            }
        }
        Cmd::List { runs, profiles } => {
            let mut all = serde_json::Map::new();
            for (p, r) in measure_all(&cli.root, &profiles::or_all(profiles), runs)? {
                all.insert(p.name().to_owned(), serde_json::to_value(r)?);
            }
            println!("{}", serde_json::to_string_pretty(&all)?);
        }
        Cmd::Search {
            runs,
            profile,
            expr,
        } => {
            let dir = generate::generate(&cli.root, profile)?;
            let dir = bibli_core::os_path::canonical(dir)?;
            let app = measure::tags_app();
            for run in 1..=runs {
                let s = measure::search(&app, &expr, &dir);
                let us = |d: std::time::Duration| u64::try_from(d.as_micros()).unwrap_or(u64::MAX);
                println!(
                    "{}",
                    serde_json::json!({
                        "run": run,
                        "first_us": s.first.map(us),
                        "complete_us": us(s.complete),
                        "found": s.found,
                    })
                );
            }
        }
        Cmd::Check { budgets, runs } => {
            let text = std::fs::read_to_string(&budgets)
                .with_context(|| format!("reading {}", budgets.display()))?;
            let budgets: measure::Budgets = toml::from_str(&text)?;
            let mut failed = false;
            for (p, r) in measure_all(&cli.root, &Profile::ALL, runs)? {
                failed |= !measure::check(p, &r, &budgets);
            }
            // The budgets are Linux's, measured on its reference machine:
            // elsewhere they are reported, not enforced, until 5b sets that
            // system's own (ADR-038).
            if failed && cfg!(target_os = "linux") {
                eprintln!("bench: BUDGET REGRESSION");
                std::process::exit(1);
            }
            if failed {
                eprintln!("bench: over Linux's budgets, reported only on this system (ADR-038)");
            } else {
                eprintln!("bench: all budgets met");
            }
        }
    }
    Ok(())
}
