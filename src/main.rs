use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow};
use gtk4_layer_shell::{Layer, LayerShell};
use tracing::{info, error, warn};
use tracing_subscriber;
use std::rc::Rc;
use std::cell::RefCell;

mod panel;
mod widgets;
mod config;

use panel::Panel;
use config::PanelConfig;

fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    info!("Starting Niri Panel");

    // Check if GSettings schemas are available
    match gtk4::gio::SettingsSchemaSource::default() {
        Some(_) => info!("GSettings schema source found."),
        None => warn!("GSettings schema source not found. Some functionality may be limited.")
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
    let panel = Panel::new(config, window_weak, active_popovers)?;
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
        },
        Err(e) => {
            error!("Failed to load CSS data: {}", e);
        }
    }
    
    window.present();
    
    info!("Niri Panel initialized successfully");
    Ok(())
}