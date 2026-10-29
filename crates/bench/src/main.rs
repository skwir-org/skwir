//! `skwir-bench`: generate synthetic trees on demand and measure the raw
//! filesystem operations the core is built on. `check` compares against
//! `budgets.toml` and fails on regression.
//!
//! Trees are generated under `bench-data/` (git-ignored) and never committed.

mod generate;
mod measure;
mod profiles;
mod trigram;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use profiles::Profile;

#[derive(Parser)]
#[command(name = "skwir-bench", version, about)]
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
        /// Measure only these metrics, by their keys in the budgets file
        /// (`delete_tail_us,tags_forget_us`); all when omitted.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
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
    /// Time the Usage view's scan of a generated tree, forced as Refresh
    /// asks it, printing each run's time.
    Usage {
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
        profile: Profile,
    },
    /// Build a trigram index of what a `content:` search under `dir`
    /// reads, and time its answers against the search's own, query by
    /// query (the question of ADR-003). Prints counts and times as JSON
    /// lines, never a path. Cold runs drop `dir`'s files from the page
    /// cache first.
    Trigram {
        dir: PathBuf,
        /// A query as typed in the address field, one `content:` term
        /// (repeatable); a default set when omitted.
        #[arg(long = "query")]
        queries: Vec<String>,
        /// Leave out the cold runs.
        #[arg(long)]
        warm_only: bool,
        /// Leave out the scans: the index's answers, unchecked.
        #[arg(long)]
        no_scan: bool,
        /// Runs of each query, by the index and by the scan, alternated.
        #[arg(long, default_value_t = 3, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
    },
    /// Measure and compare against a budgets file; exit 1 on any regression.
    Check {
        #[arg(long, default_value = "crates/bench/budgets.toml")]
        budgets: PathBuf,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
        /// Measure and check only these metrics, by their keys in the
        /// budgets file; all when omitted.
        #[arg(long, value_delimiter = ',')]
        only: Vec<String>,
    },
}

/// Measure what `only` names of `profiles` in order, then the searches,
/// which would slow what followed them (`measure::tag_search`). A tree is
/// generated only where what is measured reads it.
fn measure_all(
    root: &std::path::Path,
    profiles: &[Profile],
    runs: u16,
    only: &measure::Only,
) -> Result<Vec<(Profile, measure::Report)>> {
    let runs = usize::from(runs);
    let mut reports = Vec::new();
    for &p in profiles {
        if only.reads_tree(p) {
            generate::generate(root, p)?;
        }
        reports.push((p, measure::run(root, p, runs, only)));
    }
    for (p, r) in &mut reports {
        if only.wants("tag_search_us") {
            r.tag_search = measure::tag_search(root, *p, runs);
        }
        if only.wants("tags_check_us") {
            r.tags_check = measure::tags_check(root, *p, runs);
        }
        if [
            "tags_index_learn_us",
            "tags_index_tally_us",
            "tags_index_change_us",
        ]
        .iter()
        .any(|m| only.wants(m))
        {
            (r.tags_index_learn, r.tags_index_tally, r.tags_index_change) =
                measure::tags_index(*p, runs);
        }
    }
    Ok(reports)
}

fn main() -> Result<()> {
    // The allocator the application ships with (ADR-011).
    skwir_alloc::tune();
    let mut cli = Cli::parse();
    // Locations are absolute paths: a relative one is the folder shown's,
    // which the bench has none of.
    cli.root = std::path::absolute(&cli.root)?;
    match cli.cmd {
        Cmd::Gen { profiles } => {
            for p in profiles::or_all(profiles) {
                generate::generate(&cli.root, p)?;
            }
        }
        Cmd::List {
            runs,
            only,
            profiles,
        } => {
            let only = measure::Only::named(only)?;
            let mut all = serde_json::Map::new();
            for (p, r) in measure_all(&cli.root, &profiles::or_all(profiles), runs, &only)? {
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
            let dir = skwir_core::os_path::canonical(dir)?;
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
        Cmd::Usage { runs, profile } => {
            generate::generate(&cli.root, profile)?;
            for (run, took) in measure::usage_scans(&cli.root, profile, usize::from(runs))
                .into_iter()
                .enumerate()
            {
                let us = u64::try_from(took.as_micros()).unwrap_or(u64::MAX);
                println!("{}", serde_json::json!({ "run": run + 1, "us": us }));
            }
        }
        Cmd::Trigram {
            dir,
            queries,
            warm_only,
            no_scan,
            runs,
        } => trigram::run(
            &dir,
            &trigram::Options {
                queries,
                cold: !warm_only,
                scan: !no_scan,
                runs: usize::from(runs),
            },
        )?,
        Cmd::Check {
            budgets,
            runs,
            only,
        } => {
            let only = measure::Only::named(only)?;
            let text = std::fs::read_to_string(&budgets)
                .with_context(|| format!("reading {}", budgets.display()))?;
            let budgets: measure::Budgets = toml::from_str(&text)?;
            let mut failed = false;
            for (p, r) in measure_all(&cli.root, &Profile::ALL, runs, &only)? {
                failed |= !measure::check(p, &r, &budgets, &only);
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
