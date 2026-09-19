# bibli task runner. Run `just` to list recipes.

default:
    @just --list

# Build everything in the main workspace.
build:
    cargo build --workspace

# Run all tests.
test:
    cargo nextest run --workspace --no-tests=pass

# Exercise real Poppler with synthetic text, geometry and navigation fixtures.
poppler-check:
    cargo build -p bibli-poppler-worker
    python3 tools/pdf/check.py --engine poppler --worker target/debug/bibli-poppler-worker

# Exercise GStreamer playback using a synthetic two-track video and subtitles.
media-check:
    cargo build -p bibli-media-worker
    python3 tools/media/check.py

# Exercise audio and archive helpers against generated inputs outside the repo.
previews-check:
    cargo build -p bibli-media-worker -p bibli-archive-worker
    python3 tools/previews/check.py --temporary

# Run the vendored Blade atlas regression tests with a real GPU context.
atlas-check:
    python3 tools/gpui/check-atlas.py

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
    cargo build --release -p bibli -p bibli-pdf-worker -p bibli-poppler-worker -p bibli-media-worker -p bibli-archive-worker
    ./target/release/bibli {{dir}}

# Profile the app with samply (opens the Firefox Profiler UI when done).
flame dir=".":
    cargo build --release -p bibli -p bibli-pdf-worker -p bibli-poppler-worker -p bibli-media-worker -p bibli-archive-worker
    samply record ./target/release/bibli {{dir}}

# Profile a bench profile's listing with a flamegraph SVG.
flame-bench profile="flat100k":
    cargo flamegraph --release -p bibli-bench -o flamegraph-{{profile}}.svg -- list {{profile}}

# Watch and re-check on change.
watch:
    bacon clippy
