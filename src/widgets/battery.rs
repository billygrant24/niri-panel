use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, ListBox, ListBoxRow};
use glib::timeout_add_seconds_local;
use anyhow::Result;
use std::fs;
use std::path::Path;
use std::process::Command;
use tracing::{info, warn};

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
        popover_box.set_size_request(250, -1);
        
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
        
        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        popover_box.append(&separator);
        
        // Power profiles section
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
            let popover_weak = popover.downgrade();
            profiles_list.connect_row_activated(move |list, row| {
                let profile_name = row.widget_name();
                if let Some(profile) = PowerProfile::from_string(&profile_name) {
                    Self::set_power_profile(profile);
                    
                    // Update UI to show selected profile
                    Self::update_profile_selection(list);
                    
                    // Close popover after a short delay
                    if let Some(popover) = popover_weak.upgrade() {
                        glib::timeout_add_local_once(std::time::Duration::from_millis(200), move || {
                            popover.popdown();
                        });
                    }
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
        Self::update_battery(&icon, &label, &status_label, &time_label, &profiles_list);
        
        // Update every 30 seconds
        let icon_weak = icon.downgrade();
        let label_weak = label.downgrade();
        let status_label_weak = status_label.downgrade();
        let time_label_weak = time_label.downgrade();
        let profiles_list_weak = profiles_list.downgrade();
        timeout_add_seconds_local(30, move || {
            if let (Some(icon), Some(label), Some(status), Some(time), Some(profiles)) = 
                (icon_weak.upgrade(), label_weak.upgrade(), status_label_weak.upgrade(), 
                 time_label_weak.upgrade(), profiles_list_weak.upgrade()) {
                Self::update_battery(&icon, &label, &status, &time, &profiles);
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
    
    fn update_battery(icon: &Image, label: &Label, status_label: &Label, time_label: &Label, profiles_list: &ListBox) {
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
        
        // Update power profile selection
        Self::update_profile_selection(profiles_list);
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
