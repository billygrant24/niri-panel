# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Niri-panel is a GTK4-based panel for the Niri Wayland compositor, providing system status and control widgets. It's written in Rust and uses GTK4 with the layer-shell extension to create a desktop panel that sits at the top of the screen.

## Build and Run Commands

### Building the Project

```bash
# Build the project using Nix (recommended)
./build.sh

# Build with Cargo directly
cargo build --release

# Build in debug mode
cargo build
```

### Running the Panel

```bash
# Run using Nix
nix develop -c cargo run

# Run the built binary directly
./target/release/niri-panel

# Run in debug mode
cargo run
```

### Development Commands

```bash
# Check for compilation errors without building
cargo check

# Run formatting checks
cargo fmt -- --check

# Format code
cargo fmt

# Run linting checks
cargo clippy

# Run with automatic reloading on file changes (requires cargo-watch)
cargo watch -x run
```

## Project Architecture

### Core Components

1. **Panel Container (`src/panel.rs`)**: Main container that organizes widgets in left, center, and right sections.

2. **Configuration (`src/config.rs`)**: Handles loading and saving panel configuration from `~/.config/niri-panel/config.toml`.

3. **Widgets (`src/widgets/`)**: Individual UI components for system status and controls:
   - `battery.rs`: Battery status and power management
   - `bluetooth.rs`: Bluetooth device management
   - `clock.rs`: Time display with calendar popover
   - `keyboard_mode.rs`: Keyboard input mode management
   - `launcher.rs`: Application launcher
   - `network.rs`: Network status and control
   - `overview.rs`: System overview
   - `places.rs`: Quick access to file locations
   - `power.rs`: Power controls (shutdown, restart, etc.)
   - `search.rs`: File and application search
   - `secrets.rs`: Password and secret management
   - `sound.rs`: Volume control
   - `workspaces.rs`: Workspace switcher

### Key Concepts

1. **GTK4 Layer Shell**: The panel uses `gtk4-layer-shell` to create a panel that sits at the top layer of the Wayland compositor.

2. **Popovers and Keyboard Management**: Widgets that need keyboard input (like search) use a keyboard mode management system to ensure the panel can capture keyboard events when needed.

3. **External Commands**: Many widgets execute external system commands (e.g., `bluetoothctl` for Bluetooth management) to interact with system services.

4. **Event-Driven Updates**: Widgets use periodic updates and event handlers to keep system status current.

### Development Guidelines

1. **Widget Pattern**: Follow the existing widget pattern:
   - Create a struct with a GTK widget
   - Implement `new()` function that sets up the widget and event handlers
   - Implement `widget()` function to return the GTK widget

2. **Style Consistency**: Use the existing CSS classes in `assets/style.css` for consistent styling.

3. **Error Handling**: Use `anyhow::Result` for error propagation and the `tracing` crate for logging.

4. **Configuration Integration**: New features should be configurable through the `PanelConfig` struct.