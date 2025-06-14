use gtk4::prelude::*;
use gtk4::{Button, Image, Popover, Box, Orientation, Label, ApplicationWindow};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use anyhow::Result;
use std::process::Command;
use std::fs;
use std::time::Duration;
use std::rc::Rc;
use std::cell::RefCell;
use tracing::info;

pub struct Power {
    button: Button,
}

#[derive(Debug, Clone)]
enum PowerAction {
    Lock,
    Logout,
    Sleep,
    Reboot,
    Shutdown,
}

#[derive(Debug, Clone)]
struct SystemStats {
    os: String,
    kernel: String,
    hostname: String,
    cpu_model: String,
    uptime: String,
    cpu_usage: f32,
    memory_usage: (u64, u64), // (used, total) in MB
    disk_usage: (u64, u64),    // (used, total) in GB
    packages: String,
}

impl PowerAction {
    fn label(&self) -> &str {
        match self {
            PowerAction::Lock => "Lock",
            PowerAction::Logout => "Log Out",
            PowerAction::Sleep => "Sleep",
            PowerAction::Reboot => "Restart",
            PowerAction::Shutdown => "Shut Down",
        }
    }
    
    fn icon(&self) -> &str {
        match self {
            PowerAction::Lock => "system-lock-screen-symbolic",
            PowerAction::Logout => "system-log-out-symbolic",
            PowerAction::Sleep => "system-suspend-symbolic",
            PowerAction::Reboot => "system-reboot-symbolic",
            PowerAction::Shutdown => "system-shutdown-symbolic",
        }
    }
    
    fn execute(&self) {
        match self {
            PowerAction::Lock => {
                // Try swaylock first, then other lock screens
                if Command::new("swaylock")
                    .args(&["-c", "2e3440", "-f"])
                    .spawn()
                    .is_err()
                {
                    // Try other lock screens
                    let _ = Command::new("loginctl").arg("lock-session").spawn();
                }
            }
            PowerAction::Logout => {
                let user = std::env::var("USER").unwrap_or_default();
                let _ = Command::new("loginctl")
                    .args(&["kill-user", &user])
                    .spawn();
            }
            PowerAction::Sleep => {
                // Lock first
                if Command::new("swaylock")
                    .args(&["-c", "2e3440", "-f"])
                    .spawn()
                    .is_ok()
                {
                    // Wait a moment for lock to engage
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                
                // Then hibernate
                let _ = Command::new("systemctl").arg("hibernate").spawn();
            }
            PowerAction::Reboot => {
                let _ = Command::new("systemctl").arg("reboot").spawn();
            }
            PowerAction::Shutdown => {
                let _ = Command::new("systemctl").arg("poweroff").spawn();
            }
        }
    }
    
    fn needs_confirmation(&self) -> bool {
        match self {
            PowerAction::Lock => false,
            _ => true,
        }
    }
}

impl Power {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("power");
        
        // Try multiple power icon fallbacks
        let icon_names = vec![
            "system-shutdown-symbolic",
            "application-exit-symbolic",
            "system-power-symbolic",
            "gtk-quit"
        ];
        
        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                break;
            }
        }
        
        if image.icon_name().is_none() {
            let label = Label::new(Some("⏻"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }
        
        // Create popover for power menu
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("power-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        
        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Power popover shown - keyboard mode set to OnDemand (active popovers: {})", 
                      *active_popovers_show.borrow());
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
                    info!("Power popover hidden - keyboard mode set to None");
                }
            } else {
                info!("Power popover hidden - keeping keyboard mode (active popovers: {})", count);
            }
        });
        
        let popover_box = Box::new(Orientation::Vertical, 0);
        popover_box.set_margin_top(5);
        popover_box.set_margin_bottom(5);
        popover_box.set_size_request(350, -1);
        
        // User info section
        let user_box = Box::new(Orientation::Horizontal, 10);
        user_box.set_margin_start(15);
        user_box.set_margin_end(15);
        user_box.set_margin_top(10);
        user_box.set_margin_bottom(10);
        
        let user_icon = Image::from_icon_name("avatar-default-symbolic");
        user_icon.set_pixel_size(32);
        user_box.append(&user_icon);
        
        let user_label = Label::new(Some(&std::env::var("USER").unwrap_or_else(|_| "User".to_string())));
        user_label.add_css_class("power-user-label");
        user_label.set_halign(gtk4::Align::Start);
        user_box.append(&user_label);
        
        popover_box.append(&user_box);
        
        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        popover_box.append(&separator);
        
        // System Stats Section
        let stats_box = Box::new(Orientation::Vertical, 8);
        stats_box.set_margin_start(15);
        stats_box.set_margin_end(15);
        stats_box.set_margin_top(10);
        stats_box.set_margin_bottom(10);
        stats_box.add_css_class("power-stats-box");
        
        // Stats title
        let stats_title = Label::new(Some("System Information"));
        stats_title.add_css_class("power-stats-title");
        stats_title.set_halign(gtk4::Align::Start);
        stats_box.append(&stats_title);
        
        // Create stat labels
        let os_label = Self::create_stat_label("OS", "Loading...");
        let kernel_label = Self::create_stat_label("Kernel", "Loading...");
        let hostname_label = Self::create_stat_label("Hostname", "Loading...");
        let cpu_model_label = Self::create_stat_label("CPU", "Loading...");
        let uptime_label = Self::create_stat_label("Uptime", "0m");
        let cpu_label = Self::create_stat_label("CPU Usage", "0%");
        let memory_label = Self::create_stat_label("Memory", "0 MB / 0 MB");
        let disk_label = Self::create_stat_label("Disk (/)", "0 GB / 0 GB");
        let packages_label = Self::create_stat_label("Packages", "Loading...");
        
        stats_box.append(&os_label);
        stats_box.append(&kernel_label);
        stats_box.append(&hostname_label);
        stats_box.append(&cpu_model_label);
        stats_box.append(&uptime_label);
        stats_box.append(&cpu_label);
        stats_box.append(&memory_label);
        stats_box.append(&disk_label);
        stats_box.append(&packages_label);
        
        popover_box.append(&stats_box);
        
        // Update stats immediately and schedule updates
        Self::update_stats(&os_label, &kernel_label, &hostname_label, &cpu_model_label,
                          &uptime_label, &cpu_label, &memory_label, &disk_label,
                          &packages_label);
        
        let os_weak = os_label.downgrade();
        let kernel_weak = kernel_label.downgrade();
        let hostname_weak = hostname_label.downgrade();
        let cpu_model_weak = cpu_model_label.downgrade();
        let uptime_weak = uptime_label.downgrade();
        let cpu_weak = cpu_label.downgrade();
        let memory_weak = memory_label.downgrade();
        let disk_weak = disk_label.downgrade();
        let packages_weak = packages_label.downgrade();
        let popover_weak = popover.downgrade();
        
        glib::timeout_add_local(Duration::from_secs(2), move || {
            if let Some(popover) = popover_weak.upgrade() {
                // Only update if popover is visible
                if popover.is_visible() {
                    if let (Some(os), Some(kernel), Some(hostname), Some(cpu_model),
                            Some(uptime), Some(cpu), Some(memory), Some(disk),
                            Some(packages)) = 
                        (os_weak.upgrade(), kernel_weak.upgrade(), hostname_weak.upgrade(),
                         cpu_model_weak.upgrade(), uptime_weak.upgrade(), cpu_weak.upgrade(),
                         memory_weak.upgrade(), disk_weak.upgrade(),
                         packages_weak.upgrade()) {
                        Self::update_stats(&os, &kernel, &hostname, &cpu_model,
                                         &uptime, &cpu, &memory, &disk,
                                         &packages);
                    }
                }
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        
        // Separator
        let separator2 = gtk4::Separator::new(Orientation::Horizontal);
        popover_box.append(&separator2);
        
        // Power actions - horizontal layout
        let actions_box = Box::new(Orientation::Horizontal, 0);
        actions_box.set_margin_start(10);
        actions_box.set_margin_end(10);
        actions_box.set_margin_top(10);
        actions_box.set_margin_bottom(10);
        actions_box.set_homogeneous(true);
        actions_box.add_css_class("power-actions-box");
        
        let actions = vec![
            PowerAction::Lock,
            PowerAction::Logout,
            PowerAction::Sleep,
            PowerAction::Reboot,
            PowerAction::Shutdown,
        ];
        
        for action in actions {
            let button = Self::create_action_button(action, popover.downgrade());
            actions_box.append(&button);
        }
        
        popover_box.append(&actions_box);
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
        button.connect_clicked(move |_| {
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    fn create_stat_label(title: &str, initial_value: &str) -> Box {
        let hbox = Box::new(Orientation::Horizontal, 0);
        
        let title_label = Label::new(Some(title));
        title_label.add_css_class("power-stat-title");
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_hexpand(true);
        
        let value_label = Label::new(Some(initial_value));
        value_label.add_css_class("power-stat-value");
        value_label.set_halign(gtk4::Align::End);
        
        hbox.append(&title_label);
        hbox.append(&value_label);
        
        hbox
    }
    
    fn update_stats(os_box: &Box, kernel_box: &Box, hostname_box: &Box, cpu_model_box: &Box,
                    uptime_box: &Box, cpu_box: &Box, memory_box: &Box, disk_box: &Box,
                    packages_box: &Box) {
        let stats = Self::get_system_stats();
        
        // Update OS
        if let Some(value_label) = os_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&stats.os);
            }
        }
        
        // Update Kernel
        if let Some(value_label) = kernel_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&stats.kernel);
            }
        }
        
        // Update Hostname
        if let Some(value_label) = hostname_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&stats.hostname);
            }
        }
        
        // Update CPU Model
        if let Some(value_label) = cpu_model_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&stats.cpu_model);
            }
        }
        
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
        
        // Update Memory
        if let Some(value_label) = memory_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                let (used_gb, total_gb) = (stats.memory_usage.0 as f32 / 1024.0, stats.memory_usage.1 as f32 / 1024.0);
                label.set_text(&format!("{:.1} GB / {:.1} GB", used_gb, total_gb));
            }
        }
        
        // Update Disk
        if let Some(value_label) = disk_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&format!("{} GB / {} GB", stats.disk_usage.0, stats.disk_usage.1));
            }
        }
        
        // Update Packages
        if let Some(value_label) = packages_box.last_child() {
            if let Some(label) = value_label.downcast_ref::<Label>() {
                label.set_text(&stats.packages);
            }
        }
    }
    
    fn get_system_stats() -> SystemStats {
        let mut stats = SystemStats {
            os: "Unknown".to_string(),
            kernel: "Unknown".to_string(),
            hostname: "Unknown".to_string(),
            cpu_model: "Unknown".to_string(),
            uptime: "Unknown".to_string(),
            cpu_usage: 0.0,
            memory_usage: (0, 0),
            disk_usage: (0, 0),
            packages: "Unknown".to_string(),
        };
        
        // Get OS information
        if let Ok(os_release) = fs::read_to_string("/etc/os-release") {
            for line in os_release.lines() {
                if line.starts_with("PRETTY_NAME=") {
                    stats.os = line.trim_start_matches("PRETTY_NAME=")
                        .trim_matches('"')
                        .to_string();
                    break;
                }
            }
        }
        
        // Get kernel version
        if let Ok(kernel) = fs::read_to_string("/proc/version") {
            if let Some(kernel_version) = kernel.split_whitespace().nth(2) {
                stats.kernel = kernel_version.to_string();
            }
        }
        
        // Get hostname
        if let Ok(hostname) = fs::read_to_string("/etc/hostname") {
            stats.hostname = hostname.trim().to_string();
        }
        
        // Get CPU model
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
            for line in cpuinfo.lines() {
                if line.starts_with("model name") {
                    if let Some(model) = line.split(':').nth(1) {
                        stats.cpu_model = model.trim().to_string();
                        break;
                    }
                }
            }
        }
        
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
        
        // Get memory usage from /proc/meminfo
        if let Ok(meminfo) = fs::read_to_string("/proc/meminfo") {
            let mut mem_total = 0u64;
            let mut mem_available = 0u64;
            
            for line in meminfo.lines() {
                if line.starts_with("MemTotal:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        mem_total = value.parse().unwrap_or(0) / 1024; // Convert KB to MB
                    }
                } else if line.starts_with("MemAvailable:") {
                    if let Some(value) = line.split_whitespace().nth(1) {
                        mem_available = value.parse().unwrap_or(0) / 1024; // Convert KB to MB
                    }
                }
            }
            
            let mem_used = mem_total.saturating_sub(mem_available);
            stats.memory_usage = (mem_used, mem_total);
        }
        
        // Get disk usage for root filesystem
        if let Ok(output) = Command::new("df")
            .args(&["-BG", "/"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = output_str.lines().nth(1) {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    // Remove 'G' suffix and parse
                    let total = parts[1].trim_end_matches('G').parse().unwrap_or(0);
                    let used = parts[2].trim_end_matches('G').parse().unwrap_or(0);
                    stats.disk_usage = (used, total);
                }
            }
        }
        
        // Count packages - try multiple package managers
        let mut package_count = 0;
        let mut package_managers = Vec::new();
        
        // Check for Nix packages
        if let Ok(output) = Command::new("nix-env")
            .args(&["-q"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let count = output_str.lines().count();
            if count > 0 {
                package_count += count;
                package_managers.push("nix");
            }
        }
        
        // Check for system packages on NixOS
        if let Ok(output) = Command::new("nix-store")
            .args(&["-q", "--requisites", "/run/current-system"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let count = output_str.lines().count();
            if count > 0 {
                package_count += count;
                package_managers.push("nixos");
            }
        }
        
        // Check for other package managers
        if let Ok(output) = Command::new("dpkg")
            .args(&["-l"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let count = output_str.lines().filter(|line| line.starts_with("ii")).count();
            if count > 0 {
                package_count += count;
                package_managers.push("dpkg");
            }
        } else if let Ok(output) = Command::new("rpm")
            .args(&["-qa"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let count = output_str.lines().count();
            if count > 0 {
                package_count += count;
                package_managers.push("rpm");
            }
        } else if let Ok(output) = Command::new("pacman")
            .args(&["-Q"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let count = output_str.lines().count();
            if count > 0 {
                package_count += count;
                package_managers.push("pacman");
            }
        }
        
        if package_count > 0 {
            stats.packages = format!("{} ({})", package_count, package_managers.join(", "));
        }
        
        stats
    }
    
    fn create_action_button(action: PowerAction, popover_weak: gtk4::glib::WeakRef<Popover>) -> Button {
        let button = Button::new();
        button.add_css_class("power-action-button");
        
        let vbox = Box::new(Orientation::Vertical, 4);
        vbox.set_margin_top(8);
        vbox.set_margin_bottom(8);
        
        let icon = Image::from_icon_name(action.icon());
        icon.set_pixel_size(24);
        vbox.append(&icon);
        
        let label = Label::new(Some(action.label()));
        label.add_css_class("power-action-label");
        vbox.append(&label);
        
        button.set_child(Some(&vbox));
        
        button.connect_clicked(move |_| {
            if action.needs_confirmation() {
                Self::show_confirmation_dialog(action.clone(), popover_weak.clone());
            } else {
                action.execute();
                if let Some(popover) = popover_weak.upgrade() {
                    popover.popdown();
                }
            }
        });
        
        button
    }
    
    fn show_confirmation_dialog(action: PowerAction, popover_weak: gtk4::glib::WeakRef<Popover>) {
        // Close the main popover first
        if let Some(main_popover) = popover_weak.upgrade() {
            main_popover.popdown();
        }
        
        // Create a new window for confirmation
        let dialog = gtk4::Window::new();
        dialog.set_title(Some("Confirm Action"));
        dialog.set_modal(true);
        dialog.set_resizable(false);
        dialog.set_decorated(false);
        dialog.add_css_class("power-confirm-dialog");
        
        // Center the window on screen
        dialog.set_default_size(300, 150);
        
        let confirm_box = Box::new(Orientation::Vertical, 20);
        confirm_box.set_margin_top(30);
        confirm_box.set_margin_bottom(30);
        confirm_box.set_margin_start(30);
        confirm_box.set_margin_end(30);
        
        // Icon and message
        let icon = Image::from_icon_name(action.icon());
        icon.set_pixel_size(48);
        confirm_box.append(&icon);
        
        // Confirmation message
        let message = Label::new(Some(&format!("Are you sure you want to {}?", action.label().to_lowercase())));
        message.add_css_class("power-confirm-message");
        message.set_wrap(true);
        confirm_box.append(&message);
        
        // Buttons
        let button_box = Box::new(Orientation::Horizontal, 10);
        button_box.set_halign(gtk4::Align::Center);
        button_box.set_margin_top(10);
        
        let cancel_button = Button::with_label("Cancel");
        cancel_button.add_css_class("power-confirm-cancel");
        
        let confirm_button = Button::with_label("Confirm");
        confirm_button.add_css_class("power-confirm-button");
        
        match &action {
            PowerAction::Shutdown | PowerAction::Reboot => {
                confirm_button.add_css_class("destructive-action");
            }
            _ => {}
        }
        
        button_box.append(&cancel_button);
        button_box.append(&confirm_button);
        confirm_box.append(&button_box);
        
        dialog.set_child(Some(&confirm_box));
        
        // Handle button clicks
        let dialog_weak = dialog.downgrade();
        cancel_button.connect_clicked(move |_| {
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });
        
        let dialog_weak2 = dialog.downgrade();
        confirm_button.connect_clicked(move |_| {
            action.execute();
            if let Some(dialog) = dialog_weak2.upgrade() {
                dialog.close();
            }
        });
        
        // Handle Escape key
        let controller = gtk4::EventControllerKey::new();
        let dialog_weak3 = dialog.downgrade();
        controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(dialog) = dialog_weak3.upgrade() {
                    dialog.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        dialog.add_controller(controller);
        
        // Show the dialog centered
        dialog.present();
        
        // Focus the cancel button by default
        cancel_button.grab_focus();
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}