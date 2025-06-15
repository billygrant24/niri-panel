use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, ListBox, ListBoxRow, Switch, ScrolledWindow, ApplicationWindow, Separator, Spinner};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use anyhow::Result;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::rc::Rc;
use std::cell::RefCell;
use tracing::{info, warn, error};
use glib::{timeout_add_local, ControlFlow};
use std::thread;
use std::sync::{Arc, Mutex};

pub struct Bluetooth {
    button: Button,
    device_cache: Arc<Mutex<Vec<BluetoothDevice>>>,
    last_scan: Arc<Mutex<Instant>>,
    is_powered: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone)]
struct BluetoothDevice {
    address: String,
    name: String,
    icon: String,
    connected: bool,
    paired: bool,
    trusted: bool,
    blocked: bool,
    battery_percentage: Option<u8>,
    device_type: DeviceType,
}

#[derive(Debug, Clone, PartialEq)]
enum DeviceType {
    Computer,
    Phone,
    AudioHeadset,
    AudioHeadphones,
    AudioSpeaker,
    Keyboard,
    Mouse,
    GameController,
    Unknown,
}

impl DeviceType {
    fn from_icon(icon: &str) -> Self {
        match icon {
            s if s.contains("computer") => DeviceType::Computer,
            s if s.contains("phone") => DeviceType::Phone,
            s if s.contains("headset") => DeviceType::AudioHeadset,
            s if s.contains("headphones") => DeviceType::AudioHeadphones,
            s if s.contains("speaker") || s.contains("audio") => DeviceType::AudioSpeaker,
            s if s.contains("keyboard") => DeviceType::Keyboard,
            s if s.contains("mouse") => DeviceType::Mouse,
            s if s.contains("game") || s.contains("controller") => DeviceType::GameController,
            _ => DeviceType::Unknown,
        }
    }
    
    fn icon_name(&self) -> &'static str {
        match self {
            DeviceType::Computer => "computer-symbolic",
            DeviceType::Phone => "phone-symbolic",
            DeviceType::AudioHeadset => "audio-headset-symbolic",
            DeviceType::AudioHeadphones => "audio-headphones-symbolic",
            DeviceType::AudioSpeaker => "audio-speakers-symbolic",
            DeviceType::Keyboard => "input-keyboard-symbolic",
            DeviceType::Mouse => "input-mouse-symbolic",
            DeviceType::GameController => "input-gaming-symbolic",
            DeviceType::Unknown => "bluetooth-symbolic",
        }
    }
}

impl Bluetooth {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("bluetooth");
        
        let container = Box::new(Orientation::Horizontal, 5);
        
        let icon = Image::from_icon_name("bluetooth-symbolic");
        icon.set_icon_size(gtk4::IconSize::Large);
        
        let status_label = Label::new(None);
        status_label.add_css_class("bluetooth-status");
        status_label.set_visible(false); // Hidden by default
        
        // Initialize caches
        let device_cache = Arc::new(Mutex::new(Vec::new()));
        let last_scan = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(3600)));
        let is_powered = Arc::new(Mutex::new(false));
        
        container.append(&icon);
        container.append(&status_label);
        button.set_child(Some(&container));
        
        // Create popover
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("bluetooth-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        
        // Handle popover show/hide for keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Bluetooth popover shown - keyboard mode set to OnDemand");
            }
        });
        
        let window_weak_hide = window_weak.clone();
        let active_popovers_hide = active_popovers.clone();
        popover.connect_hide(move |_| {
            *active_popovers_hide.borrow_mut() -= 1;
            if *active_popovers_hide.borrow() == 0 {
                if let Some(window) = window_weak_hide.upgrade() {
                    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
                    info!("Bluetooth popover hidden - keyboard mode set to None");
                }
            }
        });
        
        let main_box = Box::new(Orientation::Vertical, 10);
        main_box.set_margin_top(15);
        main_box.set_margin_bottom(15);
        main_box.set_margin_start(15);
        main_box.set_margin_end(15);
        main_box.set_size_request(350, -1);
        
        // Bluetooth toggle
        let toggle_box = Box::new(Orientation::Horizontal, 10);
        let toggle_label = Label::new(Some("Bluetooth"));
        toggle_label.set_hexpand(true);
        toggle_label.set_halign(gtk4::Align::Start);
        toggle_label.add_css_class("bluetooth-toggle-label");
        
        let power_switch = Switch::new();
        power_switch.set_valign(gtk4::Align::Center);
        
        toggle_box.append(&toggle_label);
        toggle_box.append(&power_switch);
        main_box.append(&toggle_box);
        
        // Separator
        let separator = Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        main_box.append(&separator);
        
        // Devices section header
        let devices_header = Box::new(Orientation::Horizontal, 10);
        
        let devices_label = Label::new(Some("Devices"));
        devices_label.set_halign(gtk4::Align::Start);
        devices_label.set_hexpand(true);
        devices_label.add_css_class("bluetooth-section-title");
        devices_header.append(&devices_label);
        
        let scan_spinner = Spinner::new();
        scan_spinner.set_visible(false);
        devices_header.append(&scan_spinner);
        
        let scan_button = Button::from_icon_name("view-refresh-symbolic");
        scan_button.add_css_class("bluetooth-scan-button");
        scan_button.set_tooltip_text(Some("Scan for devices"));
        devices_header.append(&scan_button);
        
        main_box.append(&devices_header);
        
        // Device list
        let device_scroll = ScrolledWindow::new();
        device_scroll.set_vexpand(true);
        device_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        device_scroll.set_min_content_height(200);
        device_scroll.set_max_content_height(400);
        
        let device_list = ListBox::new();
        device_list.add_css_class("bluetooth-device-list");
        device_list.set_selection_mode(gtk4::SelectionMode::None);
        device_scroll.set_child(Some(&device_list));
        
        main_box.append(&device_scroll);
        
        // Settings button
        let settings_button = Button::with_label("Bluetooth Settings");
        settings_button.set_margin_top(10);
        settings_button.connect_clicked(|_| {
            Self::open_bluetooth_settings();
        });
        main_box.append(&settings_button);
        
        popover.set_child(Some(&main_box));
        
        // Check if Bluetooth is available (in background)
        let bluetooth_available = Self::check_bluetooth_available();
        
        if !bluetooth_available {
            // Show error state
            power_switch.set_sensitive(false);
            scan_button.set_sensitive(false);
            
            let error_row = ListBoxRow::new();
            let error_label = Label::new(Some("Bluetooth not available"));
            error_label.add_css_class("dim-label");
            error_label.set_margin_top(20);
            error_label.set_margin_bottom(20);
            error_row.set_child(Some(&error_label));
            device_list.append(&error_row);
            
            icon.add_css_class("bluetooth-disabled");
        } else {
            // Set initial Bluetooth state (asynchronously)
            let is_powered_arc = is_powered.clone();
            let power_switch_weak = power_switch.downgrade();
            let icon_weak = icon.downgrade();
            
            Self::run_in_background(move || {
                let powered = Self::get_bluetooth_powered();
                *is_powered_arc.lock().unwrap() = powered;
                
                glib::idle_add_local(move || {
                    if let (Some(switch), Some(icon)) = (power_switch_weak.upgrade(), icon_weak.upgrade()) {
                        switch.set_active(powered);
                        Self::update_icon(&icon, powered, 0);
                    }
                    ControlFlow::Break
                });
            });
            
            // Load initial devices (asynchronously)
            let device_cache_init = device_cache.clone();
            let device_list_weak = device_list.downgrade();
            let is_powered_init = is_powered.clone();
            
            Self::run_in_background(move || {
                if *is_powered_init.lock().unwrap() {
                    let devices = Self::get_bluetooth_devices();
                    *device_cache_init.lock().unwrap() = devices.clone();
                    
                    glib::idle_add_local(move || {
                        if let Some(list) = device_list_weak.upgrade() {
                            Self::update_device_list_ui(&list, &devices);
                        }
                        ControlFlow::Break
                    });
                }
            });
            
            // Handle power toggle
            let icon_weak = icon.downgrade();
            let device_list_weak = device_list.downgrade();
            let scan_button_weak = scan_button.downgrade();
            let is_powered_toggle = is_powered.clone();
            let device_cache_toggle = device_cache.clone();
            
            power_switch.connect_state_set(move |_switch, state| {
                // Update the cached state immediately
                *is_powered_toggle.lock().unwrap() = state;
                
                // Update UI
                if let (Some(icon), Some(list), Some(scan)) = 
                    (icon_weak.upgrade(), device_list_weak.upgrade(), scan_button_weak.upgrade()) {
                    Self::update_icon(&icon, state, 0);
                    scan.set_sensitive(state);
                    
                    if !state {
                        // Clear device list
                        while let Some(child) = list.first_child() {
                            list.remove(&child);
                        }
                        
                        let disabled_row = ListBoxRow::new();
                        let disabled_label = Label::new(Some("Bluetooth is turned off"));
                        disabled_label.add_css_class("dim-label");
                        disabled_label.set_margin_top(20);
                        disabled_label.set_margin_bottom(20);
                        disabled_row.set_child(Some(&disabled_label));
                        list.append(&disabled_row);
                    }
                }
                
                // Actually change power state in background
                let is_powered_bg = is_powered_toggle.clone();
                let device_cache_bg = device_cache_toggle.clone();
                let device_list_clone = device_list_weak.clone();
                
                Self::run_in_background(move || {
                    Self::set_bluetooth_power(state);
                    
                    if state {
                        // Wait a moment for Bluetooth to power on
                        thread::sleep(Duration::from_millis(500));
                        
                        // Update device list
                        let devices = Self::get_bluetooth_devices();
                        *device_cache_bg.lock().unwrap() = devices.clone();
                        
                        glib::idle_add_local(move || {
                            if let Some(list) = device_list_clone.upgrade() {
                                Self::update_device_list_ui(&list, &devices);
                            }
                            ControlFlow::Break
                        });
                    }
                });
                
                glib::Propagation::Proceed
            });
            
            // Handle scan button
            let device_list_for_scan = device_list.downgrade();
            let spinner_weak = scan_spinner.downgrade();
            let scan_button_weak = scan_button.downgrade();
            let device_cache_scan = device_cache.clone();
            let last_scan_scan = last_scan.clone();
            
            scan_button.connect_clicked(move |_| {
                if let (Some(list), Some(spinner), Some(button)) = 
                    (device_list_for_scan.upgrade(), spinner_weak.upgrade(), scan_button_weak.upgrade()) {
                    
                    // Show spinner and disable button
                    spinner.set_visible(true);
                    spinner.start();
                    button.set_sensitive(false);
                    
                    // Update last scan time
                    *last_scan_scan.lock().unwrap() = Instant::now();
                    
                    // Start scan in background
                    let device_cache_bg = device_cache_scan.clone();
                    let list_clone = list.clone();
                    let spinner_clone = spinner.clone();
                    let button_clone = button.clone();
                    
                    Self::run_in_background(move || {
                        // Start scan
                        Self::start_device_scan();
                        
                        // Scan for 8 seconds
                        thread::sleep(Duration::from_secs(8));
                        
                        // Stop scan
                        Self::stop_device_scan();
                        
                        // Get updated device list
                        let devices = Self::get_bluetooth_devices();
                        *device_cache_bg.lock().unwrap() = devices.clone();
                        
                        // Update UI
                        let devices_clone = devices.clone();
                        glib::idle_add_local(move || {
                            Self::update_device_list_ui(&list_clone, &devices_clone);
                            spinner_clone.stop();
                            spinner_clone.set_visible(false);
                            button_clone.set_sensitive(true);
                            ControlFlow::Break
                        });
                    });
                }
            });
            
            // Set up periodic updates when popover is visible
            let device_list_weak = device_list.downgrade();
            let icon_weak = icon.downgrade();
            let status_label_weak = status_label.downgrade();
            let popover_weak = popover.downgrade();
            let device_cache_periodic = device_cache.clone();
            let is_powered_periodic = is_powered.clone();
            let last_scan_periodic = last_scan.clone();
            
            // Use a longer interval (5 seconds) to reduce CPU usage
            timeout_add_local(Duration::from_secs(5), move || {
                if let Some(popover) = popover_weak.upgrade() {
                    // Get cached power state
                    let powered = *is_powered_periodic.lock().unwrap();
                    
                    // Update devices asynchronously if popover is visible and powered
                    if popover.is_visible() && powered {
                        let device_cache_bg = device_cache_periodic.clone();
                        let device_list_clone = device_list_weak.clone();
                        let last_scan_check = last_scan_periodic.clone();
                        
                        Self::run_in_background(move || {
                            // Only do a full refresh if it's been at least 30 seconds since last scan
                            let do_full_refresh = last_scan_check.lock().unwrap().elapsed() > Duration::from_secs(30);
                            
                            // Get devices from Bluetooth
                            let devices = if do_full_refresh {
                                Self::get_bluetooth_devices()
                            } else {
                                // Just update connection status for existing devices
                                let mut cached = device_cache_bg.lock().unwrap().clone();
                                for device in &mut cached {
                                    device.connected = Self::is_device_connected(&device.address);
                                }
                                cached
                            };
                            
                            // Update cache
                            *device_cache_bg.lock().unwrap() = devices.clone();
                            
                            // Update UI
                            let devices_clone = devices.clone();
                            glib::idle_add_local(move || {
                                if let Some(list) = device_list_clone.upgrade() {
                                    Self::update_device_list_ui(&list, &devices_clone);
                                }
                                ControlFlow::Break
                            });
                        });
                    }
                    
                    // Always update icon to show connection status
                    let device_cache_icon = device_cache_periodic.clone();
                    let icon_clone = icon_weak.clone();
                    let label_clone = status_label_weak.clone();
                    
                    Self::run_in_background(move || {
                        // Calculate connected count from cache if possible
                        let connected_count = {
                            let cache = device_cache_icon.lock().unwrap();
                            if cache.is_empty() && powered {
                                Self::get_connected_device_count()
                            } else {
                                cache.iter().filter(|d| d.connected).count()
                            }
                        };
                        
                        // Update UI
                        glib::idle_add_local(move || {
                            if let (Some(icon), Some(label)) = (icon_clone.upgrade(), label_clone.upgrade()) {
                                Self::update_icon(&icon, powered, connected_count);
                                
                                // Update status label
                                if connected_count > 0 {
                                    label.set_text(&connected_count.to_string());
                                    label.set_visible(true);
                                } else {
                                    label.set_visible(false);
                                }
                            }
                            ControlFlow::Break
                        });
                    });
                    
                    ControlFlow::Continue
                } else {
                    ControlFlow::Break
                }
            });
        }
        
        // Handle Escape key
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
        
        Ok(Self { 
            button,
            device_cache,
            last_scan,
            is_powered,
        })
    }
    
    fn check_bluetooth_available() -> bool {
        // Check if bluetoothctl is available (with timeout)
        match Command::new("which")
            .arg("bluetoothctl")
            .output() {
                Ok(output) => output.status.success(),
                Err(_) => false
            }
    }
    
    fn get_bluetooth_powered() -> bool {
        match Command::new("bluetoothctl")
            .arg("show")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn() {
                Ok(child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(2);
                    
                    match child.wait_with_output() {
                        Ok(output) if start.elapsed() < timeout => {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            output_str.lines().any(|line| line.trim() == "Powered: yes")
                        }
                        _ => false
                    }
                }
                Err(_) => false
            }
    }
    
    fn set_bluetooth_power(enable: bool) {
        let power_cmd = if enable { "on" } else { "off" };
        match Command::new("bluetoothctl")
            .args(&["power", power_cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn() {
                Ok(mut child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(2);
                    
                    // Check if command completes within timeout
                    match child.wait() {
                        Ok(_) if start.elapsed() < timeout => {
                            // Command succeeded within timeout
                        }
                        _ => {
                            // Command timed out or failed, try to kill the process
                            let _ = child.kill();
                            error!("bluetoothctl power {} command timed out", power_cmd);
                        }
                    }
                }
                Err(e) => error!("Failed to execute bluetoothctl: {}", e)
            }
    }
    
    fn get_connected_device_count() -> usize {
        match Command::new("bluetoothctl")
            .arg("devices")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn() {
                Ok(child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(2);
                    
                    match child.wait_with_output() {
                        Ok(output) if start.elapsed() < timeout => {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            let mut connected_count = 0;
                            
                            for line in output_str.lines() {
                                if let Some(address) = line.split_whitespace().nth(1) {
                                    if Self::is_device_connected(address) {
                                        connected_count += 1;
                                    }
                                }
                            }
                            
                            connected_count
                        }
                        _ => 0
                    }
                }
                Err(_) => 0
            }
    }
    
    fn is_device_connected(address: &str) -> bool {
        match Command::new("bluetoothctl")
            .args(&["info", address])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn() {
                Ok(child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(1); // Shorter timeout for individual device check
                    
                    match child.wait_with_output() {
                        Ok(output) if start.elapsed() < timeout => {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            output_str.lines().any(|line| line.trim() == "Connected: yes")
                        }
                        _ => false
                    }
                }
                Err(_) => false
            }
    }
    
    fn get_bluetooth_devices() -> Vec<BluetoothDevice> {
        let mut devices = Vec::new();
        
        // Get all devices with timeout
        match Command::new("bluetoothctl")
            .arg("devices")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn() {
                Ok(child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(2);
                    
                    match child.wait_with_output() {
                        Ok(output) if start.elapsed() < timeout => {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            let mut device_addresses = Vec::new();
                            
                            // First pass: collect all addresses and names
                            for line in output_str.lines() {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 3 && parts[0] == "Device" {
                                    let address = parts[1].to_string();
                                    let name = parts[2..].join(" ");
                                    device_addresses.push((address, name));
                                }
                            }
                            
                            // Second pass: get device info with a limit on concurrent operations
                            for (address, name) in device_addresses {
                                // Add a small delay to avoid overloading bluetoothctl
                                thread::sleep(Duration::from_millis(50));
                                
                                if let Some(device) = Self::get_device_info(&address, name) {
                                    devices.push(device);
                                }
                            }
                        }
                        _ => {
                            error!("bluetoothctl devices command timed out");
                        }
                    }
                }
                Err(e) => error!("Failed to execute bluetoothctl: {}", e)
            }
        
        // Sort: connected first, then paired, then by name
        devices.sort_by(|a, b| {
            match (a.connected, b.connected) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => match (a.paired, b.paired) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.cmp(&b.name),
                }
            }
        });
        
        devices
    }
    
    fn get_device_info(address: &str, name: String) -> Option<BluetoothDevice> {
        match Command::new("bluetoothctl")
            .args(&["info", address])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn() {
                Ok(child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(1);
                    
                    match child.wait_with_output() {
                        Ok(output) if start.elapsed() < timeout => {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            
                            let mut device = BluetoothDevice {
                                address: address.to_string(),
                                name,
                                icon: String::new(),
                                connected: false,
                                paired: false,
                                trusted: false,
                                blocked: false,
                                battery_percentage: None,
                                device_type: DeviceType::Unknown,
                            };
                            
                            for line in output_str.lines() {
                                let line = line.trim();
                                if line.starts_with("Name:") {
                                    device.name = line.split(':').nth(1).unwrap_or("").trim().to_string();
                                } else if line.starts_with("Icon:") {
                                    device.icon = line.split(':').nth(1).unwrap_or("").trim().to_string();
                                    device.device_type = DeviceType::from_icon(&device.icon);
                                } else if line == "Connected: yes" {
                                    device.connected = true;
                                } else if line == "Paired: yes" {
                                    device.paired = true;
                                } else if line == "Trusted: yes" {
                                    device.trusted = true;
                                } else if line == "Blocked: yes" {
                                    device.blocked = true;
                                } else if line.starts_with("Battery Percentage:") {
                                    if let Some(value) = line.split(':').nth(1) {
                                        if let Some(percentage_str) = value.trim().strip_suffix("%)") {
                                            if let Some(percentage_str) = percentage_str.strip_prefix("0x") {
                                                if let Ok(hex_val) = u8::from_str_radix(percentage_str, 16) {
                                                    device.battery_percentage = Some(hex_val);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            
                            Some(device)
                        }
                        _ => {
                            error!("bluetoothctl info {} command timed out", address);
                            None
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to execute bluetoothctl: {}", e);
                    None
                }
            }
    }
    
    fn update_device_list_ui(device_list: &ListBox, devices: &[BluetoothDevice]) {
        // Clear existing items
        while let Some(child) = device_list.first_child() {
            device_list.remove(&child);
        }
        
        if devices.is_empty() {
            let empty_row = ListBoxRow::new();
            let empty_label = Label::new(Some("No devices found"));
            empty_label.add_css_class("dim-label");
            empty_label.set_margin_top(20);
            empty_label.set_margin_bottom(20);
            empty_row.set_child(Some(&empty_label));
            device_list.append(&empty_row);
        } else {
            // Group devices
            let mut connected_devices = Vec::new();
            let mut paired_devices = Vec::new();
            let mut available_devices = Vec::new();
            
            for device in devices {
                if device.blocked {
                    continue; // Skip blocked devices
                }
                
                if device.connected {
                    connected_devices.push(device.clone());
                } else if device.paired {
                    paired_devices.push(device.clone());
                } else {
                    available_devices.push(device.clone());
                }
            }
            
            // Add connected devices
            if !connected_devices.is_empty() {
                let header_row = ListBoxRow::new();
                header_row.set_selectable(false);
                let header_label = Label::new(Some("Connected"));
                header_label.add_css_class("bluetooth-group-header");
                header_label.set_halign(gtk4::Align::Start);
                header_label.set_margin_start(10);
                header_label.set_margin_top(10);
                header_label.set_margin_bottom(5);
                header_row.set_child(Some(&header_label));
                device_list.append(&header_row);
                
                for device in connected_devices {
                    let row = Self::create_device_row(&device);
                    device_list.append(&row);
                }
            }
            
            // Add paired devices
            if !paired_devices.is_empty() {
                let header_row = ListBoxRow::new();
                header_row.set_selectable(false);
                let header_label = Label::new(Some("Paired"));
                header_label.add_css_class("bluetooth-group-header");
                header_label.set_halign(gtk4::Align::Start);
                header_label.set_margin_start(10);
                header_label.set_margin_top(10);
                header_label.set_margin_bottom(5);
                header_row.set_child(Some(&header_label));
                device_list.append(&header_row);
                
                for device in paired_devices {
                    let row = Self::create_device_row(&device);
                    device_list.append(&row);
                }
            }
            
            // Add available devices
            if !available_devices.is_empty() {
                let header_row = ListBoxRow::new();
                header_row.set_selectable(false);
                let header_label = Label::new(Some("Available"));
                header_label.add_css_class("bluetooth-group-header");
                header_label.set_halign(gtk4::Align::Start);
                header_label.set_margin_start(10);
                header_label.set_margin_top(10);
                header_label.set_margin_bottom(5);
                header_row.set_child(Some(&header_label));
                device_list.append(&header_row);
                
                for device in available_devices {
                    let row = Self::create_device_row(&device);
                    device_list.append(&row);
                }
            }
        }
    }
    
    fn start_device_scan() {
        match Command::new("bluetoothctl")
            .args(&["scan", "on"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn() {
                Ok(_) => {
                    info!("Started Bluetooth scan");
                }
                Err(e) => error!("Failed to start Bluetooth scan: {}", e)
            }
    }
    
    fn stop_device_scan() {
        match Command::new("bluetoothctl")
            .args(&["scan", "off"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn() {
                Ok(mut child) => {
                    // Set a timeout
                    let start = Instant::now();
                    let timeout = Duration::from_secs(1);
                    
                    // Check if command completes within timeout
                    match child.wait() {
                        Ok(_) if start.elapsed() < timeout => {
                            info!("Stopped Bluetooth scan");
                        }
                        _ => {
                            // Command timed out or failed, try to kill the process
                            let _ = child.kill();
                            error!("bluetoothctl scan off command timed out");
                        }
                    }
                }
                Err(e) => error!("Failed to stop Bluetooth scan: {}", e)
            }
    }
    
    fn connect_device(address: String) {
        Self::run_in_background(move || {
            match Command::new("bluetoothctl")
                .args(&["connect", &address])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn() {
                    Ok(mut child) => {
                        // Set a timeout
                        let start = Instant::now();
                        let timeout = Duration::from_secs(10); // Connecting can take longer
                        
                        // Check if command completes within timeout
                        match child.wait() {
                            Ok(_) if start.elapsed() < timeout => {
                                info!("Connected to Bluetooth device {}", address);
                            }
                            _ => {
                                // Command timed out or failed, try to kill the process
                                let _ = child.kill();
                                error!("bluetoothctl connect {} command timed out", address);
                            }
                        }
                    }
                    Err(e) => error!("Failed to connect to Bluetooth device: {}", e)
                }
        });
    }
    
    fn disconnect_device(address: String) {
        Self::run_in_background(move || {
            match Command::new("bluetoothctl")
                .args(&["disconnect", &address])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn() {
                    Ok(mut child) => {
                        // Set a timeout
                        let start = Instant::now();
                        let timeout = Duration::from_secs(3);
                        
                        // Check if command completes within timeout
                        match child.wait() {
                            Ok(_) if start.elapsed() < timeout => {
                                info!("Disconnected from Bluetooth device {}", address);
                            }
                            _ => {
                                // Command timed out or failed, try to kill the process
                                let _ = child.kill();
                                error!("bluetoothctl disconnect {} command timed out", address);
                            }
                        }
                    }
                    Err(e) => error!("Failed to disconnect from Bluetooth device: {}", e)
                }
        });
    }
    
    fn pair_device(address: String) {
        Self::run_in_background(move || {
            match Command::new("bluetoothctl")
                .args(&["pair", &address])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn() {
                    Ok(mut child) => {
                        // Set a timeout
                        let start = Instant::now();
                        let timeout = Duration::from_secs(20); // Pairing can take longer
                        
                        // Check if command completes within timeout
                        match child.wait() {
                            Ok(_) if start.elapsed() < timeout => {
                                info!("Paired with Bluetooth device {}", address);
                            }
                            _ => {
                                // Command timed out or failed, try to kill the process
                                let _ = child.kill();
                                error!("bluetoothctl pair {} command timed out", address);
                            }
                        }
                    }
                    Err(e) => error!("Failed to pair with Bluetooth device: {}", e)
                }
        });
    }
    
    fn create_device_row(device: &BluetoothDevice) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("bluetooth-device-row");
        
        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(10);
        hbox.set_margin_end(10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);
        
        // Device icon
        let icon = Image::from_icon_name(device.device_type.icon_name());
        icon.set_pixel_size(24);
        hbox.append(&icon);
        
        // Device info
        let info_box = Box::new(Orientation::Vertical, 2);
        info_box.set_hexpand(true);
        
        let name_label = Label::new(Some(&device.name));
        name_label.set_halign(gtk4::Align::Start);
        name_label.add_css_class("bluetooth-device-name");
        if device.connected {
            name_label.add_css_class("bluetooth-device-connected");
        }
        info_box.append(&name_label);
        
        // Status and battery
        let mut status_parts = Vec::new();
        if device.connected {
            status_parts.push("Connected".to_string());
        } else if device.paired {
            status_parts.push("Paired".to_string());
        }
        
        if let Some(battery) = device.battery_percentage {
            status_parts.push(format!("Battery: {}%", battery));
        }
        
        if !status_parts.is_empty() {
            let status_label = Label::new(Some(&status_parts.join(" • ")));
            status_label.set_halign(gtk4::Align::Start);
            status_label.add_css_class("bluetooth-device-status");
            info_box.append(&status_label);
        }
        
        hbox.append(&info_box);
        
        // Action button
        if device.connected {
            let disconnect_button = Button::with_label("Disconnect");
            disconnect_button.add_css_class("bluetooth-disconnect-button");
            
            let address = device.address.clone();
            disconnect_button.connect_clicked(move |_| {
                Self::disconnect_device(address.clone());
            });
            
            hbox.append(&disconnect_button);
        } else if device.paired {
            let connect_button = Button::with_label("Connect");
            connect_button.add_css_class("bluetooth-connect-button");
            
            let address = device.address.clone();
            connect_button.connect_clicked(move |_| {
                Self::connect_device(address.clone());
            });
            
            hbox.append(&connect_button);
        } else {
            let pair_button = Button::with_label("Pair");
            pair_button.add_css_class("bluetooth-pair-button");
            
            let address = device.address.clone();
            pair_button.connect_clicked(move |_| {
                Self::pair_device(address.clone());
            });
            
            hbox.append(&pair_button);
        }
        
        // Settings button for paired devices
        if device.paired {
            let settings_button = Button::from_icon_name("emblem-system-symbolic");
            settings_button.add_css_class("bluetooth-settings-button");
            settings_button.set_tooltip_text(Some("Device settings"));
            
            let address = device.address.clone();
            let trusted = device.trusted;
            settings_button.connect_clicked(move |button| {
                Self::show_device_menu(button, &address, trusted);
            });
            
            hbox.append(&settings_button);
        }
        
        row.set_child(Some(&hbox));
        row
    }
    
    fn update_icon(icon: &Image, powered: bool, connected_count: usize) {
        if !powered {
            icon.set_from_icon_name(Some("bluetooth-disabled-symbolic"));
            icon.add_css_class("bluetooth-disabled");
            icon.remove_css_class("bluetooth-active");
        } else if connected_count > 0 {
            icon.set_from_icon_name(Some("bluetooth-active-symbolic"));
            icon.add_css_class("bluetooth-active");
            icon.remove_css_class("bluetooth-disabled");
        } else {
            icon.set_from_icon_name(Some("bluetooth-symbolic"));
            icon.remove_css_class("bluetooth-active");
            icon.remove_css_class("bluetooth-disabled");
        }
    }
    
    fn show_device_menu(button: &Button, address: &str, trusted: bool) {
        let popover = Popover::new();
        popover.set_parent(button);
        popover.set_has_arrow(true);
        popover.set_position(gtk4::PositionType::Bottom);
        
        let menu_box = Box::new(Orientation::Vertical, 5);
        menu_box.set_margin_top(10);
        menu_box.set_margin_bottom(10);
        menu_box.set_margin_start(10);
        menu_box.set_margin_end(10);
        
        // Trust/Untrust button
        let trust_button = if trusted {
            Button::with_label("Remove Trust")
        } else {
            Button::with_label("Trust Device")
        };
        
        let address_clone = address.to_string();
        let popover_weak = popover.downgrade();
        trust_button.connect_clicked(move |_| {
            // Dismiss popover immediately for better UX
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
            
            // Run command in background
            let address_bg = address_clone.clone();
            let trusted_bg = trusted;
            Self::run_in_background(move || {
                let cmd = if trusted_bg { "untrust" } else { "trust" };
                match Command::new("bluetoothctl")
                    .args(&[cmd, &address_bg])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn() {
                        Ok(mut child) => {
                            // Set a timeout
                            let start = Instant::now();
                            let timeout = Duration::from_secs(2);
                            
                            // Check if command completes within timeout
                            match child.wait() {
                                Ok(_) if start.elapsed() < timeout => {
                                    info!("Changed trust status for device {}", address_bg);
                                }
                                _ => {
                                    // Command timed out or failed, try to kill the process
                                    let _ = child.kill();
                                    error!("bluetoothctl {} {} command timed out", cmd, address_bg);
                                }
                            }
                        }
                        Err(e) => error!("Failed to change trust status: {}", e)
                    }
            });
        });
        menu_box.append(&trust_button);
        
        // Remove device button
        let remove_button = Button::with_label("Remove Device");
        remove_button.add_css_class("destructive-action");
        
        let address_clone = address.to_string();
        let popover_weak = popover.downgrade();
        remove_button.connect_clicked(move |_| {
            // Dismiss popover immediately for better UX
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
            
            // Run command in background
            let address_bg = address_clone.clone();
            Self::run_in_background(move || {
                match Command::new("bluetoothctl")
                    .args(&["remove", &address_bg])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn() {
                        Ok(mut child) => {
                            // Set a timeout
                            let start = Instant::now();
                            let timeout = Duration::from_secs(3);
                            
                            // Check if command completes within timeout
                            match child.wait() {
                                Ok(_) if start.elapsed() < timeout => {
                                    info!("Removed Bluetooth device {}", address_bg);
                                }
                                _ => {
                                    // Command timed out or failed, try to kill the process
                                    let _ = child.kill();
                                    error!("bluetoothctl remove {} command timed out", address_bg);
                                }
                            }
                        }
                        Err(e) => error!("Failed to remove Bluetooth device: {}", e)
                    }
            });
        });
        menu_box.append(&remove_button);
        
        popover.set_child(Some(&menu_box));
        popover.popup();
    }
    
    fn open_bluetooth_settings() {
        Self::run_in_background(move || {
            // Try different Bluetooth settings commands
            let commands = vec![
                ("gnome-control-center", vec!["bluetooth"]),
                ("blueberry", vec![]),
                ("blueman-manager", vec![]),
                ("systemsettings5", vec!["kcm_bluetooth"]),
            ];
            
            for (cmd, args) in commands {
                match Command::new(cmd)
                    .args(&args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn() {
                        Ok(_) => {
                            info!("Opened Bluetooth settings with {}", cmd);
                            return;
                        }
                        Err(_) => continue
                    }
            }
            
            warn!("Could not find Bluetooth settings application");
        });
    }
    
    // Helper function to run operations in background
    fn run_in_background<F>(f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        thread::spawn(move || {
            f();
        });
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}