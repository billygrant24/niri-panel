use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, ListBox, ListBoxRow, Scale};
use glib::timeout_add_seconds_local;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{warn, info};
use notify::{Watcher, RecursiveMode, Event, EventKind, RecommendedWatcher, Config};

pub struct Battery {
    button: Button,
}

#[derive(Debug)]
struct BatteryInfo {
    percentage: u32,
    charging: bool,
    plugged: bool,
    time_to_empty: Option<String>,
    time_to_full: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum PowerProfile {
    PowerSaver,
    Balanced,
    Performance,
}

#[derive(Debug)]
struct SystemStats {
    uptime: String,
    cpu_usage: f32,
    temperature: Option<f32>,
    power_consumption: Option<f32>,
}

impl PowerProfile {
    fn to_string(&self) -> &str {
        match self {
            PowerProfile::PowerSaver => "power-saver",
            PowerProfile::Balanced => "balanced",
            PowerProfile::Performance => "performance",
        }
    }
    
    fn from_string(s: &str) -> Option<Self> {
        match s.trim() {
            "power-saver" => Some(PowerProfile::PowerSaver),
            "balanced" => Some(PowerProfile::Balanced),
            "performance" => Some(PowerProfile::Performance),
            _ => None,
        }
    }
    
    fn display_name(&self) -> &str {
        match self {
            PowerProfile::PowerSaver => "Power Saver",
            PowerProfile::Balanced => "Balanced",
            PowerProfile::Performance => "Performance",
        }
    }
    
    fn icon_name(&self) -> &str {
        match self {
            PowerProfile::PowerSaver => "power-profile-power-saver-symbolic",
            PowerProfile::Balanced => "power-profile-balanced-symbolic",
            PowerProfile::Performance => "power-profile-performance-symbolic",
        }
    }
}

impl Battery {
    pub fn new() -> Result<Self> {
        let button = Button::new();
        button.add_css_class("battery");
        
        let container = Box::new(Orientation::Horizontal, 5);
        
        let icon = Image::new();
        let label = Label::new(None);
        label.add_css_class("battery-percentage");
        
        container.append(&icon);
        container.append(&label);
        button.set_child(Some(&container));
        
        // Create popover for battery details
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("battery-popover");
        
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(10);
        popover_box.set_margin_bottom(10);
        popover_box.set_margin_start(10);
        popover_box.set_margin_end(10);
        popover_box.set_size_request(300, -1);
        
        // Battery status section
        let status_box = Box::new(Orientation::Vertical, 5);
        status_box.add_css_class("battery-status-box");
        
        let status_label = Label::new(None);
        status_label.add_css_class("battery-status-label");
        status_label.set_halign(gtk4::Align::Start);
        status_box.append(&status_label);
        
        let time_label = Label::new(None);
        time_label.add_css_class("battery-time-label");
        time_label.set_halign(gtk4::Align::Start);
        status_box.append(&time_label);
        
        popover_box.append(&status_box);
        
        // System stats section
        let stats_separator = gtk4::Separator::new(Orientation::Horizontal);
        stats_separator.set_margin_top(5);
        stats_separator.set_margin_bottom(5);
        popover_box.append(&stats_separator);
        
        let stats_label = Label::new(Some("System Status"));
        stats_label.set_halign(gtk4::Align::Start);
        stats_label.add_css_class("battery-section-label");
        popover_box.append(&stats_label);
        
        // Stats labels
        let uptime_label = Self::create_stat_label("Uptime", "0m");
        let cpu_label = Self::create_stat_label("CPU Usage", "0%");
        let temp_label = Self::create_stat_label("Temperature", "N/A");
        let power_label = Self::create_stat_label("Power Draw", "N/A");
        
        popover_box.append(&uptime_label);
        popover_box.append(&cpu_label);
        popover_box.append(&temp_label);
        popover_box.append(&power_label);
        
        // Brightness control section
        let brightness_separator = gtk4::Separator::new(Orientation::Horizontal);
        brightness_separator.set_margin_top(5);
        brightness_separator.set_margin_bottom(5);
        popover_box.append(&brightness_separator);
        
        let brightness_label = Label::new(Some("Screen Brightness"));
        brightness_label.set_halign(gtk4::Align::Start);
        brightness_label.add_css_class("battery-section-label");
        popover_box.append(&brightness_label);
        
        let brightness_box = Box::new(Orientation::Horizontal, 10);
        
        let brightness_icon = Image::from_icon_name("display-brightness-symbolic");
        brightness_box.append(&brightness_icon);
        
        let brightness_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        brightness_scale.set_hexpand(true);
        brightness_scale.set_draw_value(false);
        brightness_scale.add_css_class("brightness-slider");
        
        let brightness_value_label = Label::new(Some("50%"));
        brightness_value_label.set_width_chars(4);
        brightness_value_label.add_css_class("brightness-label");
        
        brightness_box.append(&brightness_scale);
        brightness_box.append(&brightness_value_label);
        
        popover_box.append(&brightness_box);
        
        // Set initial brightness
        if let Some(current_brightness) = Self::get_brightness() {
            brightness_scale.set_value(current_brightness as f64);
            brightness_value_label.set_text(&format!("{}%", current_brightness));
        }
        
        // Handle brightness changes from the slider
        let brightness_value_weak = brightness_value_label.downgrade();
        let brightness_updating = std::rc::Rc::new(std::cell::RefCell::new(false));
        let brightness_updating_clone = brightness_updating.clone();
        brightness_scale.connect_value_changed(move |scale| {
            // Set flag to prevent feedback loop
            *brightness_updating_clone.borrow_mut() = true;
            
            let value = scale.value() as u32;
            Self::set_brightness(value);
            
            if let Some(label) = brightness_value_weak.upgrade() {
                label.set_text(&format!("{}%", value));
            }
            
            // Clear flag after a short delay
            let brightness_updating_clear = brightness_updating_clone.clone();
            glib::timeout_add_local_once(Duration::from_millis(100), move || {
                *brightness_updating_clear.borrow_mut() = false;
            });
        });
        
        // Set up brightness monitoring using notify
        let brightness_scale_weak = brightness_scale.downgrade();
        let brightness_label_weak = brightness_value_label.downgrade();
        let brightness_updating_for_monitor = brightness_updating.clone();
        
        if let Ok(brightness_rx) = Self::setup_brightness_monitor() {
            info!("Brightness monitoring initialized");
            
            // Spawn a timeout to check for brightness changes
            glib::timeout_add_local(Duration::from_millis(50), move || {
                // Check if we have any brightness updates
                while let Ok(brightness) = brightness_rx.try_recv() {
                    // Only update if we're not currently updating from the slider
                    if !*brightness_updating_for_monitor.borrow() {
                        if let (Some(scale), Some(label)) = 
                            (brightness_scale_weak.upgrade(), brightness_label_weak.upgrade()) {
                            scale.set_value(brightness as f64);
                            label.set_text(&format!("{}%", brightness));
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        } else {
            warn!("Failed to set up brightness monitoring");
        }
        
        // Power profiles section
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        popover_box.append(&separator);
        
        let profiles_label = Label::new(Some("Power Profile"));
        profiles_label.set_halign(gtk4::Align::Start);
        profiles_label.add_css_class("battery-section-label");
        popover_box.append(&profiles_label);
        
        let profiles_list = ListBox::new();
        profiles_list.add_css_class("battery-profiles-list");
        profiles_list.set_selection_mode(gtk4::SelectionMode::None);
        
        // Check if power-profiles-daemon is available
        let has_ppd = Self::check_power_profiles_daemon();
        
        if has_ppd {
            // Add power profile options
            for profile in [PowerProfile::PowerSaver, PowerProfile::Balanced, PowerProfile::Performance] {
                let row = ListBoxRow::new();
                row.add_css_class("battery-profile-row");
                
                let hbox = Box::new(Orientation::Horizontal, 10);
                hbox.set_margin_start(5);
                hbox.set_margin_end(5);
                hbox.set_margin_top(8);
                hbox.set_margin_bottom(8);
                
                let profile_icon = Image::from_icon_name(profile.icon_name());
                profile_icon.set_pixel_size(16);
                hbox.append(&profile_icon);
                
                let profile_label = Label::new(Some(profile.display_name()));
                profile_label.set_hexpand(true);
                profile_label.set_halign(gtk4::Align::Start);
                hbox.append(&profile_label);
                
                let check_icon = Image::from_icon_name("object-select-symbolic");
                check_icon.set_pixel_size(16);
                check_icon.set_visible(false);
                check_icon.add_css_class("battery-profile-check");
                hbox.append(&check_icon);
                
                row.set_child(Some(&hbox));
                
                // Store profile type in widget name for retrieval
                row.set_widget_name(profile.to_string());
                
                profiles_list.append(&row);
            }
            
            // Handle profile selection
            profiles_list.connect_row_activated(move |list, row| {
                let profile_name = row.widget_name();
                if let Some(profile) = PowerProfile::from_string(&profile_name) {
                    Self::set_power_profile(profile);
                    
                    // Update UI to show selected profile
                    Self::update_profile_selection(list);
                }
            });
            
            popover_box.append(&profiles_list);
        } else {
            let no_ppd_label = Label::new(Some("Power profiles not available"));
            no_ppd_label.add_css_class("battery-no-ppd");
            no_ppd_label.set_halign(gtk4::Align::Start);
            popover_box.append(&no_ppd_label);
        }
        
        // Power settings button
        let settings_button = Button::with_label("Power Settings");
        settings_button.set_margin_top(10);
        settings_button.connect_clicked(|_| {
            Self::open_power_settings();
        });
        popover_box.append(&settings_button);
        
        popover.set_child(Some(&popover_box));
        
        // Update battery status immediately
        Self::update_battery(&icon, &label, &status_label, &time_label, &profiles_list,
                           &uptime_label, &cpu_label, &temp_label, &power_label);
        
        // Update every 30 seconds for battery, every 2 seconds for stats when visible
        let icon_weak = icon.downgrade();
        let label_weak = label.downgrade();
        let status_label_weak = status_label.downgrade();
        let time_label_weak = time_label.downgrade();
        let profiles_list_weak = profiles_list.downgrade();
        let uptime_weak = uptime_label.downgrade();
        let cpu_weak = cpu_label.downgrade();
        let temp_weak = temp_label.downgrade();
        let power_weak = power_label.downgrade();
        let popover_weak = popover.downgrade();
        
        // Fast update timer (2s) for system stats when popover is visible
        timeout_add_seconds_local(2, move || {
            if let Some(popover) = popover_weak.upgrade() {
                if popover.is_visible() {
                    if let (Some(icon), Some(label), Some(status), Some(time), Some(profiles),
                            Some(uptime), Some(cpu), Some(temp), Some(power)) = 
                        (icon_weak.upgrade(), label_weak.upgrade(), status_label_weak.upgrade(), 
                         time_label_weak.upgrade(), profiles_list_weak.upgrade(),
                         uptime_weak.upgrade(), cpu_weak.upgrade(), temp_weak.upgrade(), power_weak.upgrade()) {
                        Self::update_battery(&icon, &label, &status, &time, &profiles,
                                           &uptime, &cpu, &temp, &power);
                    }
                }
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        
        // Slow update timer (30s) for battery icon/label
        let icon_weak2 = icon.downgrade();
        let label_weak2 = label.downgrade();
        timeout_add_seconds_local(30, move || {
            if let (Some(icon), Some(label)) = (icon_weak2.upgrade(), label_weak2.upgrade()) {
                if let Some(info) = Self::get_battery_info() {
                    // Update icon and label
                    let icon_name = Self::get_battery_icon_name(&info);
                    icon.set_from_icon_name(Some(&icon_name));
                    label.set_text(&format!("{}%", info.percentage));
                    
                    // Update CSS class for low battery
                    if info.percentage <= 20 && !info.charging {
                        label.add_css_class("battery-low");
                    } else {
                        label.remove_css_class("battery-low");
                    }
                }
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        
        // Show popover on click
        button.connect_clicked(move |_| {
            // Update profile selection before showing
            Self::update_profile_selection(&profiles_list);
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    fn setup_brightness_monitor() -> Result<mpsc::Receiver<u32>> {
        let (tx, rx) = mpsc::channel();
        
        // Find brightness file path
        let brightness_path = Self::find_brightness_file()
            .ok_or_else(|| anyhow::anyhow!("No brightness file found"))?;
        
        // Get parent directory for max_brightness
        let backlight_dir = brightness_path.parent()
            .ok_or_else(|| anyhow::anyhow!("Invalid brightness path"))?;
        
        // Read max brightness once
        let max_brightness = Self::read_max_brightness(backlight_dir);
        
        // Clone paths for the thread
        let brightness_path_clone = brightness_path.clone();
        let backlight_dir_clone = backlight_dir.to_path_buf();
        
        thread::spawn(move || {
            // Create channel for watcher events
            let (watch_tx, watch_rx) = mpsc::channel();
            
            // Create a watcher with default config
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
                    warn!("Failed to create file watcher: {}", e);
                    return;
                }
            };
            
            // Watch the brightness file
            if let Err(e) = watcher.watch(&brightness_path_clone, RecursiveMode::NonRecursive) {
                warn!("Failed to watch brightness file: {}", e);
                return;
            }
            
            info!("Watching brightness file: {:?}", brightness_path_clone);
            
            // Also watch max_brightness in case it changes
            let max_brightness_path = backlight_dir_clone.join("max_brightness");
            let _ = watcher.watch(&max_brightness_path, RecursiveMode::NonRecursive);
            
            let mut current_max = max_brightness;
            
            // Process file change events
            while let Ok(event) = watch_rx.recv() {
                match event.kind {
                    EventKind::Modify(_) => {
                        // Check which file was modified
                        for path in &event.paths {
                            if path.file_name() == Some(std::ffi::OsStr::new("max_brightness")) {
                                // Max brightness changed, re-read it
                                if let Ok(content) = fs::read_to_string(path) {
                                    if let Ok(new_max) = content.trim().parse::<u32>() {
                                        current_max = new_max;
                                        info!("Max brightness updated to: {}", current_max);
                                    }
                                }
                            } else if path == &brightness_path_clone {
                                // Brightness changed
                                if let Ok(content) = fs::read_to_string(path) {
                                    if let Ok(brightness) = content.trim().parse::<u32>() {
                                        let percentage = if current_max > 0 {
                                            (brightness * 100) / current_max
                                        } else {
                                            0
                                        };
                                        let _ = tx.send(percentage);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            
            warn!("Brightness monitor thread exiting");
        });
        
        Ok(rx)
    }
    
    fn find_brightness_file() -> Option<PathBuf> {
        let backlight_dir = Path::new("/sys/class/backlight");
        if let Ok(entries) = fs::read_dir(backlight_dir) {
            for entry in entries.flatten() {
                let brightness_path = entry.path().join("brightness");
                if brightness_path.exists() {
                    return Some(brightness_path);
                }
            }
        }
        None
    }
    
    fn read_max_brightness(backlight_path: &Path) -> u32 {
        fs::read_to_string(backlight_path.join("max_brightness"))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(100)
    }
    
    fn create_stat_label(title: &str, initial_value: &str) -> Box {
        let hbox = Box::new(Orientation::Horizontal, 0);
        
        let title_label = Label::new(Some(title));
        title_label.add_css_class("battery-stat-title");
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_hexpand(true);
        
        let value_label = Label::new(Some(initial_value));
        value_label.add_css_class("battery-stat-value");
        value_label.set_halign(gtk4::Align::End);
        
        hbox.append(&title_label);
        hbox.append(&value_label);
        
        hbox
    }
    
    fn update_battery(icon: &Image, label: &Label, status_label: &Label, time_label: &Label, 
                     profiles_list: &ListBox, uptime_box: &Box, cpu_box: &Box, 
                     temp_box: &Box, power_box: &Box) {
        if let Some(info) = Self::get_battery_info() {
            // Update icon based on battery level and charging status
            let icon_name = Self::get_battery_icon_name(&info);
            icon.set_from_icon_name(Some(&icon_name));
            
            // Update label
            label.set_text(&format!("{}%", info.percentage));
            
            // Update status
            let status_text = if info.charging {
                format!("Charging - {}%", info.percentage)
            } else if info.plugged {
                "Fully Charged".to_string()
            } else {
                format!("On Battery - {}%", info.percentage)
            };
            status_label.set_text(&status_text);
            
            // Update time estimate
            let time_text = if info.charging {
                info.time_to_full.unwrap_or_else(|| "Calculating...".to_string())
            } else if !info.plugged {
                info.time_to_empty.unwrap_or_else(|| "Calculating...".to_string())
            } else {
                String::new()
            };
            time_label.set_text(&time_text);
            time_label.set_visible(!time_text.is_empty());
            
            // Add CSS class for low battery warning
            if info.percentage <= 20 && !info.charging {
                label.add_css_class("battery-low");
            } else {
                label.remove_css_class("battery-low");
            }
        } else {
            // No battery found (probably desktop)
            icon.set_from_icon_name(Some("battery-missing-symbolic"));
            label.set_text("N/A");
            status_label.set_text("No battery detected");
            time_label.set_visible(false);
        }
        
        // Update system stats
        let stats = Self::get_system_stats();
        
        // Update uptime
        if let Some(value_label) = uptime_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&stats.uptime);
            }
        }
        
        // Update CPU
        if let Some(value_label) = cpu_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&format!("{:.1}%", stats.cpu_usage));
            }
        }
        
        // Update temperature
        if let Some(value_label) = temp_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                if let Some(temp) = stats.temperature {
                    label.set_text(&format!("{:.1}°C", temp));
                } else {
                    label.set_text("N/A");
                }
            }
        }
        
        // Update power consumption
        if let Some(value_label) = power_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                if let Some(power) = stats.power_consumption {
                    label.set_text(&format!("{:.1}W", power));
                } else {
                    label.set_text("N/A");
                }
            }
        }
        
        // Update power profile selection
        Self::update_profile_selection(profiles_list);
    }
    
    fn get_system_stats() -> SystemStats {
        let mut stats = SystemStats {
            uptime: "Unknown".to_string(),
            cpu_usage: 0.0,
            temperature: None,
            power_consumption: None,
        };
        
        // Get uptime
        if let Ok(uptime_content) = fs::read_to_string("/proc/uptime") {
            if let Some(uptime_str) = uptime_content.split_whitespace().next() {
                if let Ok(uptime_secs) = uptime_str.parse::<f64>() {
                    let days = (uptime_secs / 86400.0) as u64;
                    let hours = ((uptime_secs % 86400.0) / 3600.0) as u64;
                    let minutes = ((uptime_secs % 3600.0) / 60.0) as u64;
                    
                    if days > 0 {
                        stats.uptime = format!("{}d {}h {}m", days, hours, minutes);
                    } else if hours > 0 {
                        stats.uptime = format!("{}h {}m", hours, minutes);
                    } else {
                        stats.uptime = format!("{}m", minutes);
                    }
                }
            }
        }
        
        // Get CPU usage (simple average from /proc/stat)
        if let Ok(stat_content) = fs::read_to_string("/proc/stat") {
            if let Some(cpu_line) = stat_content.lines().next() {
                let values: Vec<u64> = cpu_line
                    .split_whitespace()
                    .skip(1)
                    .filter_map(|s| s.parse().ok())
                    .collect();
                
                if values.len() >= 4 {
                    let idle = values[3];
                    let total: u64 = values.iter().sum();
                    if total > 0 {
                        stats.cpu_usage = ((total - idle) as f32 / total as f32) * 100.0;
                    }
                }
            }
        }
        
        // Get temperature from thermal zones
        let thermal_zone_path = Path::new("/sys/class/thermal");
        if thermal_zone_path.exists() {
            let mut max_temp: Option<f32> = None;
            
            if let Ok(entries) = fs::read_dir(thermal_zone_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with("thermal_zone"))
                        .unwrap_or(false)
                    {
                        let temp_path = path.join("temp");
                        if let Ok(temp_str) = fs::read_to_string(&temp_path) {
                            if let Ok(temp) = temp_str.trim().parse::<f32>() {
                                let temp_celsius = temp / 1000.0;
                                max_temp = Some(max_temp.unwrap_or(0.0).max(temp_celsius));
                            }
                        }
                    }
                }
            }
            
            stats.temperature = max_temp;
        }
        
        // Get power consumption from battery
        let power_supply_path = Path::new("/sys/class/power_supply");
        if power_supply_path.exists() {
            if let Ok(entries) = fs::read_dir(power_supply_path) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_str().unwrap_or("");
                    
                    if name_str.starts_with("BAT") {
                        let bat_path = entry.path();
                        
                        // Try to read power_now (in microwatts)
                        let power_now_path = bat_path.join("power_now");
                        if let Ok(power_str) = fs::read_to_string(&power_now_path) {
                            if let Ok(power_uw) = power_str.trim().parse::<f32>() {
                                stats.power_consumption = Some(power_uw / 1_000_000.0);
                                break;
                            }
                        }
                        
                        // Fallback: calculate from current and voltage
                        let current_now_path = bat_path.join("current_now");
                        let voltage_now_path = bat_path.join("voltage_now");
                        
                        if let (Ok(current_str), Ok(voltage_str)) = (
                            fs::read_to_string(&current_now_path),
                            fs::read_to_string(&voltage_now_path)
                        ) {
                            if let (Ok(current_ua), Ok(voltage_uv)) = (
                                current_str.trim().parse::<f32>(),
                                voltage_str.trim().parse::<f32>()
                            ) {
                                let power_w = (current_ua * voltage_uv) / 1_000_000_000_000.0;
                                stats.power_consumption = Some(power_w);
                                break;
                            }
                        }
                    }
                }
            }
        }
        
        stats
    }
    
    fn get_brightness() -> Option<u32> {
        // Try using brightnessctl first
        if let Ok(output) = Command::new("brightnessctl")
            .args(&["get"])
            .output()
        {
            if let Ok(current) = String::from_utf8_lossy(&output.stdout).trim().parse::<u32>() {
                // Get max brightness
                if let Ok(max_output) = Command::new("brightnessctl")
                    .args(&["max"])
                    .output()
                {
                    if let Ok(max) = String::from_utf8_lossy(&max_output.stdout).trim().parse::<u32>() {
                        return Some((current * 100) / max);
                    }
                }
            }
        }
        
        // Fallback to sysfs
        let backlight_path = Path::new("/sys/class/backlight");
        if backlight_path.exists() {
            if let Ok(entries) = fs::read_dir(backlight_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let brightness_path = path.join("brightness");
                    let max_brightness_path = path.join("max_brightness");
                    
                    if let (Ok(brightness_str), Ok(max_str)) = (
                        fs::read_to_string(&brightness_path),
                        fs::read_to_string(&max_brightness_path)
                    ) {
                        if let (Ok(brightness), Ok(max)) = (
                            brightness_str.trim().parse::<u32>(),
                            max_str.trim().parse::<u32>()
                        ) {
                            if max > 0 {
                                return Some((brightness * 100) / max);
                            }
                        }
                    }
                }
            }
        }
        
        None
    }
    
    fn set_brightness(percentage: u32) {
        // Try using brightnessctl first
        let _ = Command::new("brightnessctl")
            .args(&["set", &format!("{}%", percentage)])
            .spawn();
        
        // Fallback to sysfs (requires permissions)
        let backlight_path = Path::new("/sys/class/backlight");
        if backlight_path.exists() {
            if let Ok(entries) = fs::read_dir(backlight_path) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let max_brightness_path = path.join("max_brightness");
                    let brightness_path = path.join("brightness");
                    
                    if let Ok(max_str) = fs::read_to_string(&max_brightness_path) {
                        if let Ok(max) = max_str.trim().parse::<u32>() {
                            let new_brightness = (max * percentage) / 100;
                            let _ = fs::write(&brightness_path, new_brightness.to_string());
                        }
                    }
                }
            }
        }
    }
    
    fn check_power_profiles_daemon() -> bool {
        // Check if power-profiles-daemon is available
        Command::new("powerprofilesctl")
            .arg("version")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
    
    fn get_current_power_profile() -> Option<PowerProfile> {
        if let Ok(output) = Command::new("powerprofilesctl")
            .arg("get")
            .output()
        {
            let profile_str = String::from_utf8_lossy(&output.stdout);
            PowerProfile::from_string(&profile_str)
        } else {
            None
        }
    }
    
    fn set_power_profile(profile: PowerProfile) {
        let _ = Command::new("powerprofilesctl")
            .arg("set")
            .arg(profile.to_string())
            .spawn();
    }
    
    fn update_profile_selection(profiles_list: &ListBox) {
        if let Some(current_profile) = Self::get_current_power_profile() {
            let mut index = 0;
            while let Some(row) = profiles_list.row_at_index(index) {
                if let Some(child) = row.child() {
                    if let Some(hbox) = child.downcast_ref::<Box>() {
                        // Check if this row matches current profile
                        let row_profile = row.widget_name();
                        let is_selected = PowerProfile::from_string(&row_profile) == Some(current_profile.clone());
                        
                        // Show/hide check icon
                        if let Some(check_icon) = hbox.last_child() {
                            check_icon.set_visible(is_selected);
                        }
                        
                        // Update row style
                        if is_selected {
                            row.add_css_class("battery-profile-selected");
                        } else {
                            row.remove_css_class("battery-profile-selected");
                        }
                    }
                }
                index += 1;
            }
        }
    }
    
    fn get_battery_info() -> Option<BatteryInfo> {
        // Try to find battery in /sys/class/power_supply/
        let power_supply_path = Path::new("/sys/class/power_supply");
        
        if !power_supply_path.exists() {
            return None;
        }
        
        // Look for BAT0, BAT1, etc.
        for entry in fs::read_dir(power_supply_path).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name();
            let name_str = name.to_str()?;
            
            if name_str.starts_with("BAT") {
                let bat_path = entry.path();
                
                // Read capacity
                let capacity_path = bat_path.join("capacity");
                let capacity = fs::read_to_string(capacity_path)
                    .ok()?
                    .trim()
                    .parse::<u32>()
                    .ok()?;
                
                // Read status
                let status_path = bat_path.join("status");
                let status = fs::read_to_string(status_path)
                    .ok()?
                    .trim()
                    .to_string();
                
                let charging = status == "Charging";
                let plugged = status == "Charging" || status == "Full";
                
                // Try to read time estimates
                let time_to_empty = if !charging && !plugged {
                    Self::read_time_estimate(&bat_path, "time_to_empty")
                } else {
                    None
                };
                
                let time_to_full = if charging {
                    Self::read_time_estimate(&bat_path, "time_to_full")
                } else {
                    None
                };
                
                return Some(BatteryInfo {
                    percentage: capacity,
                    charging,
                    plugged,
                    time_to_empty,
                    time_to_full,
                });
            }
        }
        
        None
    }
    
    fn read_time_estimate(bat_path: &Path, file_name: &str) -> Option<String> {
        let time_path = bat_path.join(file_name);
        if let Ok(time_str) = fs::read_to_string(time_path) {
            if let Ok(minutes) = time_str.trim().parse::<u32>() {
                if minutes > 0 && minutes < 1440 { // Less than 24 hours
                    let hours = minutes / 60;
                    let mins = minutes % 60;
                    if hours > 0 {
                        return Some(format!("{} hr {} min remaining", hours, mins));
                    } else {
                        return Some(format!("{} min remaining", mins));
                    }
                }
            }
        }
        None
    }
    
    fn get_battery_icon_name(info: &BatteryInfo) -> String {
        let level = match info.percentage {
            0..=10 => "empty",
            11..=30 => "caution",
            31..=50 => "low",
            51..=80 => "good",
            _ => "full",
        };
        
        if info.charging {
            format!("battery-{}-charging-symbolic", level)
        } else {
            format!("battery-{}-symbolic", level)
        }
    }
    
    fn open_power_settings() {
        // Try different power settings commands
        let commands = vec![
            ("gnome-control-center", vec!["power"]),
            ("xfce4-power-manager-settings", vec![]),
            ("mate-power-preferences", vec![]),
        ];
        
        for (cmd, args) in commands {
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }
        
        warn!("Could not find power settings application");
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}