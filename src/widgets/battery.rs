use anyhow::Result;
use glib::timeout_add_seconds_local;
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Image, Label, ListBox, ListBoxRow, Orientation, Popover, Scale,
};
use gtk4_layer_shell::LayerShell;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{info, warn};

use crate::widgets::Widget as WidgetTrait;

pub struct Battery {
    button: Button,
    popover: Popover,
}

#[derive(Debug)]
struct BatteryInfo {
    percentage: i32,
    state: BatteryState,
    time_remaining: Option<String>,
    power_draw: Option<f64>,
    battery_type: BatteryType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BatteryState {
    Charging,
    Discharging,
    Full,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum BatteryType {
    Battery,
    UPS,
    Unknown,
}

impl Default for BatteryInfo {
    fn default() -> Self {
        Self {
            percentage: 0,
            state: BatteryState::Unknown,
            time_remaining: None,
            power_draw: None,
            battery_type: BatteryType::Unknown,
        }
    }
}

impl Battery {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("battery");

        // Create a container for battery icon and percentage
        let container = Box::new(Orientation::Horizontal, 5);

        // Battery icon
        let icon = Image::new();

        // Battery percentage
        let label = Label::new(None);
        label.add_css_class("battery-percentage");

        container.append(&icon);
        container.append(&label);
        button.set_child(Some(&container));

        // Create popover for battery details
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("battery-popover");
        popover.set_autohide(true);

        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!(
                    "Battery popover shown - keyboard mode set to OnDemand (active popovers: {})",
                    *active_popovers_show.borrow()
                );
            }
        });

        // Handle popover hide event - disable keyboard mode if no other popovers
        let window_weak_hide = window_weak.clone();
        let active_popovers_hide = active_popovers.clone();
        popover.connect_hide(move |_| {
            *active_popovers_hide.borrow_mut() -= 1;
            let count = *active_popovers_hide.borrow();
            if count == 0 {
                if let Some(window) = window_weak_hide.upgrade() {
                    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
                    info!("Battery popover hidden - keyboard mode set to None");
                }
            } else {
                info!(
                    "Battery popover hidden - keeping keyboard mode (active popovers: {})",
                    count
                );
            }
        });

        let popover_box = Box::new(Orientation::Vertical, 15);
        popover_box.set_margin_top(15);
        popover_box.set_margin_bottom(15);
        popover_box.set_margin_start(15);
        popover_box.set_margin_end(15);
        popover_box.set_size_request(280, -1);

        // Battery info box
        let info_box = Box::new(Orientation::Vertical, 10);
        info_box.add_css_class("battery-info");

        // Status label
        let status_label = Label::new(Some("Checking battery..."));
        status_label.set_halign(gtk4::Align::Start);
        status_label.add_css_class("battery-status");
        info_box.append(&status_label);

        // Battery level bar
        let level_box = Box::new(Orientation::Horizontal, 10);
        level_box.set_margin_top(5);
        level_box.set_margin_bottom(5);

        let level_icon = Image::new();
        level_icon.set_from_icon_name(Some("battery-level-60-symbolic"));
        level_box.append(&level_icon);

        let level_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        level_scale.set_hexpand(true);
        level_scale.set_draw_value(false);
        level_scale.set_sensitive(false); // Non-interactive
        level_scale.add_css_class("battery-level-scale");
        level_box.append(&level_scale);

        let level_percentage = Label::new(Some("?%"));
        level_percentage.set_width_chars(4);
        level_percentage.add_css_class("battery-level-percentage");
        level_box.append(&level_percentage);

        info_box.append(&level_box);

        // Additional battery stats
        let stats_box = Box::new(Orientation::Vertical, 10);
        stats_box.set_margin_top(5);
        stats_box.add_css_class("battery-stats");

        // Time remaining
        let time_box = Self::create_stat_label("Time Remaining:", "Unknown");
        stats_box.append(&time_box);

        // Power draw
        let power_box = Self::create_stat_label("Power Draw:", "Unknown");
        stats_box.append(&power_box);

        // Type
        let type_box = Self::create_stat_label("Type:", "Unknown");
        stats_box.append(&type_box);

        info_box.append(&stats_box);

        // Power settings button
        let settings_button = Button::with_label("Power Settings");
        settings_button.set_margin_top(10);
        settings_button.connect_clicked(|_| {
            Self::open_power_settings();
        });
        info_box.append(&settings_button);

        popover_box.append(&info_box);
        popover.set_child(Some(&popover_box));

        // Add Escape key handler to close popover
        let escape_controller = gtk4::EventControllerKey::new();
        let popover_weak_escape = popover.downgrade();
        escape_controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(popover) = popover_weak_escape.upgrade() {
                    popover.popdown();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        popover.add_controller(escape_controller);

        // Show popover on click
        let popover_ref = popover.clone();
        button.connect_clicked(move |_| {
            popover_ref.popup();
        });

        // Initialize with unknown state
        icon.set_from_icon_name(Some("battery-missing-symbolic"));
        label.set_text("?%");

        // Attempt to update the battery info immediately
        Self::update_battery_info(&container, &icon, &label, &status_label, &level_icon, &level_scale, &level_percentage);

        // Start periodic updates (every 30 seconds)
        let container_clone = container.clone();
        let icon_clone = icon.clone();
        let label_clone = label.clone();
        let status_label_clone = status_label.clone();
        let level_icon_clone = level_icon.clone();
        let level_scale_clone = level_scale.clone();
        let level_percentage_clone = level_percentage.clone();

        // Extract stat labels for updating - manually find the value label (second child)
        let time_label = if let Some(label) = time_box.last_child() {
            if let Some(label_widget) = label.downcast_ref::<Label>() {
                Some(label_widget.clone())
            } else {
                None
            }
        } else {
            None
        };

        let power_label = if let Some(label) = power_box.last_child() {
            if let Some(label_widget) = label.downcast_ref::<Label>() {
                Some(label_widget.clone())
            } else {
                None
            }
        } else {
            None
        };

        let type_label = if let Some(label) = type_box.last_child() {
            if let Some(label_widget) = label.downcast_ref::<Label>() {
                Some(label_widget.clone())
            } else {
                None
            }
        } else {
            None
        };

        timeout_add_seconds_local(30, move || {
            Self::update_battery_info(
                &container_clone,
                &icon_clone,
                &label_clone,
                &status_label_clone,
                &level_icon_clone,
                &level_scale_clone,
                &level_percentage_clone,
            );

            // Update additional stats
            if let Some(info) = Self::get_battery_info() {
                // Update time remaining
                if let Some(time) = &time_label {
                    if let Some(remaining) = &info.time_remaining {
                        time.set_text(remaining);
                    } else {
                        time.set_text("Unknown");
                    }
                }

                // Update power draw
                if let Some(power) = &power_label {
                    if let Some(draw) = info.power_draw {
                        power.set_text(&format!("{:.1} W", draw));
                    } else {
                        power.set_text("Unknown");
                    }
                }

                // Update battery type
                if let Some(type_lbl) = &type_label {
                    match info.battery_type {
                        BatteryType::Battery => type_lbl.set_text("Battery"),
                        BatteryType::UPS => type_lbl.set_text("UPS"),
                        BatteryType::Unknown => type_lbl.set_text("Unknown"),
                    }
                }
            }

            // Continue the timer
            glib::ControlFlow::Continue
        });

        // Try to set up a file monitor for faster updates
        if let Ok(battery_rx) = Self::setup_battery_monitor() {
            info!("Battery monitoring initialized with file system watcher");

            let container_watch = container.clone();
            let icon_watch = icon.clone();
            let label_watch = label.clone();
            let status_label_watch = status_label.clone();
            let level_icon_watch = level_icon.clone();
            let level_scale_watch = level_scale.clone();
            let level_percentage_watch = level_percentage.clone();

            // Check for battery updates from file system monitor
            glib::timeout_add_local(Duration::from_millis(100), move || {
                // Check if we have any battery updates
                if battery_rx.try_recv().is_ok() {
                    Self::update_battery_info(
                        &container_watch,
                        &icon_watch,
                        &label_watch,
                        &status_label_watch,
                        &level_icon_watch,
                        &level_scale_watch,
                        &level_percentage_watch,
                    );
                }
                glib::ControlFlow::Continue
            });
        }

        // Try to set up brightness monitoring
        if let Ok(brightness_rx) = Self::setup_brightness_monitor() {
            // Create scroll event handler for brightness control
            let controller =
                gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
            controller.connect_scroll(move |_, _, dy| {
                Self::adjust_brightness(dy);
                glib::Propagation::Stop
            });
            button.add_controller(controller);

            // Monitor brightness changes from file system
            glib::timeout_add_local(Duration::from_millis(100), move || {
                while let Ok(brightness) = brightness_rx.try_recv() {
                    info!("Brightness changed: {}", brightness);
                    // Could use this to update a brightness indicator if we add one
                }
                glib::ControlFlow::Continue
            });
        }

        Ok(Self { button, popover })
    }

    fn setup_brightness_monitor() -> Result<mpsc::Receiver<u32>> {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            // Check for common brightness file locations
            let brightness_paths = vec![
                // Intel backlight
                "/sys/class/backlight/intel_backlight/brightness",
                // ACPI backlight
                "/sys/class/backlight/acpi_video0/brightness",
                // AMD backlight
                "/sys/class/backlight/amdgpu_bl0/brightness",
                // Generic backlight
                "/sys/class/backlight/*/brightness",
            ];

            // Find the first existing path
            let mut path_to_watch = None;
            for path_str in brightness_paths {
                // Handle glob patterns
                if path_str.contains('*') {
                    if let Ok(entries) = glob::glob(path_str) {
                        for entry in entries {
                            if let Ok(path) = entry {
                                if path.exists() {
                                    path_to_watch = Some(path);
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    let path = PathBuf::from(path_str);
                    if path.exists() {
                        path_to_watch = Some(path);
                        break;
                    }
                }
            }

            // If we found a brightness file
            if let Some(brightness_path) = path_to_watch {
                info!("Monitoring brightness at: {:?}", brightness_path);

                // Get the parent directory to monitor
                if let Some(parent) = brightness_path.parent() {
                    // Create a channel for watcher
                    let (watch_tx, watch_rx) = mpsc::channel();

                    // Create a watcher
                    let mut watcher = match RecommendedWatcher::new(
                        move |res: Result<Event, notify::Error>| {
                            if let Ok(event) = res {
                                let _ = watch_tx.send(event);
                            }
                        },
                        Config::default(),
                    ) {
                        Ok(w) => w,
                        Err(e) => {
                            warn!("Failed to create brightness watcher: {}", e);
                            return;
                        }
                    };

                    // Watch the backlight directory
                    if let Err(e) = watcher.watch(parent, RecursiveMode::NonRecursive) {
                        warn!("Failed to watch brightness path: {}", e);
                        return;
                    }

                    // Read initial brightness
                    if let Ok(content) = fs::read_to_string(&brightness_path) {
                        if let Ok(brightness) = content.trim().parse::<u32>() {
                            let _ = tx.send(brightness);
                        }
                    }

                    // Process file change events
                    while let Ok(event) = watch_rx.recv() {
                        match event.kind {
                            EventKind::Modify(_) => {
                                // Try to read updated brightness
                                if let Ok(content) = fs::read_to_string(&brightness_path) {
                                    if let Ok(brightness) = content.trim().parse::<u32>() {
                                        let _ = tx.send(brightness);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    fn setup_battery_monitor() -> Result<mpsc::Receiver<()>> {
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            // Check for common battery file locations
            let battery_paths = vec![
                // Linux power supply class
                "/sys/class/power_supply/BAT0",
                "/sys/class/power_supply/BAT1",
                // UPS devices
                "/sys/class/power_supply/ups",
            ];

            // Find existing paths
            let mut paths_to_watch = Vec::new();
            for path_str in battery_paths {
                let path = PathBuf::from(path_str);
                if path.exists() {
                    paths_to_watch.push(path);
                }
            }

            // If we found any battery paths
            if !paths_to_watch.is_empty() {
                info!("Monitoring battery paths: {:?}", paths_to_watch);

                // Create a channel for watcher
                let (watch_tx, watch_rx) = mpsc::channel();

                // Create a watcher
                let mut watcher = match RecommendedWatcher::new(
                    move |res: Result<Event, notify::Error>| {
                        if let Ok(event) = res {
                            let _ = watch_tx.send(event);
                        }
                    },
                    Config::default(),
                ) {
                    Ok(w) => w,
                    Err(e) => {
                        warn!("Failed to create battery watcher: {}", e);
                        return;
                    }
                };

                // Watch all battery directories
                for path in &paths_to_watch {
                    if let Err(e) = watcher.watch(path, RecursiveMode::Recursive) {
                        warn!("Failed to watch battery path: {:?} - {}", path, e);
                    }
                }

                // Send initial notification
                let _ = tx.send(());

                // Process file change events
                while let Ok(event) = watch_rx.recv() {
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            // Battery status changed
                            let _ = tx.send(());
                        }
                        _ => {}
                    }
                }
            } else {
                warn!("No battery paths found to monitor");
            }
        });

        Ok(rx)
    }

    fn update_battery_info(
        container: &Box,
        icon: &Image,
        label: &Label,
        status_label: &Label,
        level_icon: &Image,
        level_scale: &Scale,
        level_percentage: &Label,
    ) {
        let battery_info = Self::get_battery_info();

        if container.first_child().is_none() {
            container.append(icon);
            container.append(label);
        }

        match battery_info {
            Some(info) => {
                let percentage = info.percentage;
                let state = info.state;

                // Update panel icon based on percentage and state
                let icon_name = Self::get_battery_icon(percentage, state);
                icon.set_from_icon_name(Some(icon_name));

                // Update percentage label in panel
                if percentage >= 0 {
                    if percentage <= 10 {
                        label.add_css_class("battery-critical");
                    } else {
                        label.remove_css_class("battery-critical");
                    }

                    label.set_text(&format!("{}%", percentage));
                } else {
                    label.set_text("??%");
                }

                // Update popover status label
                let status_text = match state {
                    BatteryState::Charging => format!("Battery Charging ({}%)", percentage),
                    BatteryState::Discharging => {
                        if let Some(time) = &info.time_remaining {
                            format!("Battery Discharging ({}%) - {} remaining", percentage, time)
                        } else {
                            format!("Battery Discharging ({}%)", percentage)
                        }
                    }
                    BatteryState::Full => "Battery Full (100%)".to_string(),
                    BatteryState::Unknown => {
                        if percentage >= 0 {
                            format!("Battery Level: {}%", percentage)
                        } else {
                            "Battery Status Unknown".to_string()
                        }
                    }
                };
                status_label.set_text(&status_text);

                // Update level bar
                level_icon.set_from_icon_name(Some(icon_name));
                level_scale.set_value(percentage as f64);
                level_percentage.set_text(&format!("{}%", percentage));

                // Show the widget
                container.set_visible(true);
            }
            None => {
                // Hide battery widget if no battery found
                container.set_visible(false);
                status_label.set_text("No battery detected");
                level_scale.set_value(0.0);
                level_percentage.set_text("N/A");
            }
        }
    }

    fn get_battery_icon(percentage: i32, state: BatteryState) -> &'static str {
        match state {
            BatteryState::Charging => match percentage {
                0..=10 => "battery-level-10-charging-symbolic",
                11..=20 => "battery-level-20-charging-symbolic",
                21..=30 => "battery-level-30-charging-symbolic",
                31..=40 => "battery-level-40-charging-symbolic",
                41..=50 => "battery-level-50-charging-symbolic",
                51..=60 => "battery-level-60-charging-symbolic",
                61..=70 => "battery-level-70-charging-symbolic",
                71..=80 => "battery-level-80-charging-symbolic",
                81..=90 => "battery-level-90-charging-symbolic",
                91..=100 => "battery-level-100-charged-symbolic",
                _ => "battery-missing-symbolic",
            },
            BatteryState::Discharging | BatteryState::Unknown => match percentage {
                0..=10 => "battery-level-10-symbolic",
                11..=20 => "battery-level-20-symbolic",
                21..=30 => "battery-level-30-symbolic",
                31..=40 => "battery-level-40-symbolic",
                41..=50 => "battery-level-50-symbolic",
                51..=60 => "battery-level-60-symbolic",
                61..=70 => "battery-level-70-symbolic",
                71..=80 => "battery-level-80-symbolic",
                81..=90 => "battery-level-90-symbolic",
                91..=100 => "battery-level-100-symbolic",
                _ => "battery-missing-symbolic",
            },
            BatteryState::Full => "battery-level-100-charged-symbolic",
        }
    }

    fn get_battery_info() -> Option<BatteryInfo> {
        // Try to get battery info from system files
        let battery_paths = [
            "/sys/class/power_supply/BAT0",
            "/sys/class/power_supply/BAT1",
            "/sys/class/power_supply/BAT2",
        ];

        for path in battery_paths {
            let bat_path = Path::new(path);
            if bat_path.exists() {
                // Check that this is actually a battery
                if let Ok(type_str) = fs::read_to_string(bat_path.join("type")) {
                    let type_str = type_str.trim();
                    if type_str != "Battery" {
                        continue;
                    }
                }

                // Get battery percentage
                let percentage = if let Ok(energy_now) = fs::read_to_string(bat_path.join("energy_now"))
                    .or_else(|_| fs::read_to_string(bat_path.join("charge_now")))
                {
                    if let Ok(energy_full) = fs::read_to_string(bat_path.join("energy_full"))
                        .or_else(|_| fs::read_to_string(bat_path.join("charge_full")))
                    {
                        let now = energy_now.trim().parse::<f64>().unwrap_or(0.0);
                        let full = energy_full.trim().parse::<f64>().unwrap_or(1.0);
                        if full > 0.0 {
                            (now / full * 100.0) as i32
                        } else {
                            -1
                        }
                    } else {
                        // Try capacity directly
                        if let Ok(capacity) = fs::read_to_string(bat_path.join("capacity")) {
                            capacity.trim().parse::<i32>().unwrap_or(-1)
                        } else {
                            -1
                        }
                    }
                } else {
                    // Try capacity directly
                    if let Ok(capacity) = fs::read_to_string(bat_path.join("capacity")) {
                        capacity.trim().parse::<i32>().unwrap_or(-1)
                    } else {
                        -1
                    }
                };

                // Get battery state
                let state = if let Ok(status) = fs::read_to_string(bat_path.join("status")) {
                    match status.trim() {
                        "Charging" => BatteryState::Charging,
                        "Discharging" => BatteryState::Discharging,
                        "Full" => BatteryState::Full,
                        _ => BatteryState::Unknown,
                    }
                } else {
                    BatteryState::Unknown
                };

                // Calculate time remaining
                let time_remaining = if state == BatteryState::Discharging || state == BatteryState::Charging
                {
                    if let Ok(power_now) = fs::read_to_string(bat_path.join("power_now"))
                        .or_else(|_| fs::read_to_string(bat_path.join("current_now")))
                    {
                        let power = power_now.trim().parse::<f64>().unwrap_or(0.0);
                        if power > 0.0 {
                            if let Ok(energy_now) = fs::read_to_string(bat_path.join("energy_now"))
                                .or_else(|_| fs::read_to_string(bat_path.join("charge_now")))
                            {
                                let energy = energy_now.trim().parse::<f64>().unwrap_or(0.0);
                                if state == BatteryState::Discharging {
                                    let hours = energy / power;
                                    Some(Self::format_time(hours))
                                } else {
                                    if let Ok(energy_full) = fs::read_to_string(bat_path.join("energy_full"))
                                        .or_else(|_| {
                                            fs::read_to_string(bat_path.join("charge_full"))
                                        })
                                    {
                                        let full = energy_full.trim().parse::<f64>().unwrap_or(0.0);
                                        let remaining = full - energy;
                                        if remaining > 0.0 {
                                            let hours = remaining / power;
                                            Some(Self::format_time(hours))
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    }
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Get power draw
                let power_draw = if let Ok(power_now) = fs::read_to_string(bat_path.join("power_now"))
                    .or_else(|_| fs::read_to_string(bat_path.join("current_now")))
                {
                    if let Ok(voltage) = fs::read_to_string(bat_path.join("voltage_now")) {
                        let power = power_now.trim().parse::<f64>().unwrap_or(0.0);
                        let voltage = voltage.trim().parse::<f64>().unwrap_or(0.0);

                        // Convert to watts (power in microW, or current in microA * voltage in microV -> W)
                        if power > 0.0 && voltage > 0.0 {
                            if bat_path.join("power_now").exists() {
                                Some(power / 1_000_000.0) // microW to W
                            } else {
                                Some((power * voltage) / 1_000_000_000_000.0) // microA * microV to W
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Get battery type
                let battery_type = if let Ok(type_str) = fs::read_to_string(bat_path.join("type")) {
                    match type_str.trim() {
                        "Battery" => BatteryType::Battery,
                        "UPS" => BatteryType::UPS,
                        _ => BatteryType::Unknown,
                    }
                } else {
                    BatteryType::Unknown
                };

                return Some(BatteryInfo {
                    percentage,
                    state,
                    time_remaining,
                    power_draw,
                    battery_type,
                });
            }
        }

        // No battery found in sysfs, try UPower
        Self::get_battery_info_upower()
    }

    fn get_battery_info_upower() -> Option<BatteryInfo> {
        if let Ok(output) = Command::new("upower").args(&["-e"]).output() {
            let device_list = String::from_utf8_lossy(&output.stdout);

            for device in device_list.lines() {
                if device.contains("battery_") || device.contains("BAT") {
                    if let Ok(info_output) = Command::new("upower").args(&["-i", device]).output() {
                        let info_str = String::from_utf8_lossy(&info_output.stdout);

                        let mut percentage = -1;
                        let mut state = BatteryState::Unknown;
                        let mut time_remaining = None;
                        let mut power_draw = None;
                        let mut battery_type = BatteryType::Unknown;

                        for line in info_str.lines() {
                            let line = line.trim();

                            if line.starts_with("percentage:") {
                                percentage = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.trim_end_matches('%').parse::<i32>().ok())
                                    .unwrap_or(-1);
                            } else if line.starts_with("state:") {
                                state = match line.split_whitespace().nth(1).unwrap_or("") {
                                    "charging" => BatteryState::Charging,
                                    "discharging" => BatteryState::Discharging,
                                    "fully-charged" => BatteryState::Full,
                                    _ => BatteryState::Unknown,
                                };
                            } else if line.starts_with("time to empty:") {
                                if state == BatteryState::Discharging {
                                    time_remaining = line
                                        .split_whitespace()
                                        .nth(3)
                                        .map(|s| s.to_string());
                                }
                            } else if line.starts_with("time to full:") {
                                if state == BatteryState::Charging {
                                    time_remaining = line
                                        .split_whitespace()
                                        .nth(3)
                                        .map(|s| s.to_string());
                                }
                            } else if line.starts_with("energy-rate:") {
                                power_draw = line
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| s.parse::<f64>().ok());
                            } else if line.starts_with("native-path:") {
                                if let Some(path) = line.split_whitespace().nth(1) {
                                    if path.contains("UPS") {
                                        battery_type = BatteryType::UPS;
                                    } else {
                                        battery_type = BatteryType::Battery;
                                    }
                                }
                            }
                        }

                        return Some(BatteryInfo {
                            percentage,
                            state,
                            time_remaining,
                            power_draw,
                            battery_type,
                        });
                    }
                }
            }
        }

        None
    }

    fn format_time(hours: f64) -> String {
        let hours_int = hours.floor() as i32;
        let minutes = ((hours - hours_int as f64) * 60.0) as i32;

        if hours_int > 0 {
            format!("{}h {}m", hours_int, minutes)
        } else {
            format!("{}m", minutes)
        }
    }

    fn adjust_brightness(delta: f64) {
        // Find brightnctl or other brightness control
        let brightness_commands = [
            ("brightnessctl", vec!["set", if delta > 0.0 { "5%-" } else { "5%+" }]),
            ("light", vec![if delta > 0.0 { "-U" } else { "-A" }, "5"]),
            ("xbacklight", vec![if delta > 0.0 { "-dec" } else { "-inc" }, "5"]),
        ];

        for (cmd, args) in &brightness_commands {
            if Command::new("which").arg(cmd).output().map_or(false, |output| output.status.success())
            {
                let _ = Command::new(cmd).args(args).spawn();
                return;
            }
        }

        // Try direct sysfs method as fallback
        if let Some(path) = Self::find_brightness_path() {
            // Read max brightness
            if let Ok(max_str) = fs::read_to_string(path.with_file_name("max_brightness")) {
                if let Ok(max) = max_str.trim().parse::<i32>() {
                    // Read current brightness
                    if let Ok(current_str) = fs::read_to_string(&path) {
                        if let Ok(current) = current_str.trim().parse::<i32>() {
                            // Calculate new brightness (5% change)
                            let step = max / 20; // 5% of max
                            let new_value = if delta > 0.0 {
                                (current - step).max(1) // Don't go completely dark
                            } else {
                                (current + step).min(max)
                            };

                            // Write new brightness
                            if new_value != current {
                                let _ = fs::write(&path, new_value.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    fn find_brightness_path() -> Option<PathBuf> {
        let paths = [
            "/sys/class/backlight/intel_backlight/brightness",
            "/sys/class/backlight/acpi_video0/brightness",
            "/sys/class/backlight/amdgpu_bl0/brightness",
        ];

        for path_str in &paths {
            let path = PathBuf::from(path_str);
            if path.exists() {
                return Some(path);
            }
        }

        None
    }

    fn open_power_settings() {
        // Try different power settings commands
        let commands = vec![
            ("gnome-control-center", vec!["power"]),
            ("xfce4-power-manager-settings", vec![]),
            ("powerprofilesctl", vec!["launch"]),
            ("mate-power-preferences", vec![]),
        ];

        for (cmd, args) in commands {
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }

        warn!("Could not find power settings application");
    }

    fn create_stat_label(title: &str, initial_value: &str) -> Box {
        let hbox = Box::new(Orientation::Horizontal, 12);
        hbox.set_margin_start(4);

        let title_label = Label::new(Some(title));
        title_label.set_halign(gtk4::Align::Start);
        title_label.add_css_class("battery-stat-title");
        title_label.set_hexpand(true);

        let value_label = Label::new(Some(initial_value));
        value_label.set_halign(gtk4::Align::End);
        value_label.add_css_class("battery-stat-value");

        hbox.append(&title_label);
        hbox.append(&value_label);

        hbox
    }

    pub fn widget(&self) -> &Button {
        &self.button
    }
}

// Implementation of Widget trait
impl WidgetTrait for Battery {
    fn popover(&self) -> Option<&Popover> {
        Some(&self.popover)
    }
}