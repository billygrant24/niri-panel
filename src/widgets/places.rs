use gtk4::prelude::*;
use gtk4::{Button, Image, Popover, Box, Orientation, Label, Separator, Notebook, ApplicationWindow};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use anyhow::Result;
use std::process::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{warn, info};
use std::rc::Rc;
use std::cell::RefCell;

pub struct Places {
    button: Button,
}

#[derive(Debug, Clone)]
struct PlaceInfo {
    name: String,
    path: PathBuf,
    icon: String,
    #[allow(dead_code)]
    is_bookmark: bool,
}

impl Places {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("places");
        
        // Use folder icon with fallbacks
        let icon_names = vec![
            "folder-symbolic",
            "folder",
            "inode-directory-symbolic",
            "gtk-directory"
        ];
        
        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                break;
            }
        }
        
        if image.icon_name().is_none() {
            let label = Label::new(Some("📁"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }
        
        // Create popover for places menu
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("places-popover");
        popover.set_has_arrow(false);
        
        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Places popover shown - keyboard mode set to OnDemand (active popovers: {})", 
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
                    info!("Places popover hidden - keyboard mode set to None");
                }
            } else {
                info!("Places popover hidden - keeping keyboard mode (active popovers: {})", count);
            }
        });
        
        // Create notebook for tabs
        let notebook = Notebook::new();
        notebook.set_margin_top(5);
        notebook.set_margin_bottom(5);
        notebook.set_size_request(250, -1);
        
        // Create Places tab (current view)
        let places_content = Self::create_places_tab();
        let places_label = Label::new(Some("Places"));
        notebook.append_page(&places_content, Some(&places_label));
        
        // Create Sources tab
        let sources_content = Self::create_sources_tab();
        let sources_label = Label::new(Some("Sources"));
        notebook.append_page(&sources_content, Some(&sources_label));
        
        // Create Servers tab
        let servers_content = Self::create_servers_tab();
        let servers_label = Label::new(Some("Servers"));
        notebook.append_page(&servers_content, Some(&servers_label));
        
        // Set the first tab as default
        notebook.set_current_page(Some(0));
        
        popover.set_child(Some(&notebook));
        
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
        let popover_clone = popover.clone();
        button.connect_clicked(move |_| {
            popover_clone.popup();
        });
        
        Ok(Self { button })
    }
    
    fn create_places_tab() -> gtk4::ScrolledWindow {
        let scrolled_window = gtk4::ScrolledWindow::new();
        scrolled_window.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled_window.set_max_content_height(500);
        scrolled_window.set_propagate_natural_height(true);
        
        let list_box = Box::new(Orientation::Vertical, 0);
        
        // Add home directory
        let home_button = Self::create_place_button(&PlaceInfo {
            name: "Home".to_string(),
            path: dirs::home_dir().unwrap_or_default(),
            icon: "user-home-symbolic".to_string(),
            is_bookmark: false,
        });
        list_box.append(&home_button);
        
        // Add XDG directories
        let xdg_dirs = Self::get_xdg_directories();
        for place in xdg_dirs {
            let button = Self::create_place_button(&place);
            list_box.append(&button);
        }
        
        // Separator
        let separator1 = Separator::new(Orientation::Horizontal);
        separator1.set_margin_top(5);
        separator1.set_margin_bottom(5);
        list_box.append(&separator1);
        
        // Add computer/filesystem
        let computer_button = Self::create_place_button(&PlaceInfo {
            name: "Computer".to_string(),
            path: PathBuf::from("/"),
            icon: "computer-symbolic".to_string(),
            is_bookmark: false,
        });
        list_box.append(&computer_button);
        
        // Add common mount points if they exist
        let mount_points = vec![
            ("/media", "drive-removable-media-symbolic"),
            ("/mnt", "drive-harddisk-symbolic"),
        ];
        
        for (path, icon) in mount_points {
            let path_buf = PathBuf::from(path);
            if path_buf.exists() && path_buf.is_dir() {
                let button = Self::create_place_button(&PlaceInfo {
                    name: path.trim_start_matches('/').to_string(),
                    path: path_buf,
                    icon: icon.to_string(),
                    is_bookmark: false,
                });
                list_box.append(&button);
            }
        }
        
        // Add bookmarks if any
        let bookmarks = Self::get_gtk_bookmarks();
        if !bookmarks.is_empty() {
            let separator2 = Separator::new(Orientation::Horizontal);
            separator2.set_margin_top(5);
            separator2.set_margin_bottom(5);
            list_box.append(&separator2);
            
            let bookmarks_label = Label::new(Some("Bookmarks"));
            bookmarks_label.set_halign(gtk4::Align::Start);
            bookmarks_label.add_css_class("places-section-label");
            bookmarks_label.set_margin_start(10);
            bookmarks_label.set_margin_top(5);
            bookmarks_label.set_margin_bottom(5);
            list_box.append(&bookmarks_label);
            
            for bookmark in bookmarks {
                let button = Self::create_place_button(&bookmark);
                list_box.append(&button);
            }
        }
        
        // Add recent files button
        let separator3 = Separator::new(Orientation::Horizontal);
        separator3.set_margin_top(5);
        separator3.set_margin_bottom(5);
        list_box.append(&separator3);
        
        let recent_button = Button::new();
        recent_button.add_css_class("places-item");
        let recent_box = Box::new(Orientation::Horizontal, 10);
        recent_box.set_margin_start(10);
        recent_box.set_margin_end(10);
        recent_box.set_margin_top(8);
        recent_box.set_margin_bottom(8);
        
        let recent_icon = Image::from_icon_name("document-open-recent-symbolic");
        recent_icon.set_pixel_size(16);
        recent_box.append(&recent_icon);
        
        let recent_label = Label::new(Some("Recent"));
        recent_label.set_hexpand(true);
        recent_label.set_halign(gtk4::Align::Start);
        recent_box.append(&recent_label);
        
        recent_button.set_child(Some(&recent_box));
        recent_button.connect_clicked(move |_| {
            // Open recent files - this would typically open a file manager with recent view
            Self::open_recent();
        });
        list_box.append(&recent_button);
        
        scrolled_window.set_child(Some(&list_box));
        scrolled_window
    }
    
    fn create_sources_tab() -> gtk4::ScrolledWindow {
        let scrolled_window = gtk4::ScrolledWindow::new();
        scrolled_window.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled_window.set_max_content_height(500);
        scrolled_window.set_propagate_natural_height(true);
        
        let content_box = Box::new(Orientation::Vertical, 10);
        content_box.set_margin_top(20);
        content_box.set_margin_bottom(20);
        content_box.set_margin_start(20);
        content_box.set_margin_end(20);
        
        // Placeholder content for Sources tab
        let placeholder_label = Label::new(Some("Sources content goes here"));
        placeholder_label.add_css_class("dim-label");
        content_box.append(&placeholder_label);
        
        scrolled_window.set_child(Some(&content_box));
        scrolled_window
    }
    
    fn create_servers_tab() -> gtk4::ScrolledWindow {
        let scrolled_window = gtk4::ScrolledWindow::new();
        scrolled_window.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        scrolled_window.set_max_content_height(500);
        scrolled_window.set_propagate_natural_height(true);
        
        let content_box = Box::new(Orientation::Vertical, 10);
        content_box.set_margin_top(20);
        content_box.set_margin_bottom(20);
        content_box.set_margin_start(20);
        content_box.set_margin_end(20);
        
        // Placeholder content for Servers tab
        let placeholder_label = Label::new(Some("Servers content goes here"));
        placeholder_label.add_css_class("dim-label");
        content_box.append(&placeholder_label);
        
        scrolled_window.set_child(Some(&content_box));
        scrolled_window
    }
    
    fn get_xdg_directories() -> Vec<PlaceInfo> {
        let mut dirs = Vec::new();
        
        // Define XDG directories with their icons
        let xdg_mappings = vec![
            (dirs::desktop_dir(), "Desktop", "user-desktop-symbolic"),
            (dirs::document_dir(), "Documents", "folder-documents-symbolic"),
            (dirs::download_dir(), "Downloads", "folder-download-symbolic"),
            (dirs::audio_dir(), "Music", "folder-music-symbolic"),
            (dirs::picture_dir(), "Pictures", "folder-pictures-symbolic"),
            (dirs::public_dir(), "Public", "folder-publicshare-symbolic"),
            (dirs::template_dir(), "Templates", "folder-templates-symbolic"),
            (dirs::video_dir(), "Videos", "folder-videos-symbolic"),
        ];
        
        for (dir_opt, name, icon) in xdg_mappings {
            if let Some(dir) = dir_opt {
                if dir.exists() {
                    dirs.push(PlaceInfo {
                        name: name.to_string(),
                        path: dir,
                        icon: icon.to_string(),
                        is_bookmark: false,
                    });
                }
            }
        }
        
        dirs
    }
    
    fn get_gtk_bookmarks() -> Vec<PlaceInfo> {
        let mut bookmarks = Vec::new();
        
        // GTK3/4 bookmarks location
        if let Some(config_dir) = dirs::config_dir() {
            let bookmarks_path = config_dir.join("gtk-3.0").join("bookmarks");
            
            if bookmarks_path.exists() {
                if let Ok(content) = fs::read_to_string(&bookmarks_path) {
                    for line in content.lines() {
                        if line.trim().is_empty() {
                            continue;
                        }
                        
                        // Parse bookmark line (format: "file:///path Name" or just "file:///path")
                        let parts: Vec<&str> = line.splitn(2, ' ').collect();
                        if let Some(uri) = parts.get(0) {
                            if let Some(path_str) = uri.strip_prefix("file://") {
                                let path = PathBuf::from(path_str);
                                if path.exists() {
                                    let name = if let Some(custom_name) = parts.get(1) {
                                        custom_name.to_string()
                                    } else {
                                        path.file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or("Bookmark")
                                            .to_string()
                                    };
                                    
                                    bookmarks.push(PlaceInfo {
                                        name,
                                        path,
                                        icon: "folder-symbolic".to_string(),
                                        is_bookmark: true,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
        
        bookmarks
    }
    
    fn create_place_button(place: &PlaceInfo) -> Button {
        let button = Button::new();
        button.add_css_class("places-item");
        
        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(10);
        hbox.set_margin_end(10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);
        
        let icon = Image::from_icon_name(&place.icon);
        icon.set_pixel_size(16);
        hbox.append(&icon);
        
        let label = Label::new(Some(&place.name));
        label.set_hexpand(true);
        label.set_halign(gtk4::Align::Start);
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        hbox.append(&label);
        
        button.set_child(Some(&hbox));
        
        let path = place.path.clone();
        button.connect_clicked(move |_| {
            Self::open_location(&path);
        });
        
        button
    }
    
    fn open_location(path: &Path) {
        // Try to open with default file manager
        let file_managers = vec![
            ("nautilus", vec![]),
            ("nemo", vec![]),
            ("caja", vec![]),
            ("thunar", vec![]),
            ("pcmanfm", vec![]),
            ("dolphin", vec![]),
        ];
        
        let path_str = path.to_string_lossy();
        
        for (cmd, mut args) in file_managers {
            args.push(path_str.as_ref());
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }
        
        warn!("Could not find file manager to open: {}", path_str);
    }
    
    fn open_recent() {
        // Try to open recent files
        // Most file managers support a recent:// URI
        let commands = vec![
            ("xdg-open", vec!["recent://"]),
            ("nautilus", vec!["recent://"]),
            ("nemo", vec!["recent://"]),
            ("caja", vec!["recent://"]),
        ];
        
        for (cmd, args) in commands {
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }
        
        // Fallback to opening home directory
        if let Some(home) = dirs::home_dir() {
            Self::open_location(&home);
        }
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}