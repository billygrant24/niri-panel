# Niri-Panel CPU Usage Issues

This document identifies areas of high CPU usage in the niri-panel application and provides recommendations for fixes.

## Issues by Widget

### Network Widget (`src/widgets/network.rs`)

- **High frequency polling**: Uses 500ms polling for network status updates (lines 471-531)
- **Multiple external commands**: Executes numerous external commands with each update cycle
- **Multi-phase updates**: Implements 5 different update phases that run in sequence
- **Frequent subprocess spawning**: Creates many short-lived processes for network information

Problematic code:
```rust
// In network.rs
glib::timeout_add_local(Duration::from_millis(500), move || {
    // Only update if we're not currently updating from UI controls
    if !*volume_updating_for_monitor.borrow() && !*mute_updating_for_monitor.borrow() {
        // Use adaptive update frequency based on popover visibility
        let should_update = if let Some(popover) = popover_weak.upgrade() {
            if popover.is_visible() {
                // Update frequently when popover is visible
                true
            } else {
                // Update less often when popover is hidden
                last_audio_update.borrow().elapsed().as_millis() > 1000
            }
        } else {
            // Default case
            true
        };
        // ...
    }
    glib::ControlFlow::Continue
});
```

### Sound Widget (`src/widgets/sound.rs`)

- **High frequency audio polling**: Updates audio info every 500ms (lines 471-531)
- **Continuous filesystem monitoring**: Polls filesystem every 50ms for changes (lines 597-632)
- **Media info updates**: Updates media playback information every 3 seconds (lines 535-579)
- **Multiple external processes**: Spawns processes for each volume, media and device operation

Problematic code:
```rust
// In sound.rs - Filesystem monitoring loop
glib::timeout_add_local(Duration::from_millis(50), move || {
    // Check if we have any audio updates
    while let Ok(()) = audio_rx.try_recv() {
        // Only update if we're not currently updating from UI controls
        if !*volume_updating2.borrow() && !*mute_updating2.borrow() {
            // ...perform updates...
        }
    }
    glib::ControlFlow::Continue
});
```

### Battery Widget (`src/widgets/battery.rs`)

- **Brightness monitoring**: Checks for file changes every 50ms (lines 449-539)
- **Frequent system stat collection**: Polls CPU and hardware stats every 2 seconds when popover is visible (lines 381-412)
- **Multiple filesystem operations**: Performs numerous file read operations for battery and system stats

Problematic code:
```rust
// In battery.rs - Brightness monitoring
glib::timeout_add_local(Duration::from_millis(50), move || {
    // Check if we have any brightness updates
    while let Ok(brightness) = brightness_rx.try_recv() {
        // Only update if we're not currently updating from the slider
        if !*brightness_updating_for_monitor.borrow() {
            // ...update UI...
        }
    }
    glib::ControlFlow::Continue
});
```

## Recommendations

### General Improvements

1. **Reduce polling frequency**:
   - Increase minimum polling intervals to at least 1-2 seconds for most operations
   - Implement exponential backoff for operations that rarely change

2. **Implement adaptive polling**:
   - Further reduce update frequency when panel is not in focus
   - Use much longer intervals (5-10 seconds) when widgets are not visible

3. **Consolidate external commands**:
   - Combine multiple command executions into single operations where possible
   - Use libraries instead of processes where feasible (e.g., network operations)

4. **Add proper caching**:
   - Cache command results and only update when values change
   - Implement TTL (time-to-live) for cached values to prevent stale data

5. **Replace polling with events**:
   - Use proper event-driven architecture instead of continuous polling
   - Leverage D-Bus signals for system events rather than checking files

### Specific Widget Fixes

#### Network Widget

- Increase polling interval to minimum of 2 seconds
- Use NetworkManager D-Bus API instead of spawning commands
- Implement caching for network information that rarely changes
- Add exponential backoff when network is stable

#### Sound Widget

- Reduce audio update frequency to 1-2 seconds minimum
- Replace filesystem polling with proper inotify or D-Bus events
- Consolidate media player information collection
- Add smart throttling based on audio activity

#### Battery Widget

- Replace filesystem polling with proper inotify events
- Reduce system stats collection to once every 5-10 seconds
- Implement change-based updates rather than time-based polling
- Cache hardware information that doesn't change frequently

## Implementation Priority

1. Network widget (highest CPU impact)
2. Sound widget (medium-high CPU impact)
3. Battery widget (medium CPU impact)