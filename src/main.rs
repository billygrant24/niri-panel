use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Layer, LayerShell};
use notify::{Event, EventKind};
use std::cell::RefCell;
use std::rc::Rc;
use tracing::{error, info, warn};
use tracing_subscriber;

mod config;
mod panel;
mod widgets;

use config::PanelConfig;
use panel::Panel;

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting Niri Panel");

    // Check if GSettings schemas are available
    match gtk4::gio::SettingsSchemaSource::default() {
        Some(_) => info!("GSettings schema source found."),
        None => warn!("GSettings schema source not found. Some functionality may be limited."),
    }
    // We continue anyway as the panel doesn't strictly require these schemas

    // Create GTK application
    let app = Application::builder()
        .application_id("org.niri.panel")
        .build();

    app.connect_activate(|app| {
        if let Err(e) = build_ui(app) {
            error!("Failed to build UI: {}", e);
        }
    });

    // Run the application
    app.run();
    Ok(())
}

fn build_ui(app: &Application) -> anyhow::Result<()> {
    // Load configuration
    let config = PanelConfig::load()?;

    // Create main window
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Niri Panel")
        .build();

    // Initialize layer shell
    window.init_layer_shell();
    window.set_layer(Layer::Top);
    window.set_anchor(gtk4_layer_shell::Edge::Top, true);
    window.set_anchor(gtk4_layer_shell::Edge::Left, true);
    window.set_anchor(gtk4_layer_shell::Edge::Right, true);

    // Set exclusive zone to reserve space
    window.set_exclusive_zone(config.height);

    // Set keyboard mode to None initially (panel doesn't capture keyboard)
    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);

    // Set window properties
    window.set_height_request(config.height);
    window.set_margin(gtk4_layer_shell::Edge::Top, 0);
    window.set_margin(gtk4_layer_shell::Edge::Bottom, 0);
    window.set_margin(gtk4_layer_shell::Edge::Left, 0);
    window.set_margin(gtk4_layer_shell::Edge::Right, 0);

    // Create shared state for keyboard mode management
    let window_weak = window.downgrade();
    let active_popovers = Rc::new(RefCell::new(0));

    // Create and setup panel with keyboard mode management
    let panel = Panel::new(config.clone(), window_weak.clone(), active_popovers.clone())?;
    window.set_child(Some(panel.container()));

    // Apply CSS styling
    let css_provider = gtk4::CssProvider::new();

    // Use a safer way to load CSS that handles errors gracefully
    match std::str::from_utf8(include_bytes!("../assets/style.css")) {
        Ok(css_data) => {
            css_provider.load_from_data(css_data);

            if let Some(display) = gtk4::gdk::Display::default() {
                gtk4::style_context_add_provider_for_display(
                    &display,
                    &css_provider,
                    gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
                info!("CSS styles loaded successfully");
            } else {
                warn!("Could not get default display for CSS styling");
            }
        }
        Err(e) => {
            error!("Failed to load CSS data: {}", e);
        }
    }

    // Set up config file watching
    if let Ok(rx) = PanelConfig::watch_config_changes() {
        let app_weak = app.downgrade();
        let config_path = match PanelConfig::config_path() {
            Ok(path) => path.to_string_lossy().to_string(),
            Err(_) => String::from("config file"),
        };

        // Add an event source to handle config file changes
        let channel = gtk4::glib::MainContext::channel(gtk4::glib::Priority::DEFAULT);
        let (sender, receiver) = channel;

        // Spawn a thread to listen for file events and send them to the main context
        std::thread::spawn(move || {
            while let Ok(event) = rx.recv() {
                let _ = sender.send(event);
            }
        });

        // Handle events in the main thread
        receiver.attach(None, move |event: Event| {
            if let Some(app) = app_weak.upgrade() {
                if let EventKind::Modify(_) | EventKind::Create(_) = event.kind {
                    // Check if the event is for our config file
                    for path in event.paths {
                        let path_str = path.to_string_lossy();
                        if path_str.ends_with("config.toml") {
                            info!("Config file changed: {}", path_str);
                            
                            // Try to load the new configuration
                            match PanelConfig::load() {
                                Ok(new_config) => {
                                    info!("Reloading configuration");
                                    
                                    // Get the current window
                                    let windows = app.windows();
                                    if !windows.is_empty() {
                                        // Assuming the first window is the panel
                                        let window = windows.first().unwrap();
                                        
                                        // Convert the window to ApplicationWindow
                                        if let Some(app_window) = window.downcast_ref::<ApplicationWindow>() {
                                            // Create window weak reference
                                            let window_weak = app_window.downgrade();
                                            let active_popovers = Rc::new(RefCell::new(0));
                                            
                                            // Create new panel with new config
                                            if let Ok(panel) = Panel::new(
                                                new_config.clone(),
                                                window_weak,
                                                active_popovers
                                            ) {
                                                // Update window height if changed
                                                window.set_height_request(new_config.height);
                                                app_window.set_exclusive_zone(new_config.height);
                                                
                                                // Replace the old panel with the new one
                                                window.set_child(Some(panel.container()));
                                                info!("Panel reloaded with new configuration");
                                            } else {
                                                error!("Failed to create new panel with updated config");
                                            }
                                        }
                                    }
                                },
                                Err(e) => {
                                    error!("Failed to load new configuration: {}", e);
                                }
                            }
                        }
                    }
                }
            }
            
            // Continue receiving events
            gtk4::glib::ControlFlow::Continue
        });

        info!("Watching for changes to {}", config_path);
    } else {
        warn!("Failed to set up config file watcher");
    }

    window.present();

    info!("Niri Panel initialized successfully");
    Ok(())
}
