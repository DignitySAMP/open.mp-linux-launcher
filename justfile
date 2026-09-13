set shell := ["sh", "-cu"]

default:
    @just --list

build:
    cargo build --release

# Run every test, including the Wine launch test when Wine is installed.
test:
    cargo test --workspace

# Run the tests without the Wine launch test.
test-fast:
    OMPTUI_WINE_E2E=0 cargo test --workspace

# Only the Windows helper.
injector:
    cd crates/injector && cargo build --release

# Copy the release binary to ~/.local/bin and register the desktop entries.
install: build
    ./target/release/omp-tui --install-desktop

check:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo fmt --all -- --check
    cd crates/injector && cargo clippy --release -- -D warnings

fmt:
    cargo fmt --all
    cd crates/injector && cargo fmt
    cd crates/testpe && cargo fmt
