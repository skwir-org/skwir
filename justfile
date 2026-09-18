# bibli task runner. Run `just` to list recipes.

default:
    @just --list

# Build everything in the main workspace.
build:
    cargo build --workspace

# Run all tests.
test:
    cargo nextest run --workspace --no-tests=pass

# Lint: rustfmt check + clippy with warnings as errors.
lint:
    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings

# Format the workspace.
fmt:
    cargo fmt --all

# Generate the synthetic benchmark trees under bench-data/ (idempotent).
bench-gen *profiles:
    cargo run --release -p bibli-bench -- gen {{profiles}}

# Print raw filesystem measurements as JSON.
bench-list *profiles:
    cargo run --release -p bibli-bench -- list {{profiles}}

# Fail on any performance budget regression (what CI runs).
bench-check:
    cargo run --release -p bibli-bench -- check

# Build both toolkit spikes.
spike-build:
    cd spikes && cargo build --release

# Run a spike: `just spike gpui nvidia` or `just spike floem intel`.
spike toolkit gpu="intel" rows="100000":
    ./spikes/run.sh {{toolkit}} {{gpu}} {{rows}}

# Run the app (release) on a directory, default: the current one.
run dir=".":
    cargo run --release -p bibli -- {{dir}}

# Watch and re-check on change.
watch:
    bacon clippy
