use anyhow::Result;
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Image, Label, Orientation, Popover, Separator, ScrolledWindow,
};
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::io::BufRead;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use tracing::{error, info, warn};

use crate::widgets::Widget as WidgetTrait;

pub struct Servers {
    button: Button,
    popover: Popover,
}

#[derive(Debug, Clone)]
struct SSHConnection {
    name: String,
    hostname: String,
    user: Option<String>,
    port: Option<u16>,
}

impl Servers {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("servers");

        // Use server icon
        let icon = Image::from_icon_name("network-server-symbolic");
        icon.set_icon_size(gtk4::IconSize::Large);
        button.set_child(Some(&icon));

        // Create popover for servers
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("servers-popover");
        popover.set_autohide(true);

        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!(
                    "Servers popover shown - keyboard mode set to OnDemand (active popovers: {})",
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
                    info!("Servers popover hidden - keyboard mode set to None");
                }
            } else {
                info!(
                    "Servers popover hidden - keeping keyboard mode (active popovers: {})",
                    count
                );
            }
        });

        // Create servers content
        let servers_content = Self::create_servers_content();
        popover.set_child(Some(&servers_content));

        // Add Escape key handler to close popover
        let escape_controller = gtk4::EventControllerKey::new();
        let popover_weak_escape = popover.downgrade();
        escape_controller.set_propagation_phase(gtk4::PropagationPhase::Capture);
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
        let popover_clone = popover.clone();
        button.connect_clicked(move |_| {
            popover_clone.popup();
        });

        Ok(Self { button, popover })
    }

    fn create_servers_content() -> ScrolledWindow {
        let scrolled_window = ScrolledWindow::new();
        scrolled_window.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled_window.set_max_content_height(500);
        scrolled_window.set_propagate_natural_height(true);

        let list_box = Box::new(Orientation::Vertical, 0);

        // Add SSH connections section
        let ssh_label = Label::new(Some("SSH Connections"));
        ssh_label.set_halign(gtk4::Align::Start);
        ssh_label.add_css_class("servers-section-label");
        ssh_label.set_margin_start(10);
        ssh_label.set_margin_top(10);
        ssh_label.set_margin_bottom(5);
        list_box.append(&ssh_label);

        // Get SSH connections from config file
        let ssh_connections = Self::get_ssh_connections();

        if ssh_connections.is_empty() {
            let empty_label = Label::new(Some("No SSH connections found"));
            empty_label.add_css_class("dim-label");
            empty_label.set_margin_start(20);
            empty_label.set_margin_top(10);
            empty_label.set_margin_bottom(10);
            list_box.append(&empty_label);
        } else {
            for connection in ssh_connections {
                let button = Self::create_ssh_button(&connection);
                list_box.append(&button);
            }
        }

        // Add "Add SSH Connection" button
        let separator = Separator::new(Orientation::Horizontal);
        separator.set_margin_top(10);
        separator.set_margin_bottom(10);
        list_box.append(&separator);

        let add_button = Button::new();
        add_button.add_css_class("servers-item");
        let add_box = Box::new(Orientation::Horizontal, 10);
        add_box.set_margin_start(10);
        add_box.set_margin_end(10);
        add_box.set_margin_top(8);
        add_box.set_margin_bottom(8);

        let add_icon = Image::from_icon_name("list-add-symbolic");
        add_icon.set_pixel_size(16);
        add_box.append(&add_icon);

        let add_label = Label::new(Some("Edit SSH Config"));
        add_label.set_hexpand(true);
        add_label.set_halign(gtk4::Align::Start);
        add_box.append(&add_label);

        add_button.set_child(Some(&add_box));
        add_button.connect_clicked(move |_| {
            Self::open_ssh_config();
        });
        list_box.append(&add_button);

        scrolled_window.set_child(Some(&list_box));
        scrolled_window
    }

    fn get_ssh_connections() -> Vec<SSHConnection> {
        let mut connections = Vec::new();
        let config_path = Self::get_ssh_config_path();

        if !config_path.exists() {
            // Create SSH config directory if it doesn't exist
            if let Some(config_dir) = config_path.parent() {
                if !config_dir.exists() {
                    if let Err(e) = std::fs::create_dir_all(config_dir) {
                        error!("Failed to create SSH config directory: {}", e);
                        return connections;
                    }
                }
            }

            // Create empty config file
            if let Err(e) = std::fs::File::create(&config_path) {
                error!("Failed to create SSH config file: {}", e);
                return connections;
            }
        }

        // Open the config file
        match std::fs::File::open(&config_path) {
            Ok(file) => {
                let reader = std::io::BufReader::new(file);
                let mut current_host: Option<SSHConnection> = None;

                for line in reader.lines() {
                    if let Ok(line) = line {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') {
                            continue;
                        }

                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() < 2 {
                            continue;
                        }

                        let keyword = parts[0].to_lowercase();
                        let value = parts[1];

                        if keyword == "host" && !value.contains('*') {
                            // Finish previous host if any
                            if let Some(host) = current_host.take() {
                                if !host.hostname.is_empty() {
                                    connections.push(host);
                                }
                            }

                            // Start new host
                            current_host = Some(SSHConnection {
                                name: value.to_string(),
                                hostname: String::new(),
                                user: None,
                                port: None,
                            });
                        } else if let Some(host) = &mut current_host {
                            if keyword == "hostname" {
                                host.hostname = value.to_string();
                            } else if keyword == "user" {
                                host.user = Some(value.to_string());
                            } else if keyword == "port" {
                                if let Ok(port) = value.parse::<u16>() {
                                    host.port = Some(port);
                                }
                            }
                        }
                    }
                }

                // Add the last host if any
                if let Some(host) = current_host {
                    if !host.hostname.is_empty() {
                        connections.push(host);
                    }
                }
            }
            Err(e) => {
                error!("Failed to open SSH config file: {}", e);
            }
        }

        connections
    }

    fn get_ssh_config_path() -> PathBuf {
        if let Some(home_dir) = dirs::home_dir() {
            home_dir.join(".ssh").join("config")
        } else {
            PathBuf::from("/etc/ssh/ssh_config")
        }
    }

    fn create_ssh_button(connection: &SSHConnection) -> Button {
        let button = Button::new();
        button.add_css_class("servers-item");

        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(10);
        hbox.set_margin_end(10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);

        let icon = Image::from_icon_name("network-server-symbolic");
        icon.set_pixel_size(16);
        hbox.append(&icon);

        let mut display_name = connection.name.clone();
        if let Some(user) = &connection.user {
            display_name = format!("{} ({})", display_name, user);
        }

        let label = Label::new(Some(&display_name));
        label.set_hexpand(true);
        label.set_halign(gtk4::Align::Start);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        hbox.append(&label);

        button.set_child(Some(&hbox));

        let connection_clone = connection.clone();
        button.connect_clicked(move |_| {
            Self::open_ssh_connection(&connection_clone);
        });

        button
    }

    fn open_ssh_connection(connection: &SSHConnection) {
        // Build SSH command arguments
        let mut ssh_args = Vec::new();

        // Add user@host
        let mut host_string = String::new();
        if let Some(user) = &connection.user {
            host_string.push_str(user);
            host_string.push('@');
        }
        host_string.push_str(&connection.hostname);
        ssh_args.push(host_string.clone());

        // Add port if specified
        if let Some(port) = connection.port {
            ssh_args.push("-p".to_string());
            ssh_args.push(port.to_string());
        }

        // Display connection info for logging
        let display_name = if let Some(user) = &connection.user {
            format!("{}@{}", user, connection.hostname)
        } else {
            connection.hostname.clone()
        };

        info!("Opening SSH connection to {}", display_name);

        // Try to open with alacritty first (preferred)
        let mut alacritty_args = vec!["-e", "ssh"];
        alacritty_args.extend(ssh_args.iter().map(|s| s.as_str()));

        if Command::new("alacritty")
            .args(&alacritty_args)
            .spawn()
            .is_ok()
        {
            info!("Opened SSH connection with alacritty");
            return;
        }

        // Fallbacks to other terminals
        let terminals = vec![
            ("kitty", vec!["-e", "ssh"]),
            ("gnome-terminal", vec!["--", "ssh"]),
            ("xterm", vec!["-e", "ssh"]),
            ("konsole", vec!["-e", "ssh"]),
            ("terminator", vec!["-e", "ssh"]),
            ("terminology", vec!["-e", "ssh"]),
        ];

        for (terminal, base_args) in terminals {
            let mut args = base_args.clone();

            // Add SSH arguments
            args.extend(ssh_args.iter().map(|s| s.as_str()));

            if Command::new(terminal).args(&args).spawn().is_ok() {
                info!("Opened SSH connection with {}", terminal);
                return;
            }
        }

        // If we got here, no terminal worked
        error!(
            "Could not find terminal to open SSH connection to {}",
            display_name
        );
    }

    fn open_ssh_config() {
        let config_path = Self::get_ssh_config_path();

        // First try to open with a GUI editor
        let editors = vec![
            ("gedit", vec![]),
            ("kate", vec![]),
            ("mousepad", vec![]),
            ("pluma", vec![]),
            ("xed", vec![]),
        ];

        let path_str = config_path.to_string_lossy();

        for (cmd, mut args) in editors {
            args.push(path_str.as_ref());
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }

        // Fallback to terminal with editor
        let terminal_editors = vec![
            ("alacritty", vec!["-e", "nano", path_str.as_ref()]),
            ("gnome-terminal", vec!["--", "nano", path_str.as_ref()]),
            ("xterm", vec!["-e", "nano", path_str.as_ref()]),
        ];

        for (terminal, args) in terminal_editors {
            if Command::new(terminal).args(&args).spawn().is_ok() {
                return;
            }
        }

        error!("Could not find editor to open SSH config: {}", path_str);
    }

    pub fn widget(&self) -> &Button {
        &self.button
    }
}

// Implementation of Widget trait
impl WidgetTrait for Servers {
    fn popover(&self) -> Option<&Popover> {
        Some(&self.popover)
    }
}