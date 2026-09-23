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
    /// Measure and compare against a budgets file; exit 1 on any regression.
    Check {
        #[arg(long, default_value = "crates/bench/budgets.toml")]
        budgets: PathBuf,
        #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u16).range(1..))]
        runs: u16,
    },
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
            for p in profiles::or_all(profiles) {
                generate::generate(&cli.root, p)?;
                let r = measure::run(&cli.root, p, usize::from(runs));
                all.insert(p.name().to_owned(), serde_json::to_value(r)?);
            }
            println!("{}", serde_json::to_string_pretty(&all)?);
        }
        Cmd::Check { budgets, runs } => {
            let text = std::fs::read_to_string(&budgets)
                .with_context(|| format!("reading {}", budgets.display()))?;
            let budgets: measure::Budgets = toml::from_str(&text)?;
            let mut failed = false;
            for p in Profile::ALL {
                generate::generate(&cli.root, p)?;
                let r = measure::run(&cli.root, p, usize::from(runs));
                failed |= !measure::check(p, &r, &budgets);
            }
            if failed {
                eprintln!("bench: BUDGET REGRESSION");
                std::process::exit(1);
            }
            eprintln!("bench: all budgets met");
        }
    }
    Ok(())
}
