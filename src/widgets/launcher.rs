use gtk4::prelude::*;
use gtk4::{Button, Image, Popover, Box, Orientation, SearchEntry, ListBox, ListBoxRow, Label, ScrolledWindow, ApplicationWindow};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use anyhow::Result;
use std::process::Command;
use std::fs;
use std::path::Path;
use tracing::{warn, info};
use std::rc::Rc;
use std::cell::RefCell;

pub struct Launcher {
    button: Button,
}

#[derive(Clone, Debug)]
struct AppInfo {
    name: String,
    exec: String,
    icon: Option<String>,
    comment: Option<String>,
}

impl Launcher {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("launcher");
        
        // Use icon from theme - try multiple fallbacks
        let icon_names = vec![
            "view-app-grid-symbolic",
            "application-x-executable-symbolic", 
            "applications-other",
            "view-grid-symbolic"
        ];
        
        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                break;
            }
        }
        
        // If no icon found, use a text fallback
        if image.icon_name().is_none() {
            let label = Label::new(Some("☰"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }
        
        // Create popover for app launcher
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("launcher-popover");
        popover.set_autohide(true);
        popover.set_has_arrow(false);
        
        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Launcher popover shown - keyboard mode set to OnDemand (active popovers: {})", 
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
                    info!("Launcher popover hidden - keyboard mode set to None");
                }
            } else {
                info!("Launcher popover hidden - keeping keyboard mode (active popovers: {})", count);
            }
        });
        
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(10);
        popover_box.set_margin_bottom(10);
        popover_box.set_margin_start(10);
        popover_box.set_margin_end(10);
        popover_box.set_size_request(400, 500);
        
        // Search entry
        let search_entry = SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search applications..."));
        search_entry.add_css_class("launcher-search");
        search_entry.set_hexpand(true);
        search_entry.set_can_focus(true);
        popover_box.append(&search_entry);
        
        // Scrolled window for app list
        let scrolled_window = ScrolledWindow::new();
        scrolled_window.set_vexpand(true);
        scrolled_window.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        
        // App list
        let app_list = ListBox::new();
        app_list.add_css_class("launcher-list");
        app_list.set_selection_mode(gtk4::SelectionMode::Single);
        scrolled_window.set_child(Some(&app_list));
        
        popover_box.append(&scrolled_window);
        popover.set_child(Some(&popover_box));
        
        // Load applications
        let apps = Self::load_applications();
        let _apps_clone = apps.clone();
        
        // Populate initial list
        Self::populate_app_list(&app_list, &apps, "");
        
        // Handle search
        let app_list_weak = app_list.downgrade();
        let apps_for_search = apps.clone();
        search_entry.connect_search_changed(move |entry| {
            if let Some(list) = app_list_weak.upgrade() {
                let query = entry.text();
                Self::populate_app_list(&list, &apps_for_search, &query);
            }
        });
        
        // Handle app activation
        let popover_weak = popover.downgrade();
        app_list.connect_row_activated(move |_, row| {
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
            
            // Get the app info from the row
            if let Some(child) = row.child() {
                if let Some(box_widget) = child.downcast_ref::<Box>() {
                    // The exec command is stored in the widget name
                    let exec = box_widget.widget_name();
                    if !exec.is_empty() {
                        Self::launch_app(&exec);
                    }
                }
            }
        });
        
        // Handle Enter key in search
        let app_list_for_enter = app_list.clone();
        let popover_for_enter = popover.downgrade();
        search_entry.connect_activate(move |_| {
            if let Some(selected_row) = app_list_for_enter.selected_row() {
                if let Some(popover) = popover_for_enter.upgrade() {
                    popover.popdown();
                }
                
                // Launch the selected app
                if let Some(child) = selected_row.child() {
                    if let Some(box_widget) = child.downcast_ref::<Box>() {
                        let exec = box_widget.widget_name();
                        if !exec.is_empty() {
                            Self::launch_app(&exec);
                        }
                    }
                }
            }
        });

        // Handle Escape key in search entry to close popover
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
        search_entry.add_controller(escape_controller);
        
        // Show popover on click
        let search_entry_weak = search_entry.downgrade();
        let popover_clone = popover.clone();
        button.connect_clicked(move |_| {
            popover_clone.popup();
            
            // Focus search entry when popover is shown
            if let Some(search) = search_entry_weak.upgrade() {
                search.set_text("");
                // Use idle_add to ensure the popover is fully shown before grabbing focus
                glib::idle_add_local_once(move || {
                    search.grab_focus();
                });
            }
        });
        
        // Also allow fallback to external launcher with right-click
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3); // Right mouse button
        gesture.connect_released(move |_, _, _, _| {
            Self::launch_external_menu();
        });
        button.add_controller(gesture);
        
        Ok(Self { button })
    }
    
    fn load_applications() -> Vec<AppInfo> {
        let mut apps = Vec::new();
        
        let home = std::env::var("HOME").unwrap_or_default();
        let user_apps = format!("{}/.local/share/applications", home);
        let user_flatpak = format!("{}/.local/share/flatpak/exports/share/applications", home);
        
        // Get NixOS profile paths
        let user_profile = format!("{}/.nix-profile/share/applications", home);
        let system_profile = "/run/current-system/sw/share/applications".to_string();
        
        // Standard desktop file locations including NixOS paths
        let desktop_dirs = vec![
            // NixOS paths (check these first)
            &system_profile,
            &user_profile,
            "/nix/var/nix/profiles/default/share/applications",
            
            // User-specific
            &user_apps,
            
            // Traditional Linux paths (might still have some apps)
            "/usr/share/applications",
            "/usr/local/share/applications",
            
            // Flatpak paths
            "/var/lib/flatpak/exports/share/applications",
            &user_flatpak,
        ];
        
        info!("Searching for applications in: {:?}", desktop_dirs);
        
        for dir in desktop_dirs {
            match fs::read_dir(dir) {
                Ok(entries) => {
                    let mut count = 0;
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.ends_with(".desktop") {
                                count += 1;
                                if let Ok(content) = fs::read_to_string(entry.path()) {
                                    if let Some(app) = Self::parse_desktop_file(&content) {
                                        // Skip NoDisplay apps and duplicate entries
                                        if !content.contains("NoDisplay=true") && !content.contains("Hidden=true") {
                                            // Check if we already have this app (by name)
                                            if !apps.iter().any(|a: &AppInfo| a.name == app.name) {
                                                apps.push(app);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if count > 0 {
                        info!("Found {} .desktop files in {}", count, dir);
                    }
                }
                Err(e) => {
                    // Only log if it's not a "not found" error
                    if e.kind() != std::io::ErrorKind::NotFound {
                        warn!("Error reading directory {}: {}", dir, e);
                    }
                }
            }
        }
        
        // Sort by name
        apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        
        info!("Loaded {} applications", apps.len());
        apps
    }
    
    fn parse_desktop_file(content: &str) -> Option<AppInfo> {
        let mut name = None;
        let mut exec = None;
        let mut icon = None;
        let mut comment = None;
        let mut in_desktop_entry = false;
        
        for line in content.lines() {
            let line = line.trim();
            
            if line == "[Desktop Entry]" {
                in_desktop_entry = true;
                continue;
            }
            
            if line.starts_with('[') {
                in_desktop_entry = false;
                continue;
            }
            
            if !in_desktop_entry {
                continue;
            }
            
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "Name" => name = Some(value.to_string()),
                    "Exec" => exec = Some(value.to_string()),
                    "Icon" => icon = Some(value.to_string()),
                    "Comment" => comment = Some(value.to_string()),
                    _ => {}
                }
            }
        }
        
        if let (Some(name), Some(exec)) = (name, exec) {
            Some(AppInfo {
                name,
                exec,
                icon,
                comment,
            })
        } else {
            None
        }
    }
    
    fn populate_app_list(list: &ListBox, apps: &[AppInfo], query: &str) {
        // Clear existing items
        while let Some(child) = list.first_child() {
            list.remove(&child);
        }
        
        let query_lower = query.to_lowercase();
        
        // Filter and score apps
        let mut scored_apps: Vec<(&AppInfo, i32)> = apps
            .iter()
            .filter_map(|app| {
                let name_lower = app.name.to_lowercase();
                let comment_lower = app.comment.as_ref().map(|c| c.to_lowercase()).unwrap_or_default();
                
                if query.is_empty() {
                    Some((app, 0))
                } else {
                    // Simple fuzzy matching
                    let mut score = 0;
                    
                    // Exact match in name
                    if name_lower.contains(&query_lower) {
                        score += 100;
                        // Bonus for start of word
                        if name_lower.starts_with(&query_lower) {
                            score += 50;
                        }
                    }
                    
                    // Match in comment
                    if comment_lower.contains(&query_lower) {
                        score += 30;
                    }
                    
                    // Character-by-character fuzzy match
                    let mut query_chars = query_lower.chars();
                    let mut current_char = query_chars.next();
                    let mut consecutive = 0;
                    
                    for name_char in name_lower.chars() {
                        if let Some(qc) = current_char {
                            if name_char == qc {
                                score += 10 + consecutive * 2;
                                consecutive += 1;
                                current_char = query_chars.next();
                            } else {
                                consecutive = 0;
                            }
                        }
                    }
                    
                    // Only include if all query chars were found
                    if current_char.is_none() && score > 0 {
                        Some((app, score))
                    } else {
                        None
                    }
                }
            })
            .collect();
        
        // Sort by score (highest first)
        scored_apps.sort_by(|a, b| b.1.cmp(&a.1));
        
        // Add apps to list
        for (app, _) in scored_apps.iter().take(20) {
            let row = ListBoxRow::new();
            row.add_css_class("launcher-item");
            
            let hbox = Box::new(Orientation::Horizontal, 10);
            hbox.set_margin_start(10);
            hbox.set_margin_end(10);
            hbox.set_margin_top(8);
            hbox.set_margin_bottom(8);
            
            // Store exec command in widget name for retrieval later
            hbox.set_widget_name(&app.exec);
            
            // App icon
            let icon = Image::new();
            if let Some(icon_name) = &app.icon {
                if icon_name.contains('/') {
                    // It's a path
                    icon.set_from_file(Some(Path::new(icon_name)));
                } else {
                    // It's an icon name
                    icon.set_from_icon_name(Some(icon_name));
                }
            } else {
                icon.set_from_icon_name(Some("application-x-executable"));
            }
            icon.set_pixel_size(32);
            hbox.append(&icon);
            
            // App name and description
            let vbox = Box::new(Orientation::Vertical, 2);
            vbox.set_hexpand(true);
            vbox.set_valign(gtk4::Align::Center);
            
            let name_label = Label::new(Some(&app.name));
            name_label.set_halign(gtk4::Align::Start);
            name_label.add_css_class("launcher-app-name");
            vbox.append(&name_label);
            
            if let Some(comment) = &app.comment {
                let comment_label = Label::new(Some(comment));
                comment_label.set_halign(gtk4::Align::Start);
                comment_label.add_css_class("launcher-app-comment");
                comment_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                vbox.append(&comment_label);
            }
            
            hbox.append(&vbox);
            row.set_child(Some(&hbox));
            list.append(&row);
        }
        
        // Select first item if any
        if let Some(first_row) = list.row_at_index(0) {
            list.select_row(Some(&first_row));
        }
    }
    
    fn launch_app(exec: &str) {
        // Remove field codes like %f, %F, %u, %U, etc.
        let clean_exec = exec
            .split_whitespace()
            .filter(|s| !s.starts_with('%'))
            .collect::<Vec<_>>()
            .join(" ");
        
        // Try to launch the app
        if let Err(e) = Command::new("sh")
            .arg("-c")
            .arg(&clean_exec)
            .spawn()
        {
            warn!("Failed to launch app '{}': {}", clean_exec, e);
        }
    }
    
    fn launch_external_menu() {
        // Try to launch common application menus
        let launchers = vec![
            ("fuzzel", vec![]),
            ("wofi", vec!["--show", "drun"]),
            ("rofi", vec!["-show", "drun"]),
            ("dmenu_run", vec![]),
        ];
        
        for (cmd, args) in launchers {
            match Command::new(cmd).args(&args).spawn() {
                Ok(_) => {
                    return;
                }
                Err(_) => continue,
            }
        }
        
        warn!("No external application launcher found. Install fuzzel, wofi, rofi, or dmenu.");
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}