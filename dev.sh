#!/usr/bin/env bash
# Simplified development helper script

set -e # Exit on error

function show_help() {
  echo "Development Helper Script for intercept-bounce"
  echo "Usage: ./dev.sh [command]"
  echo
  echo "Commands:"
  echo "  fmt       Format code with rustfmt"
  echo "  check     Run cargo check"
  echo "  clippy    Run clippy lints"
  echo "  test      Run tests"
  echo "  docs      Generate documentation"
  echo "  build     Build the project"
  echo "  nix       Build with Nix"
  echo "  all       Run all checks: fmt, clippy, test"
  echo "  clean     Clean build artifacts"
  echo "  help      Show this help message"
  echo
  echo "Examples:"
  echo "  ./dev.sh all     # Run all checks"
  echo "  ./dev.sh build   # Build the project"
}

# Ensure we're in the project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Process command
case "${1:-help}" in
  fmt)
    echo "Formatting code..."
    cargo fmt --all
    ;;
  check)
    echo "Running cargo check..."
    cargo check --all
    ;;
  clippy)
    echo "Running clippy lints..."
    cargo clippy --all-targets -- -D warnings
    ;;
  test)
    echo "Running tests..."
    cargo test --all
    ;;
  docs)
    echo "Generating documentation..."
    cargo run --package xtask --bin xtask -- generate-docs
    ;;
  build)
    echo "Building in release mode..."
    cargo build --release
    ;;
  nix)
    echo "Building with Nix..."
    nix build
    ;;
  all)
    echo "Running all checks..."
    ./dev.sh fmt
    ./dev.sh clippy
    ./dev.sh test
    ;;
  clean)
    echo "Cleaning build artifacts..."
    cargo clean
    ;;
  help|*)
    show_help
    ;;
esac
