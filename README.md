# Niri Panel

A GTK4-based panel for the Niri Wayland compositor, providing system status and control widgets.

## Features

- System status widgets (battery, network, sound, bluetooth)
- Application launcher
- Workspace switcher
- Clock with calendar
- Places (quick access to file locations)
- Sound control with media player support
- Power controls
- Search functionality

## Building and Installation

### Build with Nix (recommended)

```bash
./build.sh
```

### Build with Cargo

```bash
cargo build --release
```

## Running

```bash
# Run with Nix
nix develop -c cargo run

# Run directly
./target/release/niri-panel
```

## Configuration

Configuration is stored in `~/.config/niri-panel/config.toml`. The panel will create a default configuration file if none exists.

## CLI Control

Niri Panel provides a command-line interface to control widget popovers. This allows integration with Niri, Sway, or other window managers.

### Show a widget popover

```bash
niri-panel-ctrl show launcher   # Show the application launcher
niri-panel-ctrl show sound      # Show the sound control panel
niri-panel-ctrl show bluetooth  # Show the bluetooth panel
```

### Available widgets

- `launcher` - Application launcher
- `places` - File location shortcuts
- `search` - Search functionality
- `git` - Git repository tools
- `secrets` - Password manager
- `sound` - Sound and media controls
- `bluetooth` - Bluetooth device management
- `network` - Network management
- `battery` - Battery status
- `clock` - Clock and calendar
- `power` - Power controls (logout, shutdown, etc.)

### List available widgets

```bash
niri-panel-ctrl list
```

### Integration with Niri

Add keybindings to your Niri config.toml:

```toml
[bindings]
"super+a" = "exec niri-panel-ctrl show launcher"
"super+s" = "exec niri-panel-ctrl show sound"
```

### Integration with Sway

Add keybindings to your Sway config:

```
bindsym $mod+a exec niri-panel-ctrl show launcher
bindsym $mod+s exec niri-panel-ctrl show sound
```