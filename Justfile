# osx-clnr task runner — praxis house standard (justfile is canonical; no Makefile).
# `just --list` shows all recipes.

default: ci

build:
    cargo build

release:
    cargo build --release

install: release
    cargo install --path .

test:
    cargo test

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --all-targets -- -D warnings

lint: clippy

deny:
    cargo deny check

typos:
    typos

doc:
    cargo doc --workspace --no-deps

# Full local pipeline: fmt → clippy → test → doctor checks (scripts/sanity.sh)
sanity:
    ./scripts/sanity.sh

clean:
    cargo clean

ci: fmt-check clippy test
