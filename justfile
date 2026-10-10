# SuperDeck task runner. Run `just` to list recipes.
# Every recipe wraps plain cargo/pnpm commands, so `just` is optional.

set shell := ["bash", "-euo", "pipefail", "-c"]

# List available recipes
default:
    @just --list

# Install JS dependencies
setup:
    pnpm install

# Format all code
fmt: (_cargo "fmt --all")
    pnpm format

# Check formatting without changing files
fmt-check: (_cargo "fmt --all --check")
    pnpm format:check

# Lint Rust (clippy, warnings are errors) and JS/TS (ESLint)
lint: (_cargo "clippy --workspace --all-targets -- -D warnings")
    pnpm lint

# Type-check TypeScript packages
typecheck:
    pnpm typecheck

# Run all tests
test: (_cargo "test --workspace")
    pnpm test

# Everything CI runs
check: fmt-check lint typecheck test

# Audit Rust dependencies for license and security problems
deny: (_cargo "deny check")

# Build everything
build: (_cargo "build --workspace")
    pnpm build

# Run a cargo command, skipping it while the workspace has no crates yet
[private]
_cargo args:
    @if cargo metadata --no-deps --format-version 1 | grep -q '^{"packages":\[\]'; then \
        echo "skipping 'cargo {{ args }}': no Rust crates yet"; \
    else \
        cargo {{ args }}; \
    fi
