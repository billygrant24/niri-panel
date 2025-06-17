#!/usr/bin/env bash

set -e

echo "Building Niri Panel..."

# Initialize flake if not already done
if [ ! -f "flake.lock" ]; then
    echo "Initializing Nix flake..."
    nix flake update
fi

# Enter development shell and build
echo "Entering Nix development environment..."
nix develop -c bash -c "
    echo 'Building with Cargo...'
    cargo build --release
    echo 'Build complete! Binary is at: target/release/niri-panel'
"

echo ""
echo "To run the panel:"
echo "  nix develop -c cargo run"
echo ""
echo "Or run the built binary directly:"
echo "  ./target/release/niri-panel"
echo ""
echo "Run CLI control commands:"
echo "  ./target/release/niri-panel show launcher"
echo "  ./target/release/niri-panel list"
echo "  ./target/release/niri-panel hide sound"
