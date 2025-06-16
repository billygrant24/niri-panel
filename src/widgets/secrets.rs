// src/widgets/secrets.rs
use anyhow::Result;
use glib::timeout_add_local;
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Entry, Image, Label, ListBox, ListBoxRow, Orientation, Popover,
    ScrolledWindow, SearchEntry, Separator, Stack, StackSwitcher,
};
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::time::Duration;
use tracing::{error, info, warn};

pub struct Secrets {
    button: Button,
}

#[derive(Debug, Clone)]
struct SecretEntry {
    name: String,
    path: String,
    is_otp: bool,
    category: String,
}

#[derive(Debug, Clone)]
struct SecretInfo {
    name: String,
    username: Option<String>,
    password: Option<String>,
    otp_code: Option<String>,
    otp_remaining: Option<u32>,
    url: Option<String>,
    notes: Option<String>,
}

impl Secrets {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("secrets");

        // Use lock icon
        let icon_names = vec![
            "dialog-password-symbolic",
            "security-high-symbolic",
            "channel-secure-symbolic",
            "lock-symbolic",
        ];

        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                info!("Secrets button icon found: {}", &icon_name);
                break;
            }
        }

        if image.icon_name().is_none() {
            let label = Label::new(Some("🔐"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }

        // Create popover
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("secrets-popover");
        popover.set_autohide(true);

        // Handle popover show/hide for keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Secrets popover shown - keyboard mode set to OnDemand");
            }
        });

        let window_weak_hide = window_weak.clone();
        let active_popovers_hide = active_popovers.clone();
        popover.connect_hide(move |_| {
            *active_popovers_hide.borrow_mut() -= 1;
            if *active_popovers_hide.borrow() == 0 {
                if let Some(window) = window_weak_hide.upgrade() {
                    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
                    info!("Secrets popover hidden - keyboard mode set to None");
                }
            }
        });

        let main_box = Box::new(Orientation::Vertical, 10);
        main_box.set_margin_top(15);
        main_box.set_margin_bottom(15);
        main_box.set_margin_start(15);
        main_box.set_margin_end(15);
        main_box.set_size_request(400, 500);

        // Check if pass is available
        if !Self::check_pass_available() {
            let error_label = Label::new(Some("Password store not found!\n\nPlease install 'pass' and initialize it with:\npass init <gpg-key-id>"));
            error_label.add_css_class("error-label");
            error_label.set_wrap(true);
            main_box.append(&error_label);

            popover.set_child(Some(&main_box));

            button.connect_clicked(move |_| {
                popover.popup();
            });

            return Ok(Self { button });
        }

        // Search entry
        let search_entry = SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search passwords..."));
        search_entry.add_css_class("secrets-search");
        search_entry.set_hexpand(true);
        main_box.append(&search_entry);

        // Stack for tabs
        let stack = Stack::new();
        stack.set_transition_type(gtk4::StackTransitionType::SlideLeftRight);

        // Passwords tab
        let passwords_box = Box::new(Orientation::Vertical, 5);
        let passwords_scroll = ScrolledWindow::new();
        passwords_scroll.set_vexpand(true);
        passwords_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        let passwords_list = ListBox::new();
        passwords_list.add_css_class("secrets-list");
        passwords_list.set_selection_mode(gtk4::SelectionMode::None);
        passwords_scroll.set_child(Some(&passwords_list));
        passwords_box.append(&passwords_scroll);

        stack.add_titled(&passwords_box, Some("passwords"), "Passwords");

        // OTP tab
        let otp_box = Box::new(Orientation::Vertical, 5);
        let otp_scroll = ScrolledWindow::new();
        otp_scroll.set_vexpand(true);
        otp_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        let otp_list = ListBox::new();
        otp_list.add_css_class("secrets-list");
        otp_list.set_selection_mode(gtk4::SelectionMode::None);
        otp_scroll.set_child(Some(&otp_list));
        otp_box.append(&otp_scroll);

        stack.add_titled(&otp_box, Some("otp"), "2FA Codes");

        // Stack switcher
        let switcher = StackSwitcher::new();
        switcher.set_stack(Some(&stack));
        switcher.set_halign(gtk4::Align::Center);
        main_box.append(&switcher);

        main_box.append(&stack);

        // Actions bar
        let actions_bar = Box::new(Orientation::Horizontal, 10);
        actions_bar.set_halign(gtk4::Align::Center);
        actions_bar.set_margin_top(10);

        let new_button = Button::with_label("New Entry");
        new_button.add_css_class("suggested-action");
        new_button.connect_clicked(|_| {
            Self::open_pass_editor();
        });
        actions_bar.append(&new_button);

        let sync_button = Button::with_label("Sync");
        sync_button.connect_clicked(|_| {
            Self::sync_password_store();
        });
        actions_bar.append(&sync_button);

        main_box.append(&actions_bar);

        popover.set_child(Some(&main_box));

        // Load initial entries
        let all_entries = Self::load_password_entries();
        Self::populate_lists(&passwords_list, &otp_list, &all_entries, "");

        // Handle search
        let passwords_list_weak = passwords_list.downgrade();
        let otp_list_weak = otp_list.downgrade();
        let entries_for_search = all_entries.clone();
        search_entry.connect_search_changed(move |entry| {
            let query = entry.text();
            if let (Some(pwd_list), Some(otp_list)) =
                (passwords_list_weak.upgrade(), otp_list_weak.upgrade())
            {
                Self::populate_lists(&pwd_list, &otp_list, &entries_for_search, &query);
            }
        });

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
        search_entry.add_controller(escape_controller);

        // Show popover on click
        let search_entry_weak = search_entry.downgrade();
        button.connect_clicked(move |_| {
            popover.popup();
            if let Some(search) = search_entry_weak.upgrade() {
                search.grab_focus();
            }
        });

        Ok(Self { button })
    }

    fn check_pass_available() -> bool {
        Command::new("which")
            .arg("pass")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn get_password_store_dir() -> PathBuf {
        if let Ok(store_dir) = std::env::var("PASSWORD_STORE_DIR") {
            PathBuf::from(store_dir)
        } else if let Some(home) = dirs::home_dir() {
            home.join(".password-store")
        } else {
            PathBuf::from(".password-store")
        }
    }

    fn load_password_entries() -> Vec<SecretEntry> {
        let mut entries = Vec::new();
        let store_dir = Self::get_password_store_dir();

        if !store_dir.exists() {
            warn!("Password store directory not found: {:?}", store_dir);
            return entries;
        }

        Self::scan_directory(&store_dir, &store_dir, &mut entries);

        // Sort entries by name
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        info!("Loaded {} password entries", entries.len());
        entries
    }

    fn scan_directory(dir: &Path, base_dir: &Path, entries: &mut Vec<SecretEntry>) {
        if let Ok(read_dir) = fs::read_dir(dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();

                // Skip hidden files and .git directory
                if file_name_str.starts_with('.') {
                    continue;
                }

                if path.is_dir() {
                    Self::scan_directory(&path, base_dir, entries);
                } else if path.extension().and_then(|s| s.to_str()) == Some("gpg") {
                    // Get relative path from base directory
                    if let Ok(relative_path) = path.strip_prefix(base_dir) {
                        let entry_path = relative_path
                            .to_string_lossy()
                            .trim_end_matches(".gpg")
                            .to_string();

                        // Determine category from path
                        let parts: Vec<&str> = entry_path.split('/').collect();
                        let (category, name) = if parts.len() > 1 {
                            (parts[0].to_string(), parts[parts.len() - 1].to_string())
                        } else {
                            ("General".to_string(), parts[0].to_string())
                        };

                        // Check if it's an OTP entry
                        let is_otp = entry_path.contains("otp") || entry_path.contains("2fa");

                        entries.push(SecretEntry {
                            name,
                            path: entry_path,
                            is_otp,
                            category,
                        });
                    }
                }
            }
        }
    }

    fn populate_lists(
        passwords_list: &ListBox,
        otp_list: &ListBox,
        entries: &[SecretEntry],
        query: &str,
    ) {
        // Clear existing items
        while let Some(child) = passwords_list.first_child() {
            passwords_list.remove(&child);
        }
        while let Some(child) = otp_list.first_child() {
            otp_list.remove(&child);
        }

        let query_lower = query.to_lowercase();

        // Group entries by category
        let mut password_categories: HashMap<String, Vec<&SecretEntry>> = HashMap::new();
        let mut otp_entries: Vec<&SecretEntry> = Vec::new();

        for entry in entries {
            // Filter by search query
            if !query.is_empty()
                && !entry.name.to_lowercase().contains(&query_lower)
                && !entry.path.to_lowercase().contains(&query_lower)
            {
                continue;
            }

            if entry.is_otp {
                otp_entries.push(entry);
            } else {
                password_categories
                    .entry(entry.category.clone())
                    .or_insert_with(Vec::new)
                    .push(entry);
            }
        }

        // Populate password list with categories
        let mut categories: Vec<_> = password_categories.keys().cloned().collect();
        categories.sort();

        for category in categories {
            if let Some(category_entries) = password_categories.get(&category) {
                // Add category header
                let header_row = ListBoxRow::new();
                header_row.set_selectable(false);
                let header_label = Label::new(Some(&category));
                header_label.add_css_class("secrets-category-header");
                header_label.set_halign(gtk4::Align::Start);
                header_label.set_margin_start(10);
                header_label.set_margin_top(10);
                header_label.set_margin_bottom(5);
                header_row.set_child(Some(&header_label));
                passwords_list.append(&header_row);

                // Add entries in this category
                for entry in category_entries {
                    let row = Self::create_secret_row(entry, false);
                    passwords_list.append(&row);
                }
            }
        }

        // Populate OTP list
        for entry in &otp_entries {
            let row = Self::create_secret_row(entry, true);
            otp_list.append(&row);
        }

        // Show empty state if needed
        if password_categories.is_empty() {
            let empty_row = ListBoxRow::new();
            let empty_label = Label::new(Some("No passwords found"));
            empty_label.add_css_class("dim-label");
            empty_label.set_margin_top(50);
            empty_label.set_margin_bottom(50);
            empty_row.set_child(Some(&empty_label));
            passwords_list.append(&empty_row);
        }

        if otp_entries.is_empty() {
            let empty_row = ListBoxRow::new();
            let empty_label = Label::new(Some("No OTP entries found"));
            empty_label.add_css_class("dim-label");
            empty_label.set_margin_top(50);
            empty_label.set_margin_bottom(50);
            empty_row.set_child(Some(&empty_label));
            otp_list.append(&empty_row);
        }
    }

    fn create_secret_row(entry: &SecretEntry, is_otp: bool) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("secret-row");

        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(10);
        hbox.set_margin_end(10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);

        // Icon
        let icon_name = if is_otp {
            "security-high-symbolic"
        } else {
            "dialog-password-symbolic"
        };
        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(24);
        hbox.append(&icon);

        // Name
        let name_label = Label::new(Some(&entry.name));
        name_label.set_halign(gtk4::Align::Start);
        name_label.set_hexpand(true);
        name_label.add_css_class("secret-name");
        hbox.append(&name_label);

        // Action buttons
        if is_otp {
            // OTP code display
            let otp_label = Label::new(Some("------"));
            otp_label.add_css_class("otp-code");
            otp_label.set_width_chars(6);
            hbox.append(&otp_label);

            // Timer
            let timer_label = Label::new(Some(""));
            timer_label.add_css_class("otp-timer");
            timer_label.set_width_chars(3);
            hbox.append(&timer_label);

            // Refresh button
            let refresh_button = Button::from_icon_name("view-refresh-symbolic");
            refresh_button.add_css_class("secret-action-button");
            refresh_button.set_tooltip_text(Some("Generate OTP code"));

            let entry_path = entry.path.clone();
            let otp_label_weak = otp_label.downgrade();
            let timer_label_weak = timer_label.downgrade();
            refresh_button.connect_clicked(move |button| {
                Self::generate_otp(
                    &entry_path,
                    button,
                    otp_label_weak.clone(),
                    timer_label_weak.clone(),
                );
            });
            hbox.append(&refresh_button);
        } else {
            // Copy password button
            let copy_button = Button::from_icon_name("edit-copy-symbolic");
            copy_button.add_css_class("secret-action-button");
            copy_button.set_tooltip_text(Some("Copy password"));

            let entry_path = entry.path.clone();
            copy_button.connect_clicked(move |button| {
                Self::copy_password(&entry_path, button);
            });
            hbox.append(&copy_button);

            // Show details button
            let show_button = Button::from_icon_name("document-properties-symbolic");
            show_button.add_css_class("secret-action-button");
            show_button.set_tooltip_text(Some("Show details"));

            let entry_path = entry.path.clone();
            let entry_name = entry.name.clone();
            show_button.connect_clicked(move |_| {
                Self::show_secret_details(&entry_path, &entry_name);
            });
            hbox.append(&show_button);
        }

        row.set_child(Some(&hbox));
        row
    }

    fn copy_password(path: &str, button: &Button) {
        match Command::new("pass").arg("-c").arg(path).output() {
            Ok(output) => {
                if output.status.success() {
                    // Visual feedback
                    button.add_css_class("success");
                    let button_weak = button.downgrade();
                    timeout_add_local(Duration::from_millis(1000), move || {
                        if let Some(button) = button_weak.upgrade() {
                            button.remove_css_class("success");
                        }
                        glib::ControlFlow::Break
                    });

                    // Clear clipboard after 45 seconds
                    timeout_add_local(Duration::from_secs(45), move || {
                        let _ = Command::new("wl-copy").arg("--clear").spawn();
                        glib::ControlFlow::Break
                    });
                } else {
                    error!(
                        "Failed to copy password: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                error!("Failed to execute pass: {}", e);
            }
        }
    }

    fn generate_otp(
        path: &str,
        button: &Button,
        otp_label_weak: WeakRef<Label>,
        timer_label_weak: WeakRef<Label>,
    ) {
        match Command::new("pass").arg("otp").arg(path).output() {
            Ok(output) => {
                if output.status.success() {
                    let otp_code = String::from_utf8_lossy(&output.stdout).trim().to_string();

                    if let Some(otp_label) = otp_label_weak.upgrade() {
                        otp_label.set_text(&otp_code);

                        // Copy to clipboard using wl-copy
                        let _ = Command::new("wl-copy")
                            .stdin(Stdio::piped())
                            .spawn()
                            .and_then(|mut child| {
                                if let Some(stdin) = child.stdin.as_mut() {
                                    stdin.write_all(otp_code.as_bytes()).ok();
                                }
                                child.wait()
                            });

                        // Visual feedback
                        button.add_css_class("success");
                        let button_weak = button.downgrade();
                        timeout_add_local(Duration::from_millis(1000), move || {
                            if let Some(button) = button_weak.upgrade() {
                                button.remove_css_class("success");
                            }
                            glib::ControlFlow::Break
                        });

                        // Start countdown timer (30 seconds for TOTP)
                        let mut remaining = 30;
                        timeout_add_local(Duration::from_secs(1), move || {
                            remaining -= 1;

                            if let (Some(timer), Some(otp)) =
                                (timer_label_weak.upgrade(), otp_label_weak.upgrade())
                            {
                                if remaining > 0 {
                                    timer.set_text(&format!("{}s", remaining));
                                    glib::ControlFlow::Continue
                                } else {
                                    timer.set_text("");
                                    otp.set_text("------");
                                    glib::ControlFlow::Break
                                }
                            } else {
                                glib::ControlFlow::Break
                            }
                        });
                    }
                } else {
                    error!(
                        "Failed to generate OTP: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                error!("Failed to execute pass otp: {}", e);
            }
        }
    }

    fn show_secret_details(path: &str, name: &str) {
        // Get full password entry
        match Command::new("pass").arg(path).output() {
            Ok(output) => {
                if output.status.success() {
                    let content = String::from_utf8_lossy(&output.stdout);

                    // Parse the content
                    let lines: Vec<&str> = content.lines().collect();
                    let password = lines.get(0).map(|s| s.to_string());

                    let mut info = SecretInfo {
                        name: name.to_string(),
                        password,
                        username: None,
                        otp_code: None,
                        otp_remaining: None,
                        url: None,
                        notes: None,
                    };

                    // Parse additional fields
                    let mut notes = Vec::new();
                    for line in lines.iter().skip(1) {
                        if line.starts_with("Username:") || line.starts_with("username:") {
                            info.username =
                                Some(line.split(':').nth(1).unwrap_or("").trim().to_string());
                        } else if line.starts_with("URL:") || line.starts_with("url:") {
                            info.url =
                                Some(line.split(':').nth(1).unwrap_or("").trim().to_string());
                        } else if !line.trim().is_empty() {
                            notes.push(line.to_string());
                        }
                    }

                    if !notes.is_empty() {
                        info.notes = Some(notes.join("\n"));
                    }

                    Self::show_details_dialog(info);
                }
            }
            Err(e) => {
                error!("Failed to get password details: {}", e);
            }
        }
    }

    fn show_details_dialog(info: SecretInfo) {
        let dialog = gtk4::Window::new();
        dialog.set_title(Some(&format!("Password: {}", info.name)));
        dialog.set_modal(true);
        dialog.set_resizable(false);
        dialog.set_default_size(400, 300);

        let vbox = Box::new(Orientation::Vertical, 10);
        vbox.set_margin_top(20);
        vbox.set_margin_bottom(20);
        vbox.set_margin_start(20);
        vbox.set_margin_end(20);

        // Password field
        if let Some(password) = &info.password {
            let pwd_box = Box::new(Orientation::Horizontal, 10);
            let pwd_label = Label::new(Some("Password:"));
            pwd_label.set_width_chars(10);
            pwd_label.set_halign(gtk4::Align::Start);
            pwd_box.append(&pwd_label);

            let pwd_entry = Entry::new();
            pwd_entry.set_text(password);
            pwd_entry.set_visibility(false);
            pwd_entry.set_hexpand(true);
            pwd_box.append(&pwd_entry);

            let show_button = Button::from_icon_name("view-reveal-symbolic");
            let is_visible = Rc::new(RefCell::new(false));
            let is_visible_clone = is_visible.clone();
            show_button.connect_clicked(move |button| {
                let mut visible = is_visible_clone.borrow_mut();
                *visible = !*visible;
                pwd_entry.set_visibility(*visible);

                // Update button icon
                if *visible {
                    button.set_icon_name("view-conceal-symbolic");
                } else {
                    button.set_icon_name("view-reveal-symbolic");
                }
            });
            pwd_box.append(&show_button);

            vbox.append(&pwd_box);
        }

        // Username field
        if let Some(username) = &info.username {
            let user_box = Box::new(Orientation::Horizontal, 10);
            let user_label = Label::new(Some("Username:"));
            user_label.set_width_chars(10);
            user_label.set_halign(gtk4::Align::Start);
            user_box.append(&user_label);

            let user_entry = Entry::new();
            user_entry.set_text(username);
            user_entry.set_hexpand(true);
            user_box.append(&user_entry);

            vbox.append(&user_box);
        }

        // URL field
        if let Some(url) = &info.url {
            let url_box = Box::new(Orientation::Horizontal, 10);
            let url_label = Label::new(Some("URL:"));
            url_label.set_width_chars(10);
            url_label.set_halign(gtk4::Align::Start);
            url_box.append(&url_label);

            let url_entry = Entry::new();
            url_entry.set_text(url);
            url_entry.set_hexpand(true);
            url_box.append(&url_entry);

            vbox.append(&url_box);
        }

        // Notes
        if let Some(notes) = &info.notes {
            let separator = Separator::new(Orientation::Horizontal);
            vbox.append(&separator);

            let notes_label = Label::new(Some("Notes:"));
            notes_label.set_halign(gtk4::Align::Start);
            vbox.append(&notes_label);

            let notes_text = Label::new(Some(notes));
            notes_text.set_wrap(true);
            notes_text.set_halign(gtk4::Align::Start);
            notes_text.add_css_class("dim-label");
            vbox.append(&notes_text);
        }

        // Close button
        let close_button = Button::with_label("Close");
        close_button.set_halign(gtk4::Align::End);
        close_button.set_margin_top(20);

        let dialog_weak = dialog.downgrade();
        close_button.connect_clicked(move |_| {
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });
        vbox.append(&close_button);

        dialog.set_child(Some(&vbox));
        dialog.present();
    }

    fn open_pass_editor() {
        // Try to open qtpass or similar GUI
        let editors = vec![
            ("qtpass", vec![]),
            ("keepassxc", vec![]),
            ("pass", vec!["insert"]),
        ];

        for (cmd, args) in editors {
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }

        warn!("No password editor found");
    }

    fn sync_password_store() {
        // Run pass git pull and push
        let store_dir = Self::get_password_store_dir();

        // Pull
        match Command::new("pass").args(&["git", "pull"]).output() {
            Ok(output) => {
                if output.status.success() {
                    info!("Password store synced successfully");

                    // Push
                    let _ = Command::new("pass").args(&["git", "push"]).spawn();
                } else {
                    warn!(
                        "Failed to sync: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            }
            Err(e) => {
                warn!("Failed to run git sync: {}", e);
            }
        }
    }

    pub fn widget(&self) -> &Button {
        &self.button
    }
}

// Add to your style.css:
/*
/* Secrets widget */
.secrets {
    background: none;
    border: none;
    padding: 0 16px;
    margin: 0;
    border-radius: 0;
    min-height: 32px;
    transition: all 0.2s ease;
}

.secrets:hover {
    background-color: rgba(255, 255, 255, 0.05);
}

.secrets:active {
    background-color: rgba(255, 255, 255, 0.1);
}

/* Secrets popover */
.secrets-popover {
    background-color: #383c4a;
    border-radius: 8px;
    border: 1px solid #20232b;
    box-shadow: 0 8px 16px rgba(0, 0, 0, 0.4);
}

.secrets-search {
    background-color: #2b2e3b;
    border: 1px solid #20232b;
    border-radius: 6px;
    color: #d3dae3;
    font-size: 14px;
    padding: 8px 12px;
    margin-bottom: 10px;
}

.secrets-search:focus {
    border-color: #5294e2;
    outline: none;
}

.secrets-list {
    background-color: transparent;
}

.secret-row {
    background-color: transparent;
    border-radius: 6px;
    transition: all 0.2s ease;
}

.secret-row:hover {
    background-color: rgba(255, 255, 255, 0.05);
}

.secrets-category-header {
    font-weight: 600;
    font-size: 12px;
    opacity: 0.7;
    text-transform: uppercase;
}

.secret-name {
    font-size: 13px;
    color: #d3dae3;
}

.secret-action-button {
    background: none;
    border: none;
    padding: 4px;
    border-radius: 4px;
    min-width: 24px;
    min-height: 24px;
    transition: all 0.2s ease;
    opacity: 0.7;
}

.secret-action-button:hover {
    background-color: rgba(255, 255, 255, 0.1);
    opacity: 1;
}

.secret-action-button.success {
    background-color: rgba(82, 148, 226, 0.3);
}

.otp-code {
    font-family: monospace;
    font-size: 16px;
    font-weight: bold;
    color: #5294e2;
}

.otp-timer {
    font-size: 11px;
    color: #7c818c;
    font-family: monospace;
}

.error-label {
    color: #f27835;
    font-size: 14px;
    text-align: center;
    margin: 40px;
}
*/
