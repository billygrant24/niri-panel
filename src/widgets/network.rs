use anyhow::Result;
use glib::timeout_add_seconds_local;
use gtk4::gdk::Display;
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Entry, GestureClick, Image, Label, ListBox, ListBoxRow,
    Orientation, Popover, ScrolledWindow, Spinner, Switch,
};
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;
use tracing::{info, warn};

pub struct Network {
    button: Button,
}

#[derive(Debug, Clone)]
struct NetworkInfo {
    interface: String,
    connection_type: ConnectionType,
    ssid: Option<String>,
    signal_strength: Option<u8>,
    ip_address: Option<String>,
    ipv6_address: Option<String>,
    // Removed public IP fields that were causing deadlocks
    connected: bool,
    vpn_active: bool,
    vpn_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum ConnectionType {
    Wifi,
    Ethernet,
    Disconnected,
}

#[derive(Debug, Clone)]
struct VpnConnection {
    name: String,
    uuid: String,
    active: bool,
}

#[derive(Debug, Clone)]
struct WifiNetwork {
    ssid: String,
    signal: u8,
    secured: bool,
    connected: bool,
    #[allow(dead_code)]
    bssid: String,
}

impl Network {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("network");

        let container = Box::new(Orientation::Horizontal, 5);

        let icon = Image::new();
        let vpn_icon = Image::new();
        vpn_icon.set_visible(false);
        vpn_icon.add_css_class("network-vpn-icon");

        let label = Label::new(None);
        label.add_css_class("network-label");

        container.append(&icon);
        container.append(&vpn_icon);
        container.append(&label);
        button.set_child(Some(&container));

        // Create popover for network details
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("network-popover");

        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!(
                    "Network popover shown - keyboard mode set to OnDemand (active popovers: {})",
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
                    info!("Network popover hidden - keyboard mode set to None");
                }
            } else {
                info!(
                    "Network popover hidden - keeping keyboard mode (active popovers: {})",
                    count
                );
            }
        });

        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(10);
        popover_box.set_margin_bottom(10);
        popover_box.set_margin_start(10);
        popover_box.set_margin_end(10);
        popover_box.set_size_request(350, -1);

        popover.set_child(Some(&popover_box));

        // Store scanning state
        let scanning = Rc::new(RefCell::new(false));

        // Initialize with a basic update of just the icon
        let initial_info = NetworkInfo {
            interface: "unknown".to_string(),
            connection_type: ConnectionType::Disconnected,
            ssid: None,
            signal_strength: None,
            ip_address: None,
            ipv6_address: None,
            // Removed public IP fields
            connected: false,
            vpn_active: false,
            vpn_name: None,
        };
        
        // Setup state for the update process
        let update_state = Rc::new(RefCell::new(0)); // 0 = idle, 1-5 = update steps
        let update_info = Rc::new(RefCell::new(initial_info.clone()));
        let vpn_connections_state = Rc::new(RefCell::new(Vec::new()));
        
        let icon_name = Self::get_network_icon_name(&initial_info);
        icon.set_from_icon_name(Some(&icon_name));
        
        // Use a dispatcher pattern to handle network updates without blocking
        let icon_weak = icon.downgrade();
        let vpn_icon_weak = vpn_icon.downgrade();
        let label_weak = label.downgrade();
        let popover_box_weak = popover_box.downgrade();
        let scanning_clone = scanning.clone();
        
        // Use a two-step update process:
        // 1. Queue the update operation at regular intervals
        // 2. Process one small part of the update in each idle callback
        
        // Clone update_state before moving it
        let update_state_for_queue = update_state.clone();
        
        // Queue the next update
        let queue_update = Rc::new(RefCell::new(move || {
            // Skip update if scanning is in progress
            if *scanning_clone.borrow() {
                return;
            }
            
            // Set update state to 1 (start update process)
            *update_state_for_queue.borrow_mut() = 1;
        }));
        
        // Initial update
        (queue_update.borrow())();
        
        // Schedule updates every 5 seconds
        let queue_update_clone = queue_update.clone();
        timeout_add_seconds_local(5, move || {
            (queue_update_clone.borrow())();
            glib::ControlFlow::Continue
        });
        
        // Process update steps in idle time to avoid blocking the UI
        let update_state_clone = update_state.clone();
        let update_info_clone = update_info.clone();
        let vpn_connections_clone = vpn_connections_state.clone();
        
        glib::idle_add_local(move || {
            if let (Some(icon), Some(vpn_icon), Some(label), Some(popover_box)) = (
                icon_weak.upgrade(),
                vpn_icon_weak.upgrade(),
                label_weak.upgrade(),
                popover_box_weak.upgrade(),
            ) {
                let state = *update_state_clone.borrow();
                
                match state {
                    0 => {
                        // Idle, nothing to do
                        return glib::ControlFlow::Continue;
                    },
                    1 => {
                        // Step 1: Check VPN status (very quick operation)
                        let mut info = initial_info.clone();
                        
                        // Check for active VPN with timeout - minimal info gathering
                        if let Ok(output) = std::process::Command::new("timeout")
                            .args(&["0.5", "nmcli", "-t", "-f", "TYPE,NAME,STATE", "connection", "show", "--active"])
                            .output()
                        {
                            for line in String::from_utf8_lossy(&output.stdout).lines() {
                                let parts: Vec<&str> = line.split(':').collect();
                                if parts.len() >= 3 && parts[0] == "vpn" && parts[2] == "activated" {
                                    info.vpn_active = true;
                                    info.vpn_name = Some(parts[1].to_string());
                                    break;
                                }
                            }
                        }
                        
                        // Get interface type quickly
                        info!("Detecting network interface type");
                        if let Ok(output) = std::process::Command::new("timeout")
                            .args(&["0.5", "ip", "-o", "link", "show", "up"])
                            .output()
                        {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            info!("IP link output: {}", output_str);
                            for line in output_str.lines() {
                                let parts: Vec<&str> = line.split_whitespace().collect();
                                if parts.len() >= 2 {
                                    let iface = parts[1].trim_end_matches(':');
                                    info!("Found interface: {}", iface);
                                    if iface.starts_with("wl") || iface.starts_with("wlan") {
                                        info.interface = iface.to_string();
                                        info.connection_type = ConnectionType::Wifi;
                                        info.connected = true;
                                        info!("Detected as WiFi interface: {}", iface);
                                        break;
                                    } else if iface.starts_with("en") || iface.starts_with("eth") {
                                        info.interface = iface.to_string();
                                        info.connection_type = ConnectionType::Ethernet;
                                        info.connected = true;
                                        info!("Detected as Ethernet interface: {}", iface);
                                        break;
                                    }
                                }
                            }
                        }
                        
                        info!("After interface detection: connection_type={:?}, connected={}", 
                              info.connection_type, info.connected);
                        
                        // Update icon immediately with what we know
                        let icon_name = Self::get_network_icon_name(&info);
                        icon.set_from_icon_name(Some(&icon_name));
                        
                        // Update VPN icon immediately
                        if info.vpn_active {
                            vpn_icon.set_from_icon_name(Some("network-vpn-symbolic"));
                            vpn_icon.set_visible(true);
                            vpn_icon.set_tooltip_text(Some(&format!(
                                "VPN: {}",
                                info.vpn_name.as_ref().unwrap_or(&"Active".to_string())
                            )));
                        } else {
                            vpn_icon.set_visible(false);
                        }
                        
                        // Store the basic info
                        *update_info_clone.borrow_mut() = info;
                        
                        // Move to next step
                        *update_state_clone.borrow_mut() = 2;
                    },
                    2 => {
                        // Step 2: Get additional connection details (SSID, etc.)
                        let mut info = update_info_clone.borrow().clone();
                        
                        if info.connection_type == ConnectionType::Wifi {
                            // For WiFi, get SSID
                            info!("Attempting to get SSID for WiFi");
                            if let Ok(output) = std::process::Command::new("timeout")
                                .args(&["0.5", "iwgetid", "-r"])
                                .output()
                            {
                                let ssid = String::from_utf8_lossy(&output.stdout).trim().to_string();
                                info!("iwgetid returned SSID: '{}' (empty: {})", ssid, ssid.is_empty());
                                if !ssid.is_empty() {
                                    info.ssid = Some(ssid);
                                } else {
                                    // Try alternative method with nmcli
                                    if let Ok(nmcli_output) = std::process::Command::new("timeout")
                                        .args(&["0.5", "nmcli", "-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
                                        .output()
                                    {
                                        let output_str = String::from_utf8_lossy(&nmcli_output.stdout);
                                        info!("Trying nmcli for SSID: {}", output_str);
                                        for line in output_str.lines() {
                                            let parts: Vec<&str> = line.split(':').collect();
                                            if parts.len() >= 2 && parts[1] == info.interface {
                                                info.ssid = Some(parts[0].to_string());
                                                info!("Found SSID via nmcli: {}", parts[0]);
                                                break;
                                            }
                                        }
                                    }
                                }
                            } else {
                                info!("iwgetid command failed, trying nmcli");
                                // Try alternative method with nmcli if iwgetid fails
                                if let Ok(output) = std::process::Command::new("timeout")
                                    .args(&["0.5", "nmcli", "-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
                                    .output()
                                {
                                    let output_str = String::from_utf8_lossy(&output.stdout);
                                    for line in output_str.lines() {
                                        let parts: Vec<&str> = line.split(':').collect();
                                        if parts.len() >= 2 && parts[1] == info.interface {
                                            info.ssid = Some(parts[0].to_string());
                                            info!("Found SSID via nmcli: {}", parts[0]);
                                            break;
                                        }
                                    }
                                }
                            }
                            
                            // Try to get signal strength
                            if let Ok(output) = std::process::Command::new("timeout")
                                .args(&["0.5", "nmcli", "-t", "-f", "ACTIVE,SIGNAL", "dev", "wifi"])
                                .output()
                            {
                                let output_str = String::from_utf8_lossy(&output.stdout);
                                for line in output_str.lines() {
                                    let parts: Vec<&str> = line.split(':').collect();
                                    if parts.get(0) == Some(&"yes") {
                                        info.signal_strength = parts.get(1).and_then(|s| s.parse::<u8>().ok());
                                        break;
                                    }
                                }
                            }
                        }
                        
                        // Update the label
                        let label_text = match &info.connection_type {
                            ConnectionType::Wifi => {
                                if let Some(ssid) = &info.ssid {
                                    if info.vpn_active {
                                        format!("{} (VPN)", ssid)
                                    } else {
                                        ssid.clone()
                                    }
                                } else {
                                    "WiFi".to_string()
                                }
                            }
                            ConnectionType::Ethernet => {
                                if info.vpn_active {
                                    "Ethernet (VPN)".to_string()
                                } else {
                                    "Ethernet".to_string()
                                }
                            }
                            ConnectionType::Disconnected => "Disconnected".to_string(),
                        };
                        label.set_text(&label_text);
                        
                        // Store the updated info
                        *update_info_clone.borrow_mut() = info;
                        
                        // Move to next step
                        *update_state_clone.borrow_mut() = 3;
                    },
                    3 => {
                        // Step 3: Get IP addresses
                        let mut info = update_info_clone.borrow().clone();
                        
                        if info.connected {
                            // Get local IP addresses
                            if let Ok(output) = std::process::Command::new("timeout")
                                .args(&["0.5", "ip", "addr", "show", &info.interface])
                                .output()
                            {
                                let output_str = String::from_utf8_lossy(&output.stdout);
                                for line in output_str.lines() {
                                    let line_trimmed = line.trim();
                                    if line_trimmed.starts_with("inet ") && info.ip_address.is_none() {
                                        let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
                                        if let Some(ip) = parts.get(1) {
                                            // Filter out IPv4 link-local addresses (169.254.x.x)
                                            let ip_clean = ip.split('/').next().unwrap_or(ip).to_string();
                                            if !ip_clean.starts_with("169.254.") {
                                                info.ip_address = Some(ip_clean);
                                            }
                                        }
                                    } else if line_trimmed.starts_with("inet6 ") && info.ipv6_address.is_none() {
                                        let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
                                        if let Some(ip) = parts.get(1) {
                                            let ip_clean = ip.split('/').next().unwrap_or(ip).to_string();
                                            // Filter out link-local addresses (fe80::) and temporary addresses
                                            if !ip_clean.starts_with("fe80:")
                                                && !parts.iter().any(|p| *p == "temporary")
                                            {
                                                info.ipv6_address = Some(ip_clean);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        
                        // Store the updated info
                        *update_info_clone.borrow_mut() = info;
                        
                        // Move to next step
                        *update_state_clone.borrow_mut() = 4;
                    },
                    4 => {
                        // Step 4: Get VPN connections (for popover)
                        let mut vpns = Vec::new();
                        
                        // Get all VPN connections with timeout
                        if let Ok(output) = std::process::Command::new("timeout")
                            .args(&["0.5", "nmcli", "-t", "-f", "NAME,UUID,TYPE,STATE", "connection", "show"])
                            .output()
                        {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            for line in output_str.lines() {
                                let parts: Vec<&str> = line.split(':').collect();
                                if parts.len() >= 4 && parts[2] == "vpn" {
                                    vpns.push(VpnConnection {
                                        name: parts[0].to_string(),
                                        uuid: parts[1].to_string(),
                                        active: parts[3] == "activated",
                                    });
                                }
                            }
                        }
                        
                        // Store the VPN connections
                        *vpn_connections_clone.borrow_mut() = vpns;
                        
                        // Move to next step - updating the popover
                        *update_state_clone.borrow_mut() = 5;
                    },
                    5 => {
                        // Step 5: Update the popover if it's visible
                        if popover_box.is_visible() {
                            let info = update_info_clone.borrow().clone();
                            let vpn_connections = vpn_connections_clone.borrow().clone();
                            
                            // Make sure we have SSID information if this is a WiFi connection
                            if info.connection_type == ConnectionType::Wifi && info.ssid.is_none() {
                                // Try one more time to get SSID with a direct approach
                                info!("Trying to get SSID one more time for popover update");
                                if let Ok(output) = std::process::Command::new("timeout")
                                    .args(&["0.5", "iwgetid", "-r"])
                                    .output()
                                {
                                    let ssid = String::from_utf8_lossy(&output.stdout).trim().to_string();
                                    info!("iwgetid for popover returned SSID: '{}' (empty: {})", ssid, ssid.is_empty());
                                    if !ssid.is_empty() {
                                        let mut updated_info = info.clone();
                                        updated_info.ssid = Some(ssid);
                                        info!("Using updated info with SSID for popover");
                                        // Update popover content with the improved info
                                        Self::update_popover_content(&popover_box, &updated_info, &vpn_connections);
                                        return glib::ControlFlow::Continue;
                                    }
                                }
                                
                                // Try with nmcli as a last resort
                                if let Ok(output) = std::process::Command::new("timeout")
                                    .args(&["0.5", "nmcli", "-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
                                    .output()
                                {
                                    let output_str = String::from_utf8_lossy(&output.stdout);
                                    info!("Last attempt with nmcli for popover: {}", output_str);
                                    for line in output_str.lines() {
                                        let parts: Vec<&str> = line.split(':').collect();
                                        if parts.len() >= 2 && parts[1] == info.interface {
                                            let mut updated_info = info.clone();
                                            updated_info.ssid = Some(parts[0].to_string());
                                            info!("Found SSID via nmcli for popover: {}", parts[0]);
                                            // Update popover content with the improved info
                                            Self::update_popover_content(&popover_box, &updated_info, &vpn_connections);
                                            return glib::ControlFlow::Continue;
                                        }
                                    }
                                }
                            }
                            
                            // Update popover content with what we have
                            Self::update_popover_content(&popover_box, &info, &vpn_connections);
                        }
                        
                        // Done with update, reset to idle
                        *update_state_clone.borrow_mut() = 0;
                    },
                    _ => {
                        // Invalid state, reset
                        *update_state_clone.borrow_mut() = 0;
                    }
                }
                
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });

        // Show popover on click and force an update
        let update_state_for_click = update_state.clone();
        button.connect_clicked(move |_| {
            // Force an update when the popover is about to be shown
            *update_state_for_click.borrow_mut() = 1;
            popover.popup();
        });

        Ok(Self { button })
    }

    // Helper method to update just the popover content
    fn update_popover_content(
        popover_box: &Box,
        info: &NetworkInfo,
        vpn_connections: &[VpnConnection],
    ) {
        // Clear existing content
        while let Some(child) = popover_box.first_child() {
            popover_box.remove(&child);
        }

        // Connection status
        let status_label = Label::new(Some(&format!(
            "Status: {}",
            if info.connected {
                "Connected"
            } else {
                "Disconnected"
            }
        )));
        status_label.set_halign(gtk4::Align::Start);
        status_label.add_css_class("network-status");
        popover_box.append(&status_label);

        // Interface
        let interface_label = Label::new(Some(&format!("Interface: {}", info.interface)));
        interface_label.set_halign(gtk4::Align::Start);
        popover_box.append(&interface_label);
        
        // SSID (after interface) - show for WiFi connections
        if info.connection_type == ConnectionType::Wifi && info.connected {
            info!("WiFi connection detected, checking SSID");
            
            // Try to find SSID using multiple methods if needed
            let ssid = if let Some(ssid) = &info.ssid {
                info!("Found SSID from info: {}", ssid);
                Some(ssid.clone())
            } else {
                // Try to get SSID with various methods
                Self::get_wifi_ssid(&info.interface)
            };
            
            if let Some(ssid_value) = ssid {
                info!("Using SSID for display: {}", ssid_value);
                let ssid_row = Self::create_address_row("SSID:", &ssid_value);
                popover_box.append(&ssid_row);
            } else {
                info!("No SSID found despite WiFi connection - showing placeholder");
                // If all methods fail, at least show something
                let unknown_row = Self::create_address_row("SSID:", "Unknown");
                popover_box.append(&unknown_row);
            }
        } else {
            info!("Not adding SSID row: connection_type={:?}, connected={}", 
                  info.connection_type, info.connected);
        }

        // Signal strength section for WiFi
        if info.connection_type == ConnectionType::Wifi && info.connected {
            info!("Adding signal strength info");
            // Signal strength
            if let Some(signal) = info.signal_strength {
                info!("Signal strength available: {}%", signal);
                let signal_label = Label::new(Some(&format!("Signal: {}%", signal)));
                signal_label.set_halign(gtk4::Align::Start);
                popover_box.append(&signal_label);
            } else {
                info!("No signal strength info available");
            }
        }

        // Separator for IP section
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        popover_box.append(&separator);

        // IP addresses header
        let ip_header = Label::new(Some("IP Addresses"));
        ip_header.add_css_class("network-section-title");
        ip_header.set_halign(gtk4::Align::Start);
        popover_box.append(&ip_header);

        // Local IP Addresses
        if let Some(ip) = &info.ip_address {
            let ip_row = Self::create_address_row("Local IPv4:", ip);
            popover_box.append(&ip_row);
        }

        if let Some(ipv6) = &info.ipv6_address {
            let ipv6_row = Self::create_address_row("Local IPv6:", ipv6);
            popover_box.append(&ipv6_row);
        }

        // WiFi Networks Section (only show for WiFi connections or when disconnected)
        if info.connection_type == ConnectionType::Wifi
            || info.connection_type == ConnectionType::Disconnected
        {
            // Separator
            let separator = gtk4::Separator::new(Orientation::Horizontal);
            separator.set_margin_top(10);
            separator.set_margin_bottom(10);
            popover_box.append(&separator);

            let wifi_header = Box::new(Orientation::Horizontal, 10);

            let wifi_label = Label::new(Some("WiFi Networks"));
            wifi_label.set_halign(gtk4::Align::Start);
            wifi_label.set_hexpand(true);
            wifi_label.add_css_class("network-section-title");
            wifi_header.append(&wifi_label);

            let refresh_button = Button::from_icon_name("view-refresh-symbolic");
            refresh_button.add_css_class("network-refresh-button");
            refresh_button.set_tooltip_text(Some("Scan for networks"));

            let popover_box_weak = popover_box.downgrade();
            refresh_button.connect_clicked(move |button| {
                if let Some(popover_box) = popover_box_weak.upgrade() {
                    button.set_sensitive(false);

                    // Start scan with timeout
                    let _ = std::process::Command::new("timeout")
                        .args(&["2", "nmcli", "device", "wifi", "rescan"])
                        .spawn();

                    // Show scanning state
                    if let Some(wifi_list_widget) = popover_box
                        .observe_children()
                        .item(popover_box.observe_children().n_items() - 1)
                    {
                        if let Some(scroll) = wifi_list_widget.downcast_ref::<ScrolledWindow>() {
                            if let Some(list) = scroll
                                .child()
                                .and_then(|w| w.downcast_ref::<ListBox>().cloned())
                            {
                                while let Some(child) = list.first_child() {
                                    list.remove(&child);
                                }

                                let scanning_row = ListBoxRow::new();
                                let scanning_box = Box::new(Orientation::Horizontal, 10);
                                scanning_box.set_margin_start(10);
                                scanning_box.set_margin_end(10);
                                scanning_box.set_margin_top(20);
                                scanning_box.set_margin_bottom(20);
                                scanning_box.set_halign(gtk4::Align::Center);

                                let spinner = Spinner::new();
                                spinner.start();
                                scanning_box.append(&spinner);

                                let scanning_label = Label::new(Some("Scanning for networks..."));
                                scanning_label.add_css_class("dim-label");
                                scanning_box.append(&scanning_label);

                                scanning_row.set_child(Some(&scanning_box));
                                list.append(&scanning_row);
                            }
                        }
                    }

                    // Wait and update
                    let popover_box_weak2 = popover_box.downgrade();
                    let button_weak = button.downgrade();
                    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                        if let (Some(popover_box), Some(button)) =
                            (popover_box_weak2.upgrade(), button_weak.upgrade())
                        {
                            button.set_sensitive(true);
                            Self::update_wifi_list(&popover_box);
                        }
                    });
                }
            });

            wifi_header.append(&refresh_button);
            popover_box.append(&wifi_header);

            // WiFi list with scroll
            let wifi_scroll = ScrolledWindow::new();
            wifi_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
            wifi_scroll.set_min_content_height(100);
            wifi_scroll.set_max_content_height(300);

            let wifi_list = ListBox::new();
            wifi_list.add_css_class("network-wifi-list");
            wifi_list.set_selection_mode(gtk4::SelectionMode::None);

            // Get available networks with small timeout
            let networks = Self::get_wifi_networks();

            if networks.is_empty() {
                let empty_row = ListBoxRow::new();
                let empty_label = Label::new(Some("No networks found"));
                empty_label.add_css_class("dim-label");
                empty_label.set_margin_top(20);
                empty_label.set_margin_bottom(20);
                empty_row.set_child(Some(&empty_label));
                wifi_list.append(&empty_row);
            } else {
                for network in networks {
                    let row = Self::create_wifi_row(&network);
                    wifi_list.append(&row);
                }
            }

            wifi_scroll.set_child(Some(&wifi_list));
            popover_box.append(&wifi_scroll);
        }

        // VPN Section
        if !vpn_connections.is_empty() || info.vpn_active {
            // Separator
            let separator = gtk4::Separator::new(Orientation::Horizontal);
            separator.set_margin_top(10);
            separator.set_margin_bottom(10);
            popover_box.append(&separator);

            let vpn_label = Label::new(Some("VPN Connections"));
            vpn_label.set_halign(gtk4::Align::Start);
            vpn_label.add_css_class("network-section-title");
            popover_box.append(&vpn_label);

            // VPN list
            let vpn_list = ListBox::new();
            vpn_list.add_css_class("network-vpn-list");
            vpn_list.set_selection_mode(gtk4::SelectionMode::None);

            for vpn in vpn_connections {
                let row = ListBoxRow::new();
                row.add_css_class("network-vpn-row");

                let hbox = Box::new(Orientation::Horizontal, 10);
                hbox.set_margin_start(5);
                hbox.set_margin_end(5);
                hbox.set_margin_top(8);
                hbox.set_margin_bottom(8);

                let vpn_icon = Image::from_icon_name("network-vpn-symbolic");
                vpn_icon.set_pixel_size(16);
                hbox.append(&vpn_icon);

                let vpn_name_label = Label::new(Some(&vpn.name));
                vpn_name_label.set_hexpand(true);
                vpn_name_label.set_halign(gtk4::Align::Start);
                hbox.append(&vpn_name_label);

                let switch = Switch::new();
                switch.set_active(vpn.active);
                switch.set_valign(gtk4::Align::Center);

                let vpn_uuid = vpn.uuid.clone();
                let vpn_active = vpn.active;
                switch.connect_state_set(move |_, state| {
                    if state != vpn_active {
                        Self::toggle_vpn(&vpn_uuid, state);
                    }
                    glib::Propagation::Proceed
                });

                hbox.append(&switch);
                row.set_child(Some(&hbox));
                vpn_list.append(&row);
            }

            popover_box.append(&vpn_list);
        }

        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(10);
        separator.set_margin_bottom(10);
        popover_box.append(&separator);

        // Action buttons
        let actions_box = Box::new(Orientation::Vertical, 5);

        // Network settings button
        let settings_button = Button::with_label("Network Settings");
        settings_button.connect_clicked(|_| {
            Self::open_network_settings();
        });
        actions_box.append(&settings_button);

        // WiFi toggle (if applicable)
        if info.connection_type == ConnectionType::Wifi
            || info.connection_type == ConnectionType::Disconnected
        {
            let wifi_toggle = Button::with_label(if info.connection_type == ConnectionType::Wifi {
                "Disconnect WiFi"
            } else {
                "Connect WiFi"
            });
            wifi_toggle.connect_clicked(move |_| {
                Self::toggle_wifi();
            });
            actions_box.append(&wifi_toggle);
        }

        popover_box.append(&actions_box);
    }

    // Original update method kept for reference but not used anymore
    fn update_network(
        icon: &Image,
        vpn_icon: &Image,
        label: &Label,
        popover_box: &Box,
        scanning: Rc<RefCell<bool>>,
    ) {
        // Get basic network info synchronously
        let mut info = Self::get_network_info();
        let vpn_connections = Self::get_vpn_connections();

        // Update icon
        let icon_name = Self::get_network_icon_name(&info);
        icon.set_from_icon_name(Some(&icon_name));

        // Update VPN icon
        if info.vpn_active {
            vpn_icon.set_from_icon_name(Some("network-vpn-symbolic"));
            vpn_icon.set_visible(true);
            vpn_icon.set_tooltip_text(Some(&format!(
                "VPN: {}",
                info.vpn_name.as_ref().unwrap_or(&"Active".to_string())
            )));
        } else {
            vpn_icon.set_visible(false);
        }

        // Update label
        let label_text = match &info.connection_type {
            ConnectionType::Wifi => {
                if let Some(ssid) = &info.ssid {
                    if info.vpn_active {
                        format!("{} (VPN)", ssid)
                    } else {
                        ssid.clone()
                    }
                } else {
                    "WiFi".to_string()
                }
            }
            ConnectionType::Ethernet => {
                if info.vpn_active {
                    "Ethernet (VPN)".to_string()
                } else {
                    "Ethernet".to_string()
                }
            }
            ConnectionType::Disconnected => "Disconnected".to_string(),
        };
        label.set_text(&label_text);

        // Update popover content
        while let Some(child) = popover_box.first_child() {
            popover_box.remove(&child);
        }

        // Connection status
        let status_label = Label::new(Some(&format!(
            "Status: {}",
            if info.connected {
                "Connected"
            } else {
                "Disconnected"
            }
        )));
        status_label.set_halign(gtk4::Align::Start);
        status_label.add_css_class("network-status");
        popover_box.append(&status_label);

        // Interface
        let interface_label = Label::new(Some(&format!("Interface: {}", info.interface)));
        interface_label.set_halign(gtk4::Align::Start);
        popover_box.append(&interface_label);
        
        // SSID (after interface) - show for WiFi connections
        if info.connection_type == ConnectionType::Wifi && info.connected {
            info!("WiFi connection detected, checking SSID");
            
            // Try to find SSID using multiple methods if needed
            let ssid = if let Some(ssid) = &info.ssid {
                info!("Found SSID from info: {}", ssid);
                Some(ssid.clone())
            } else {
                // Try to get SSID with various methods
                Self::get_wifi_ssid(&info.interface)
            };
            
            if let Some(ssid_value) = ssid {
                info!("Using SSID for display: {}", ssid_value);
                let ssid_row = Self::create_address_row("SSID:", &ssid_value);
                popover_box.append(&ssid_row);
            } else {
                info!("No SSID found despite WiFi connection - showing placeholder");
                // If all methods fail, at least show something
                let unknown_row = Self::create_address_row("SSID:", "Unknown");
                popover_box.append(&unknown_row);
            }
        } else {
            info!("Not adding SSID row: connection_type={:?}, connected={}", 
                  info.connection_type, info.connected);
        }

        // Signal strength section for WiFi
        if info.connection_type == ConnectionType::Wifi && info.connected {
            info!("Adding signal strength info");
            // Signal strength
            if let Some(signal) = info.signal_strength {
                info!("Signal strength available: {}%", signal);
                let signal_label = Label::new(Some(&format!("Signal: {}%", signal)));
                signal_label.set_halign(gtk4::Align::Start);
                popover_box.append(&signal_label);
            } else {
                info!("No signal strength info available");
            }
        }

        // Separator for IP section
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        popover_box.append(&separator);

        // IP addresses header
        let ip_header = Label::new(Some("IP Addresses"));
        ip_header.add_css_class("network-section-title");
        ip_header.set_halign(gtk4::Align::Start);
        popover_box.append(&ip_header);

        // Local IP Addresses
        if let Some(ip) = &info.ip_address {
            let ip_row = Self::create_address_row("Local IPv4:", ip);
            popover_box.append(&ip_row);
        }

        if let Some(ipv6) = &info.ipv6_address {
            let ipv6_row = Self::create_address_row("Local IPv6:", ipv6);
            popover_box.append(&ipv6_row);
        }

        // Public IP functionality removed to fix deadlocks

        // WiFi Networks Section (only show for WiFi connections or when disconnected)
        if info.connection_type == ConnectionType::Wifi
            || info.connection_type == ConnectionType::Disconnected
        {
            // Separator
            let separator = gtk4::Separator::new(Orientation::Horizontal);
            separator.set_margin_top(10);
            separator.set_margin_bottom(10);
            popover_box.append(&separator);

            let wifi_header = Box::new(Orientation::Horizontal, 10);

            let wifi_label = Label::new(Some("WiFi Networks"));
            wifi_label.set_halign(gtk4::Align::Start);
            wifi_label.set_hexpand(true);
            wifi_label.add_css_class("network-section-title");
            wifi_header.append(&wifi_label);

            let refresh_button = Button::from_icon_name("view-refresh-symbolic");
            refresh_button.add_css_class("network-refresh-button");
            refresh_button.set_tooltip_text(Some("Scan for networks"));

            let popover_box_weak = popover_box.downgrade();
            let scanning_for_refresh = scanning.clone();
            refresh_button.connect_clicked(move |button| {
                if let Some(popover_box) = popover_box_weak.upgrade() {
                    *scanning_for_refresh.borrow_mut() = true;
                    button.set_sensitive(false);

                    // Start scan with timeout
                    let _ = std::process::Command::new("timeout")
                        .args(&["2", "nmcli", "device", "wifi", "rescan"])
                        .spawn();

                    // Show scanning state
                    if let Some(wifi_list_widget) = popover_box
                        .observe_children()
                        .item(popover_box.observe_children().n_items() - 1)
                    {
                        if let Some(scroll) = wifi_list_widget.downcast_ref::<ScrolledWindow>() {
                            if let Some(list) = scroll
                                .child()
                                .and_then(|w| w.downcast_ref::<ListBox>().cloned())
                            {
                                while let Some(child) = list.first_child() {
                                    list.remove(&child);
                                }

                                let scanning_row = ListBoxRow::new();
                                let scanning_box = Box::new(Orientation::Horizontal, 10);
                                scanning_box.set_margin_start(10);
                                scanning_box.set_margin_end(10);
                                scanning_box.set_margin_top(20);
                                scanning_box.set_margin_bottom(20);
                                scanning_box.set_halign(gtk4::Align::Center);

                                let spinner = Spinner::new();
                                spinner.start();
                                scanning_box.append(&spinner);

                                let scanning_label = Label::new(Some("Scanning for networks..."));
                                scanning_label.add_css_class("dim-label");
                                scanning_box.append(&scanning_label);

                                scanning_row.set_child(Some(&scanning_box));
                                list.append(&scanning_row);
                            }
                        }
                    }

                    // Wait and update
                    let popover_box_weak2 = popover_box.downgrade();
                    let button_weak = button.downgrade();
                    let scanning_for_timeout = scanning_for_refresh.clone();
                    glib::timeout_add_local_once(std::time::Duration::from_secs(3), move || {
                        *scanning_for_timeout.borrow_mut() = false;
                        if let (Some(popover_box), Some(button)) =
                            (popover_box_weak2.upgrade(), button_weak.upgrade())
                        {
                            button.set_sensitive(true);
                            Self::update_wifi_list(&popover_box);
                        }
                    });
                }
            });

            wifi_header.append(&refresh_button);
            popover_box.append(&wifi_header);

            // WiFi list with scroll
            let wifi_scroll = ScrolledWindow::new();
            wifi_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
            wifi_scroll.set_min_content_height(100);
            wifi_scroll.set_max_content_height(300);

            let wifi_list = ListBox::new();
            wifi_list.add_css_class("network-wifi-list");
            wifi_list.set_selection_mode(gtk4::SelectionMode::None);

            // Get available networks
            let networks = Self::get_wifi_networks();

            if networks.is_empty() {
                let empty_row = ListBoxRow::new();
                let empty_label = Label::new(Some("No networks found"));
                empty_label.add_css_class("dim-label");
                empty_label.set_margin_top(20);
                empty_label.set_margin_bottom(20);
                empty_row.set_child(Some(&empty_label));
                wifi_list.append(&empty_row);
            } else {
                for network in networks {
                    let row = Self::create_wifi_row(&network);
                    wifi_list.append(&row);
                }
            }

            wifi_scroll.set_child(Some(&wifi_list));
            popover_box.append(&wifi_scroll);
        }

        // VPN Section
        if !vpn_connections.is_empty() || info.vpn_active {
            // Separator
            let separator = gtk4::Separator::new(Orientation::Horizontal);
            separator.set_margin_top(10);
            separator.set_margin_bottom(10);
            popover_box.append(&separator);

            let vpn_label = Label::new(Some("VPN Connections"));
            vpn_label.set_halign(gtk4::Align::Start);
            vpn_label.add_css_class("network-section-title");
            popover_box.append(&vpn_label);

            // VPN list
            let vpn_list = ListBox::new();
            vpn_list.add_css_class("network-vpn-list");
            vpn_list.set_selection_mode(gtk4::SelectionMode::None);

            for vpn in &vpn_connections {
                let row = ListBoxRow::new();
                row.add_css_class("network-vpn-row");

                let hbox = Box::new(Orientation::Horizontal, 10);
                hbox.set_margin_start(5);
                hbox.set_margin_end(5);
                hbox.set_margin_top(8);
                hbox.set_margin_bottom(8);

                let vpn_icon = Image::from_icon_name("network-vpn-symbolic");
                vpn_icon.set_pixel_size(16);
                hbox.append(&vpn_icon);

                let vpn_name_label = Label::new(Some(&vpn.name));
                vpn_name_label.set_hexpand(true);
                vpn_name_label.set_halign(gtk4::Align::Start);
                hbox.append(&vpn_name_label);

                let switch = Switch::new();
                switch.set_active(vpn.active);
                switch.set_valign(gtk4::Align::Center);

                let vpn_uuid = vpn.uuid.clone();
                let vpn_active = vpn.active;
                switch.connect_state_set(move |_, state| {
                    if state != vpn_active {
                        Self::toggle_vpn(&vpn_uuid, state);
                    }
                    glib::Propagation::Proceed
                });

                hbox.append(&switch);
                row.set_child(Some(&hbox));
                vpn_list.append(&row);
            }

            popover_box.append(&vpn_list);
        }

        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(10);
        separator.set_margin_bottom(10);
        popover_box.append(&separator);

        // Action buttons
        let actions_box = Box::new(Orientation::Vertical, 5);

        // Network settings button
        let settings_button = Button::with_label("Network Settings");
        settings_button.connect_clicked(|_| {
            Self::open_network_settings();
        });
        actions_box.append(&settings_button);

        // WiFi toggle (if applicable)
        if info.connection_type == ConnectionType::Wifi
            || info.connection_type == ConnectionType::Disconnected
        {
            let wifi_toggle = Button::with_label(if info.connection_type == ConnectionType::Wifi {
                "Disconnect WiFi"
            } else {
                "Connect WiFi"
            });
            wifi_toggle.connect_clicked(move |_| {
                Self::toggle_wifi();
            });
            actions_box.append(&wifi_toggle);
        }

        popover_box.append(&actions_box);
    }

    fn update_wifi_list(popover_box: &Box) {
        // Find the wifi list in the popover
        for i in 0..popover_box.observe_children().n_items() {
            if let Some(widget) = popover_box.observe_children().item(i) {
                if let Some(scroll) = widget.downcast_ref::<ScrolledWindow>() {
                    if let Some(list) = scroll
                        .child()
                        .and_then(|w| w.downcast_ref::<ListBox>().cloned())
                    {
                        // Clear existing items
                        while let Some(child) = list.first_child() {
                            list.remove(&child);
                        }

                        // Get fresh network list
                        let networks = Self::get_wifi_networks();

                        if networks.is_empty() {
                            let empty_row = ListBoxRow::new();
                            let empty_label = Label::new(Some("No networks found"));
                            empty_label.add_css_class("dim-label");
                            empty_label.set_margin_top(20);
                            empty_label.set_margin_bottom(20);
                            empty_row.set_child(Some(&empty_label));
                            list.append(&empty_row);
                        } else {
                            for network in networks {
                                let row = Self::create_wifi_row(&network);
                                list.append(&row);
                            }
                        }
                        break;
                    }
                }
            }
        }
    }

    fn create_wifi_row(network: &WifiNetwork) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("network-wifi-row");

        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(5);
        hbox.set_margin_end(5);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);

        // Signal icon
        let signal_icon = Image::from_icon_name(Self::get_wifi_signal_icon(network.signal));
        signal_icon.set_pixel_size(16);
        hbox.append(&signal_icon);

        // Network name and info
        let info_box = Box::new(Orientation::Vertical, 2);
        info_box.set_hexpand(true);

        let name_label = Label::new(Some(&network.ssid));
        name_label.set_halign(gtk4::Align::Start);
        if network.connected {
            name_label.add_css_class("network-connected");
        }
        info_box.append(&name_label);

        let status_label = if network.connected {
            Label::new(Some("Connected"))
        } else {
            Label::new(Some(&format!(
                "{}%{}",
                network.signal,
                if network.secured { " • Secured" } else { "" }
            )))
        };
        status_label.set_halign(gtk4::Align::Start);
        status_label.add_css_class("network-wifi-status");
        info_box.append(&status_label);

        hbox.append(&info_box);

        // Connect/Disconnect button
        if !network.connected {
            let connect_button = Button::with_label("Connect");
            connect_button.add_css_class("network-connect-button");

            let ssid = network.ssid.clone();
            let secured = network.secured;
            connect_button.connect_clicked(move |button| {
                if secured {
                    Self::show_password_dialog(&ssid, button);
                } else {
                    Self::connect_to_wifi(&ssid, None);
                }
            });

            hbox.append(&connect_button);
        } else {
            let disconnect_button = Button::with_label("Disconnect");
            disconnect_button.add_css_class("network-disconnect-button");

            let ssid = network.ssid.clone();
            disconnect_button.connect_clicked(move |_| {
                Self::disconnect_wifi(&ssid);
            });

            hbox.append(&disconnect_button);
        }

        row.set_child(Some(&hbox));
        row
    }

    fn show_password_dialog(ssid: &str, button: &Button) {
        // Create a simple password dialog
        let dialog = gtk4::Window::new();
        dialog.set_title(Some(&format!("Connect to {}", ssid)));
        dialog.set_modal(true);
        dialog.set_resizable(false);
        dialog.set_default_size(300, 150);

        // Find the parent window
        if let Some(native) = button.native() {
            if let Some(window) = native.downcast_ref::<gtk4::Window>() {
                dialog.set_transient_for(Some(window));
            }
        }

        let vbox = Box::new(Orientation::Vertical, 10);
        vbox.set_margin_top(20);
        vbox.set_margin_bottom(20);
        vbox.set_margin_start(20);
        vbox.set_margin_end(20);

        let label = Label::new(Some("Enter WiFi password:"));
        label.set_halign(gtk4::Align::Start);
        vbox.append(&label);

        let password_entry = Entry::new();
        password_entry.set_visibility(false);
        password_entry.set_placeholder_text(Some("Password"));
        vbox.append(&password_entry);

        let button_box = Box::new(Orientation::Horizontal, 10);
        button_box.set_halign(gtk4::Align::End);
        button_box.set_margin_top(10);

        let cancel_button = Button::with_label("Cancel");
        let connect_button = Button::with_label("Connect");
        connect_button.add_css_class("suggested-action");

        button_box.append(&cancel_button);
        button_box.append(&connect_button);
        vbox.append(&button_box);

        dialog.set_child(Some(&vbox));

        // Handle button clicks
        let dialog_weak = dialog.downgrade();
        cancel_button.connect_clicked(move |_| {
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });

        let dialog_weak2 = dialog.downgrade();
        let ssid = ssid.to_string();
        let password_entry_weak = password_entry.downgrade();
        let ssid_for_connect = ssid.clone();
        connect_button.connect_clicked(move |_| {
            if let Some(entry) = password_entry_weak.upgrade() {
                let password = entry.text();
                if !password.is_empty() {
                    Self::connect_to_wifi(&ssid_for_connect, Some(&password));
                    if let Some(dialog) = dialog_weak2.upgrade() {
                        dialog.close();
                    }
                }
            }
        });

        // Handle Enter key
        let dialog_weak3 = dialog.downgrade();
        let ssid_for_enter = ssid.clone();
        password_entry.connect_activate(move |entry| {
            let password = entry.text();
            if !password.is_empty() {
                Self::connect_to_wifi(&ssid_for_enter, Some(&password));
                if let Some(dialog) = dialog_weak3.upgrade() {
                    dialog.close();
                }
            }
        });

        // Handle Escape key
        let controller = gtk4::EventControllerKey::new();
        let dialog_weak4 = dialog.downgrade();
        controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(dialog) = dialog_weak4.upgrade() {
                    dialog.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        dialog.add_controller(controller);

        dialog.present();
        password_entry.grab_focus();
    }

    fn get_wifi_networks() -> Vec<WifiNetwork> {
        let mut networks = Vec::new();

        // Get current SSID to mark as connected with timeout
        let current_ssid = {
            let mut cmd = Command::new("iwgetid");
            cmd.arg("-r");
            
            // Set timeout for the child process
            let output = match std::process::Command::new("timeout")
                .args(&["1", "iwgetid", "-r"])
                .output() {
                Ok(output) if output.status.success() => {
                    String::from_utf8_lossy(&output.stdout).trim().to_string()
                },
                _ => String::new(),
            };
            output
        };

        // Use nmcli to get WiFi networks with timeout
        let output = match std::process::Command::new("timeout")
            .args(&[
                "2", // 2 second timeout
                "nmcli",
                "-t",
                "-f",
                "SSID,SIGNAL,SECURITY,BSSID",
                "device",
                "wifi",
                "list",
            ])
            .output() {
            Ok(output) if output.status.success() => output,
            _ => {
                return networks; // Return empty list on timeout or failure
            }
        };
            
        let output_str = String::from_utf8_lossy(&output.stdout);
        let mut seen_ssids = std::collections::HashSet::new();

        for line in output_str.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 {
                let ssid = parts[0].to_string();

                // Skip empty SSIDs and duplicates
                if ssid.is_empty() || !seen_ssids.insert(ssid.clone()) {
                    continue;
                }

                if let Ok(signal) = parts[1].parse::<u8>() {
                    let secured = !parts[2].is_empty() && parts[2] != "--";
                    let connected = ssid == current_ssid;
                    let bssid = parts[3].to_string();

                    networks.push(WifiNetwork {
                        ssid,
                        signal,
                        secured,
                        connected,
                        bssid,
                    });
                }
            }
        }

        // Sort by signal strength (connected first, then by signal)
        networks.sort_by(|a, b| match (a.connected, b.connected) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.signal.cmp(&a.signal),
        });

        networks
    }

    fn get_wifi_signal_icon(signal: u8) -> &'static str {
        match signal {
            0..=25 => "network-wireless-signal-weak-symbolic",
            26..=50 => "network-wireless-signal-ok-symbolic",
            51..=75 => "network-wireless-signal-good-symbolic",
            _ => "network-wireless-signal-excellent-symbolic",
        }
    }

    fn connect_to_wifi(ssid: &str, password: Option<&str>) {
        let mut cmd = Command::new("nmcli");
        cmd.args(&["device", "wifi", "connect", ssid]);

        if let Some(pwd) = password {
            cmd.args(&["password", pwd]);
        }

        match cmd.output() {
            Ok(output) => {
                if !output.status.success() {
                    warn!(
                        "Failed to connect to WiFi: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                warn!("Failed to execute nmcli: {}", e);
            }
        }
    }

    fn disconnect_wifi(ssid: &str) {
        match Command::new("nmcli")
            .args(&["connection", "down", ssid])
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    warn!(
                        "Failed to disconnect WiFi: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                warn!("Failed to execute nmcli: {}", e);
            }
        }
    }

    fn get_network_info() -> NetworkInfo {
        let mut info = NetworkInfo {
            interface: "none".to_string(),
            connection_type: ConnectionType::Disconnected,
            ssid: None,
            signal_strength: None,
            ip_address: None,
            ipv6_address: None,
            // Public IP fields removed to fix deadlocks
            connected: false,
            vpn_active: false,
            vpn_name: None,
        };

        // Check for active VPN with timeout
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&[
                "1", // 1 second timeout
                "nmcli",
                "-t",
                "-f",
                "TYPE,NAME,STATE",
                "connection",
                "show",
                "--active",
            ])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 3 {
                    if parts[0] == "vpn" && parts[2] == "activated" {
                        info.vpn_active = true;
                        info.vpn_name = Some(parts[1].to_string());
                        break;
                    }
                }
            }
        }

        // Get primary network connection with timeout
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["1", "ip", "route", "show", "default"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);

            if let Some(line) = output_str.lines().next() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if let Some(dev_idx) = parts.iter().position(|&x| x == "dev") {
                    if let Some(interface) = parts.get(dev_idx + 1) {
                        let interface_info = Self::get_interface_info(interface);
                        info = interface_info;

                        // Restore VPN info
                        if let Ok(output) = Command::new("nmcli")
                            .args(&[
                                "-t",
                                "-f",
                                "TYPE,NAME,STATE",
                                "connection",
                                "show",
                                "--active",
                            ])
                            .output()
                        {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            for line in output_str.lines() {
                                let parts: Vec<&str> = line.split(':').collect();
                                if parts.len() >= 3 && parts[0] == "vpn" && parts[2] == "activated"
                                {
                                    info.vpn_active = true;
                                    info.vpn_name = Some(parts[1].to_string());
                                    break;
                                }
                            }
                        }

                        return info;
                    }
                }
            }
        }

        info
    }

    fn get_vpn_connections() -> Vec<VpnConnection> {
        let mut vpns = Vec::new();

        // Get all VPN connections with timeout
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["1", "nmcli", "-t", "-f", "NAME,UUID,TYPE,STATE", "connection", "show"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 4 && parts[2] == "vpn" {
                    vpns.push(VpnConnection {
                        name: parts[0].to_string(),
                        uuid: parts[1].to_string(),
                        active: parts[3] == "activated",
                    });
                }
            }
        }

        vpns
    }

    fn toggle_vpn(uuid: &str, connect: bool) {
        let action = if connect { "up" } else { "down" };
        let _ = Command::new("nmcli")
            .args(&["connection", action, uuid])
            .spawn();
    }

    // Public IP fetch functionality removed to fix deadlocks
    
    // Async IP fetch functionality removed to fix deadlocks

    fn get_interface_info(interface: &str) -> NetworkInfo {
        let mut info = NetworkInfo {
            interface: interface.to_string(),
            connection_type: ConnectionType::Disconnected,
            ssid: None,
            signal_strength: None,
            ip_address: None,
            ipv6_address: None,
            // Public IP fields removed to fix deadlocks
            connected: true,
            vpn_active: false,
            vpn_name: None,
        };

        // Determine connection type
        if interface.starts_with("wl") || interface.starts_with("wlan") {
            info.connection_type = ConnectionType::Wifi;

            // Get WiFi info using iw or nmcli
            if let Ok(output) = Command::new("iw")
                .args(&["dev", interface, "link"])
                .output()
            {
                let output_str = String::from_utf8_lossy(&output.stdout);

                // Parse SSID
                for line in output_str.lines() {
                    if line.trim().starts_with("SSID:") {
                        info.ssid = line.split(':').nth(1).map(|s| s.trim().to_string());
                    }
                }

                // Get signal strength
                if let Ok(signal_output) = Command::new("iw")
                    .args(&["dev", interface, "station", "dump"])
                    .output()
                {
                    let signal_str = String::from_utf8_lossy(&signal_output.stdout);
                    for line in signal_str.lines() {
                        if line.contains("signal:") {
                            if let Some(signal_dbm) = line
                                .split("signal:")
                                .nth(1)
                                .and_then(|s| s.split_whitespace().next())
                                .and_then(|s| s.parse::<i32>().ok())
                            {
                                // Convert dBm to percentage (rough approximation)
                                info.signal_strength = Some(Self::dbm_to_percentage(signal_dbm));
                            }
                        }
                    }
                }
            } else if let Ok(output) = Command::new("nmcli")
                .args(&["-t", "-f", "ACTIVE,SSID,SIGNAL", "dev", "wifi"])
                .output()
            {
                // Fallback to nmcli if iw is not available
                let output_str = String::from_utf8_lossy(&output.stdout);
                for line in output_str.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.get(0) == Some(&"yes") {
                        info.ssid = parts.get(1).map(|s| s.to_string());
                        info.signal_strength = parts.get(2).and_then(|s| s.parse::<u8>().ok());
                        break;
                    }
                }
            }
        } else if interface.starts_with("en") || interface.starts_with("eth") {
            info.connection_type = ConnectionType::Ethernet;
        }

        // Get local IP addresses
        if let Ok(output) = Command::new("ip")
            .args(&["addr", "show", interface])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                let line_trimmed = line.trim();
                if line_trimmed.starts_with("inet ") && info.ip_address.is_none() {
                    let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
                    if let Some(ip) = parts.get(1) {
                        // Filter out IPv4 link-local addresses (169.254.x.x)
                        let ip_clean = ip.split('/').next().unwrap_or(ip).to_string();
                        if !ip_clean.starts_with("169.254.") {
                            info.ip_address = Some(ip_clean);
                        }
                    }
                } else if line_trimmed.starts_with("inet6 ") && info.ipv6_address.is_none() {
                    let parts: Vec<&str> = line_trimmed.split_whitespace().collect();
                    if let Some(ip) = parts.get(1) {
                        let ip_clean = ip.split('/').next().unwrap_or(ip).to_string();
                        // Filter out link-local addresses (fe80::) and temporary addresses
                        if !ip_clean.starts_with("fe80:")
                            && !parts.iter().any(|p| *p == "temporary")
                        {
                            info.ipv6_address = Some(ip_clean);
                        }
                    }
                }
            }
        }

        // Public IPs will be fetched asynchronously in update_network
        info
    }

    fn dbm_to_percentage(dbm: i32) -> u8 {
        // Convert dBm to percentage (rough approximation)
        let percentage = if dbm >= -50 {
            100
        } else if dbm >= -60 {
            90
        } else if dbm >= -70 {
            75
        } else if dbm >= -80 {
            50
        } else if dbm >= -90 {
            25
        } else {
            10
        };
        percentage as u8
    }

    fn get_network_icon_name(info: &NetworkInfo) -> String {
        match info.connection_type {
            ConnectionType::Wifi => {
                if let Some(signal) = info.signal_strength {
                    match signal {
                        0..=25 => "network-wireless-signal-weak-symbolic",
                        26..=50 => "network-wireless-signal-ok-symbolic",
                        51..=75 => "network-wireless-signal-good-symbolic",
                        _ => "network-wireless-signal-excellent-symbolic",
                    }
                } else {
                    "network-wireless-symbolic"
                }
            }
            ConnectionType::Ethernet => "network-wired-symbolic",
            ConnectionType::Disconnected => "network-offline-symbolic",
        }
        .to_string()
    }

    fn open_network_settings() {
        // Try different network settings commands
        let commands = vec![
            ("gnome-control-center", vec!["network"]),
            ("nm-connection-editor", vec![]),
            ("nmtui", vec![]),
        ];

        for (cmd, args) in commands {
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }

        warn!("Could not find network settings application");
    }

    fn toggle_wifi() {
        // Toggle WiFi using nmcli
        if let Ok(output) = Command::new("nmcli").args(&["radio", "wifi"]).output() {
            let status = String::from_utf8_lossy(&output.stdout);
            let new_state = if status.trim() == "enabled" {
                "off"
            } else {
                "on"
            };

            let _ = Command::new("nmcli")
                .args(&["radio", "wifi", new_state])
                .spawn();
        }
    }

    fn create_address_row(label_text: &str, address: &str) -> Box {
        let container = Box::new(Orientation::Horizontal, 5);
        container.add_css_class("network-address-box");

        // Create label with address
        let label = Label::new(Some(label_text));
        label.set_halign(gtk4::Align::Start);
        label.set_width_chars(10);
        container.append(&label);

        // Create value label
        let value_label = Label::new(Some(address));
        value_label.set_halign(gtk4::Align::Start);
        value_label.set_selectable(true);
        value_label.set_hexpand(true);
        value_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        container.append(&value_label);

        // Create copy button
        let copy_button = Button::from_icon_name("edit-copy-symbolic");
        copy_button.set_tooltip_text(Some(&format!("Copy {}", address)));
        copy_button.add_css_class("network-copy-button");

        // Clone address for closure
        let address_copy = address.to_string();

        // Connect button click
        copy_button.connect_clicked(move |_| {
            // Copy to clipboard
            if let Some(display) = Display::default() {
                let clipboard = display.clipboard();
                clipboard.set_text(&address_copy);
                info!("Copied to clipboard: {}", address_copy);
            }
        });

        container.append(&copy_button);
        container
    }

    pub fn widget(&self) -> &Button {
        &self.button
    }
    
    /// Try multiple methods to get the current WiFi SSID
    fn get_wifi_ssid(interface: &str) -> Option<String> {
        info!("Trying to find SSID for interface {}", interface);
        
        // Method 1: Try iwgetid first
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["0.5", "iwgetid", "-r"])
            .output()
        {
            let ssid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ssid.is_empty() {
                info!("Found SSID via iwgetid: {}", ssid);
                return Some(ssid);
            }
        }
        
        // Method 2: Try iwgetid with interface
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["0.5", "iwgetid", interface, "-r"])
            .output()
        {
            let ssid = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !ssid.is_empty() {
                info!("Found SSID via iwgetid with interface: {}", ssid);
                return Some(ssid);
            }
        }
        
        // Method 3: Try nmcli
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["0.5", "nmcli", "-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 2 && parts[1] == interface {
                    info!("Found SSID via nmcli connection: {}", parts[0]);
                    return Some(parts[0].to_string());
                }
            }
        }
        
        // Method 4: Try nmcli device wifi
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["0.5", "nmcli", "-t", "-f", "ACTIVE,SSID", "dev", "wifi"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.get(0) == Some(&"yes") && parts.len() >= 2 {
                    info!("Found SSID via nmcli dev wifi: {}", parts[1]);
                    return Some(parts[1].to_string());
                }
            }
        }
        
        // Method 5: Try iw dev
        if let Ok(output) = std::process::Command::new("timeout")
            .args(&["0.5", "iw", "dev", interface, "link"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.trim().starts_with("SSID:") {
                    if let Some(ssid) = line.split(':').nth(1) {
                        let ssid = ssid.trim().to_string();
                        if !ssid.is_empty() {
                            info!("Found SSID via iw dev: {}", ssid);
                            return Some(ssid);
                        }
                    }
                }
            }
        }
        
        info!("Could not find SSID with any method");
        None
    }
}
