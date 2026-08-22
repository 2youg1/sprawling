# sprawling daily loop. Contract mirrored in AGENTS.md; keep both in sync.
set shell := ["bash", "-uc"]

default: check

# Every card closes on this being green.
check: fmt-check clippy test gates

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all --check

# --all-features is load-bearing: code behind a feature (runtime/wasm, */conformance)
# escapes the zero-warning gate without it (found at S3.12).
clippy:
    cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

# cargo-nextest is an environment prerequisite (see AGENTS.md); `just test-std` is the fallback.
test:
    cargo nextest run --workspace --locked --all-features

test-std:
    cargo test --workspace --locked

# All machine gates (xtask). cargo-deny runs in CI and here when installed.
gates:
    cargo xtask gates
    @command -v cargo-deny >/dev/null 2>&1 && cargo deny check || echo "cargo-deny not installed locally; CI runs it"

# The client, built without `dx` (crates/web/web-SPEC.md section 8.5).
# Environment prerequisites: the wasm32-unknown-unknown target, and a
# wasm-bindgen CLI whose version equals the wasm-bindgen crate version -
# a mismatch there is the quietest way to break a wasm build.
build-web:
    cargo build -p web --target wasm32-unknown-unknown --release --locked
    wasm-bindgen --target web --no-typescript --out-dir target/web-dist \
        target/wasm32-unknown-unknown/release/web.wasm

# The gate `just check` cannot reach: web's wasm-only paths, and channels
# built without its server feature. Cheap, so it runs on its own.
check-web:
    cargo clippy -p web --target wasm32-unknown-unknown --all-targets --locked -- -D warnings

# citysim scenarios land from S2; the crate's test suite is the entry point.
sim seed="":
    cargo test --package citysim --locked

# Generate or refresh a crate SPEC skeleton (apostle-sdd 17 sections + B.5 amendments).
spec crate:
    cargo xtask spec {{crate}}

# Regenerate public-api baselines (cargo-public-api + nightly are environment prerequisites).
api-baseline:
    cargo xtask apisync --write

# Requires cargo-fuzz + nightly (environment prerequisite; V4 runs nightly in CI).
fuzz target:
    cargo fuzz run {{target}} --fuzz-dir fuzz

# Requires cargo-mutants (environment prerequisite). The threshold lives in
# xtask/budgets.toml; it is enforced here rather than in `just check` because a
# full mutation run is minutes, and a gate nobody waits for is a gate nobody runs.
mutants:
    cargo mutants --package kernel --minimum-test-timeout 60 --error-value 'kernel::AxError::failure(kernel::AxCode::InvalidArgs, "mutant", "mutant")'

# The performance register: every budget, what it costs today, and what is gated.
budget:
    cargo xtask budget

# The three wall-clock budgets, measured on this machine (never gated).
bench:
    cargo run --release -p citysim --bin bench

# CycloneDX bill of materials -> target/sbom.cdx.json (release item two).
sbom:
    cargo xtask sbom

# Two builds of one tree must be byte-identical (release item three).
repro:
    cargo xtask repro

# The whole deliverable: client bundle first, then the binary that embeds
# it, then the size badges README shows. The badges are rendered from the
# artifacts this recipe just produced, so a release cannot ship a size
# somebody typed.
dist: build-web
    cargo build --release -p sprawling --locked
    cargo xtask sbom
    cargo xtask badge --write

# The release archive: the one file a person downloads, unpacks and runs.
# `dist` first, because the archive is assembled out of its artifacts and
# never out of whatever happened to be in target/ from an earlier build.
package: dist
    cargo xtask package

# Offline chain verification (A2); strictly read-only.
replay log:
    cargo run -p sprawling --locked -- replay {{log}}

# A11: one session's resident memory, in this platform's own vocabulary.
# Optional argument: a pid to measure instead of the tool itself.
mem pid="":
    cargo xtask mem {{pid}}
