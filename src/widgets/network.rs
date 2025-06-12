use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, ListBox, ListBoxRow, Switch};
use glib::timeout_add_seconds_local;
use anyhow::Result;
use std::process::Command;
use std::fs;
use std::path::Path;
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

impl Network {
    pub fn new() -> Result<Self> {
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
        popover.set_has_arrow(false);
        
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(10);
        popover_box.set_margin_bottom(10);
        popover_box.set_margin_start(10);
        popover_box.set_margin_end(10);
        popover_box.set_size_request(300, -1);
        
        popover.set_child(Some(&popover_box));
        
        // Update network status immediately
        Self::update_network(&icon, &vpn_icon, &label, &popover_box);
        
        // Update every 5 seconds
        let icon_weak = icon.downgrade();
        let vpn_icon_weak = vpn_icon.downgrade();
        let label_weak = label.downgrade();
        let popover_box_weak = popover_box.downgrade();
        timeout_add_seconds_local(5, move || {
            if let (Some(icon), Some(vpn_icon), Some(label), Some(popover_box)) = 
                (icon_weak.upgrade(), vpn_icon_weak.upgrade(), label_weak.upgrade(), popover_box_weak.upgrade()) {
                Self::update_network(&icon, &vpn_icon, &label, &popover_box);
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        
        // Show popover on click
        button.connect_clicked(move |_| {
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    fn update_network(icon: &Image, vpn_icon: &Image, label: &Label, popover_box: &Box) {
        let info = Self::get_network_info();
        let vpn_connections = Self::get_vpn_connections();
        
        // Update icon
        let icon_name = Self::get_network_icon_name(&info);
        icon.set_from_icon_name(Some(&icon_name));
        
        // Update VPN icon
        if info.vpn_active {
            vpn_icon.set_from_icon_name(Some("network-vpn-symbolic"));
            vpn_icon.set_visible(true);
            vpn_icon.set_tooltip_text(Some(&format!("VPN: {}", info.vpn_name.as_ref().unwrap_or(&"Active".to_string()))));
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
            if info.connected { "Connected" } else { "Disconnected" }
        )));
        status_label.set_halign(gtk4::Align::Start);
        status_label.add_css_class("network-status");
        popover_box.append(&status_label);
        
        // Interface
        let interface_label = Label::new(Some(&format!("Interface: {}", info.interface)));
        interface_label.set_halign(gtk4::Align::Start);
        popover_box.append(&interface_label);
        
        // SSID for WiFi
        if let Some(ssid) = &info.ssid {
            let ssid_label = Label::new(Some(&format!("Network: {}", ssid)));
            ssid_label.set_halign(gtk4::Align::Start);
            popover_box.append(&ssid_label);
        }
        
        // Signal strength for WiFi
        if let Some(signal) = info.signal_strength {
            let signal_label = Label::new(Some(&format!("Signal: {}%", signal)));
            signal_label.set_halign(gtk4::Align::Start);
            popover_box.append(&signal_label);
        }
        
        // IP Address
        if let Some(ip) = &info.ip_address {
            let ip_label = Label::new(Some(&format!("IP: {}", ip)));
            ip_label.set_halign(gtk4::Align::Start);
            popover_box.append(&ip_label);
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
            vpn_label.add_css_class("network-vpn-title");
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
        if info.connection_type == ConnectionType::Wifi || info.connection_type == ConnectionType::Disconnected {
            let wifi_toggle = Button::with_label(
                if info.connection_type == ConnectionType::Wifi { "Disconnect WiFi" } else { "Connect WiFi" }
            );
            wifi_toggle.connect_clicked(move |_| {
                Self::toggle_wifi();
            });
            actions_box.append(&wifi_toggle);
        }
        
        popover_box.append(&actions_box);
    }
    
    fn get_network_info() -> NetworkInfo {
        let mut info = NetworkInfo {
            interface: "none".to_string(),
            connection_type: ConnectionType::Disconnected,
            ssid: None,
            signal_strength: None,
            ip_address: None,
            connected: false,
            vpn_active: false,
            vpn_name: None,
        };
        
        // Check for active VPN
        if let Ok(output) = Command::new("nmcli")
            .args(&["-t", "-f", "TYPE,NAME,STATE", "connection", "show", "--active"])
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
        
        // Get primary network connection
        if let Ok(output) = Command::new("ip")
            .args(&["route", "show", "default"])
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
                            .args(&["-t", "-f", "TYPE,NAME,STATE", "connection", "show", "--active"])
                            .output()
                        {
                            let output_str = String::from_utf8_lossy(&output.stdout);
                            for line in output_str.lines() {
                                let parts: Vec<&str> = line.split(':').collect();
                                if parts.len() >= 3 && parts[0] == "vpn" && parts[2] == "activated" {
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
        
        // Get all VPN connections
        if let Ok(output) = Command::new("nmcli")
            .args(&["-t", "-f", "NAME,UUID,TYPE,STATE", "connection", "show"])
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
    
    fn get_interface_info(interface: &str) -> NetworkInfo {
        let mut info = NetworkInfo {
            interface: interface.to_string(),
            connection_type: ConnectionType::Disconnected,
            ssid: None,
            signal_strength: None,
            ip_address: None,
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
                        info.ssid = line.split(':')
                            .nth(1)
                            .map(|s| s.trim().to_string());
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
                            if let Some(signal_dbm) = line.split("signal:")
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
                        info.signal_strength = parts.get(2)
                            .and_then(|s| s.parse::<u8>().ok());
                        break;
                    }
                }
            }
        } else if interface.starts_with("en") || interface.starts_with("eth") {
            info.connection_type = ConnectionType::Ethernet;
        }
        
        // Get IP address
        if let Ok(output) = Command::new("ip")
            .args(&["addr", "show", interface])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.trim().starts_with("inet ") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if let Some(ip) = parts.get(1) {
                        info.ip_address = Some(ip.split('/').next().unwrap_or(ip).to_string());
                        break;
                    }
                }
            }
        }
        
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
        }.to_string()
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
        if let Ok(output) = Command::new("nmcli")
            .args(&["radio", "wifi"])
            .output()
        {
            let status = String::from_utf8_lossy(&output.stdout);
            let new_state = if status.trim() == "enabled" { "off" } else { "on" };
            
            let _ = Command::new("nmcli")
                .args(&["radio", "wifi", new_state])
                .spawn();
        }
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}
