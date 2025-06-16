use anyhow::Result;
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box, Button, Entry, Image, Label, ListBox, ListBoxRow, Orientation, Popover,
    ScrolledWindow,
};
use gtk4_layer_shell::LayerShell;
use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::info;

pub struct Search {
    button: Button,
}

#[derive(Debug, Clone)]
struct SearchResult {
    path: PathBuf,
    name: String,
    parent: String,
    score: i32,
}

#[derive(Debug, Clone)]
struct SearchConfig {
    inclusions: Vec<PathBuf>,
    exclusions: Vec<String>,
    file_type: FileType,
    case_sensitive: bool,
    search_hidden: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum FileType {
    AllFiles,
    Documents,
    Images,
    Audio,
    Video,
    Folders,
}

impl FileType {
    fn extensions(&self) -> Vec<&'static str> {
        match self {
            FileType::AllFiles => vec![],
            FileType::Documents => vec![
                "txt", "md", "pdf", "doc", "docx", "odt", "rtf", "tex", "xls", "xlsx", "ods",
                "ppt", "pptx", "odp", "csv", "json", "xml", "html", "htm", "rst", "org",
            ],
            FileType::Images => vec![
                "jpg", "jpeg", "png", "gif", "bmp", "svg", "tiff", "tif", "webp", "heic", "heif",
                "ico", "xcf", "kra",
            ],
            FileType::Audio => vec![
                "mp3", "wav", "ogg", "flac", "m4a", "aac", "wma", "opus", "alac", "aiff", "midi",
                "mid",
            ],
            FileType::Video => vec![
                "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "3gp",
                "ogv", "vob", "ts",
            ],
            FileType::Folders => vec![],
        }
    }

    fn name(&self) -> &'static str {
        match self {
            FileType::AllFiles => "All Files",
            FileType::Documents => "Documents",
            FileType::Images => "Images",
            FileType::Audio => "Audio",
            FileType::Video => "Video",
            FileType::Folders => "Folders",
        }
    }

    fn icon_name(&self) -> &'static str {
        match self {
            FileType::AllFiles => "text-x-generic-symbolic",
            FileType::Documents => "x-office-document-symbolic",
            FileType::Images => "image-x-generic-symbolic",
            FileType::Audio => "audio-x-generic-symbolic",
            FileType::Video => "video-x-generic-symbolic",
            FileType::Folders => "folder-symbolic",
        }
    }
}

impl Default for SearchConfig {
    fn default() -> Self {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));

        Self {
            inclusions: vec![
                home.join("Documents"),
                home.join("Downloads"),
                home.join("Pictures"),
                home.join("Music"),
                home.join("Videos"),
                home.join("Projects"),
                home.join("Desktop"),
                home.clone(), // Include home but with exclusions
            ],
            exclusions: vec![
                "node_modules".to_string(),
                "vendor".to_string(),
                "target".to_string(),
                "build".to_string(),
                "dist".to_string(),
                ".git".to_string(),
                ".cache".to_string(),
                ".local/share/Trash".to_string(),
            ],
            file_type: FileType::AllFiles,
            case_sensitive: false,
            search_hidden: false,
        }
    }
}

impl Search {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("search");

        // Try multiple search icon fallbacks
        let icon_names = vec![
            "system-search-symbolic",
            "edit-find-symbolic",
            "search-symbolic",
            "gtk-find",
        ];

        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                break;
            }
        }

        if image.icon_name().is_none() {
            let label = Label::new(Some("🔍"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }

        // Create popover for search
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("search-popover");
        popover.set_autohide(true);

        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!(
                    "Search popover shown - keyboard mode set to OnDemand (active popovers: {})",
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
                    info!("Search popover hidden - keyboard mode set to None");
                }
            } else {
                info!(
                    "Search popover hidden - keeping keyboard mode (active popovers: {})",
                    count
                );
            }
        });

        let main_box = Box::new(Orientation::Vertical, 10);
        main_box.set_margin_top(15);
        main_box.set_margin_bottom(15);
        main_box.set_margin_start(15);
        main_box.set_margin_end(15);
        main_box.set_size_request(600, 500);

        // Search entry and button area
        let search_box = Box::new(Orientation::Horizontal, 10);

        // Search entry
        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Enter search term"));
        search_entry.add_css_class("search-entry");
        search_entry.set_hexpand(true);
        search_entry.set_icon_from_icon_name(
            gtk4::EntryIconPosition::Primary,
            Some("system-search-symbolic"),
        );
        search_box.append(&search_entry);

        // Search button
        let search_button = Button::with_label("Search");
        search_button.add_css_class("search-button");
        search_box.append(&search_button);

        main_box.append(&search_box);

        // Options bar
        let options_box = Box::new(Orientation::Horizontal, 10);
        options_box.set_margin_top(5);
        options_box.set_margin_bottom(10);

        // File type dropdown
        let file_type_box = Box::new(Orientation::Horizontal, 5);
        let file_type_label = Label::new(Some("File Type:"));
        file_type_box.append(&file_type_label);

        let file_type_combo =
            gtk4::DropDown::new(None::<gtk4::StringList>, None::<gtk4::Expression>);
        let file_types = gtk4::StringList::new(&[
            "All Files",
            "Documents",
            "Images",
            "Audio",
            "Video",
            "Folders",
        ]);
        file_type_combo.set_model(Some(&file_types));
        file_type_combo.set_selected(0); // Default to All Files
        file_type_box.append(&file_type_combo);
        options_box.append(&file_type_box);

        options_box.set_hexpand(true);

        // Case sensitive toggle
        let case_check = gtk4::CheckButton::with_label("Case sensitive");
        options_box.append(&case_check);

        // Include hidden files toggle
        let hidden_check = gtk4::CheckButton::with_label("Include hidden files");
        options_box.append(&hidden_check);

        main_box.append(&options_box);

        // Results area
        let results_scroll = ScrolledWindow::new();
        results_scroll.set_vexpand(true);
        results_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);

        let results_list = ListBox::new();
        results_list.add_css_class("search-results-list");
        results_list.set_selection_mode(gtk4::SelectionMode::Single);

        // Empty state
        let empty_label = Label::new(Some("Start typing to search..."));
        empty_label.add_css_class("search-empty-label");
        empty_label.set_vexpand(true);
        results_list.append(&empty_label);

        results_scroll.set_child(Some(&results_list));
        main_box.append(&results_scroll);

        // Status bar
        let status_label = Label::new(Some("Ready to search"));
        status_label.add_css_class("search-status");
        status_label.set_halign(gtk4::Align::Start);
        main_box.append(&status_label);

        popover.set_child(Some(&main_box));

        // Load search config
        let config = SearchConfig::default();

        // Set up search handling
        let (tx, rx) = mpsc::channel::<Vec<SearchResult>>();

        // Create a mutable search config for storing search settings
        let search_config = Rc::new(RefCell::new(config.clone()));

        // Connect file type combo box
        let search_config_clone = search_config.clone();
        file_type_combo.connect_selected_notify(move |combo| {
            let mut config = search_config_clone.borrow_mut();
            match combo.selected() {
                0 => config.file_type = FileType::AllFiles,
                1 => config.file_type = FileType::Documents,
                2 => config.file_type = FileType::Images,
                3 => config.file_type = FileType::Audio,
                4 => config.file_type = FileType::Video,
                5 => config.file_type = FileType::Folders,
                _ => config.file_type = FileType::AllFiles,
            }
        });

        // Connect case sensitive checkbox
        let search_config_clone = search_config.clone();
        case_check.connect_toggled(move |check| {
            let mut config = search_config_clone.borrow_mut();
            config.case_sensitive = check.is_active();
        });

        // Connect hidden files checkbox
        let search_config_clone = search_config.clone();
        hidden_check.connect_toggled(move |check| {
            let mut config = search_config_clone.borrow_mut();
            config.search_hidden = check.is_active();
        });

        // Set up the search button click handler
        let tx_clone = tx.clone();
        let search_config_clone = search_config.clone();
        let search_entry_clone = search_entry.clone();
        let status_weak = status_label.downgrade();
        let results_list_weak = results_list.downgrade();

        let start_search = move || {
            let query = search_entry_clone.text().to_string();

            if query.trim().is_empty() {
                // Don't search with empty query
                if let Some(list) = results_list_weak.upgrade() {
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }

                    let empty_label = Label::new(Some("Enter a search term and click Search"));
                    empty_label.add_css_class("search-empty-label");
                    empty_label.set_vexpand(true);
                    list.append(&empty_label);
                }

                if let Some(status) = status_weak.upgrade() {
                    status.set_text("Ready to search");
                }
                return;
            }

            if let Some(status) = status_weak.upgrade() {
                status.set_text("Searching...");
            }

            // Clear results list and show searching indicator
            if let Some(list) = results_list_weak.upgrade() {
                while let Some(child) = list.first_child() {
                    list.remove(&child);
                }

                let spinner = gtk4::Spinner::new();
                spinner.set_size_request(32, 32);
                spinner.set_vexpand(true);
                spinner.set_hexpand(true);
                spinner.set_halign(gtk4::Align::Center);
                spinner.set_valign(gtk4::Align::Center);
                spinner.start();
                list.append(&spinner);
            }

            let tx = tx_clone.clone();
            let config = search_config_clone.borrow().clone();
            let query_clone = query.clone();

            // Run search in background thread with timeout to prevent hanging
            thread::spawn(move || {
                // Create a channel for search termination
                let (terminate_tx, terminate_rx) = mpsc::channel::<()>();

                // Spawn another thread to do the actual search
                let search_thread =
                    thread::spawn(move || Self::search_files(&query_clone, &config));

                // Create a timeout thread that will signal to terminate after 10 seconds
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(10));
                    let _ = terminate_tx.send(());
                });

                // Wait for either search completion or timeout
                let results = match terminate_rx.recv_timeout(Duration::from_secs(10)) {
                    // Timeout received but still wait for the search thread
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Don't just return empty results, try to get what we have
                        match search_thread.join() {
                            Ok(results) => results,
                            Err(_) => Vec::new(),
                        }
                    }
                    // Channel closed (shouldn't happen)
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        // Try to get results anyway
                        match search_thread.join() {
                            Ok(results) => results,
                            Err(_) => Vec::new(),
                        }
                    }
                };

                // Send the results back to the main thread
                let _ = tx.send(results);
            });
        };

        // Connect search button
        let start_search_clone = start_search.clone();
        search_button.connect_clicked(move |_| {
            start_search_clone();
        });

        // Connect enter key in search entry
        search_entry.connect_activate(move |_| {
            start_search();
        });

        // Handle search results with less frequent checking to reduce CPU usage
        let results_list_weak = results_list.downgrade();
        let status_weak = status_label.downgrade();
        let popover_weak = popover.downgrade();

        // Use a more efficient approach to processing search results
        glib::timeout_add_local(Duration::from_millis(250), move || {
            // Only process one result per timeout to avoid UI freezes
            match rx.try_recv() {
                Ok(results) => {
                    if let (Some(list), Some(status)) =
                        (results_list_weak.upgrade(), status_weak.upgrade())
                    {
                        // Use idle callback to avoid blocking the main thread
                        let list_clone = list.clone();
                        let status_clone = status.clone();
                        let popover_weak_clone = popover_weak.clone();

                        glib::idle_add_local_once(move || {
                            // Clear previous results
                            while let Some(child) = list_clone.first_child() {
                                list_clone.remove(&child);
                            }

                            if results.is_empty() {
                                let no_results = Label::new(Some("No results found"));
                                no_results.add_css_class("search-empty-label");
                                no_results.set_vexpand(true);
                                list_clone.append(&no_results);
                                status_clone.set_text("No results");
                            } else {
                                let count = results.len();
                                // Show fewer results to improve performance
                                let displayed = count.min(50);

                                // Process results in chunks to avoid freezing the UI
                                for (i, result) in results.into_iter().take(displayed).enumerate() {
                                    // Batch operations to reduce UI updates
                                    let row =
                                        Self::create_result_row(result, popover_weak_clone.clone());
                                    list_clone.append(&row);

                                    // Give UI a chance to update for every 10 items
                                    if i % 10 == 9 {
                                        while gtk4::glib::MainContext::default().iteration(false) {}
                                    }
                                }

                                if count > displayed {
                                    status_clone.set_text(&format!(
                                        "Showing {} of {} results",
                                        displayed, count
                                    ));
                                } else {
                                    status_clone.set_text(&format!("{} results", count));
                                }
                            }
                        });
                    }
                }
                Err(_) => {} // No results to process
            }

            glib::ControlFlow::Continue
        });

        // Handle Enter key to activate selected item
        let results_list_for_enter = results_list.clone();
        let _popover_for_enter = popover.downgrade();
        search_entry.connect_activate(move |_| {
            if let Some(selected_row) = results_list_for_enter.selected_row() {
                // Simulate row activation
                results_list_for_enter.emit_by_name::<()>("row-activated", &[&selected_row]);
            }
        });

        // Handle row activation (double-click or Enter)
        let popover_for_activate = popover.downgrade();
        results_list.connect_row_activated(move |_, row| {
            // Get the path from the row's widget name
            let path_str = row.widget_name();
            if !path_str.is_empty() {
                let path = PathBuf::from(path_str.as_str());
                Self::open_file(&path);

                if let Some(popover) = popover_for_activate.upgrade() {
                    popover.popdown();
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
        button.connect_clicked(move |_| {
            popover.popup();
            // Focus search entry
            if let Some(entry) = search_entry_weak.upgrade() {
                entry.grab_focus();
            }
        });

        Ok(Self { button })
    }

    fn search_files(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
        if query.is_empty() {
            return Vec::new();
        }

        let mut results = Vec::new();

        // Try fd first (fastest and best)
        if Self::command_exists("fd") {
            results = Self::search_with_fd(query, config);
        }

        // If fd didn't find anything or isn't available, try ripgrep
        if results.is_empty() && Self::command_exists("rg") {
            results = Self::search_with_ripgrep(query, config);
        }

        // Finally, fall back to find
        if results.is_empty() {
            results = Self::search_with_find(query, config);
        }

        // Filter by file type if needed
        if config.file_type != FileType::AllFiles {
            results = Self::filter_by_file_type(results, &config.file_type);
        }

        // Sort by score (best matches first)
        results.sort_by(|a, b| b.score.cmp(&a.score));

        results
    }

    fn filter_by_file_type(results: Vec<SearchResult>, file_type: &FileType) -> Vec<SearchResult> {
        // Handle folders case separately
        if *file_type == FileType::Folders {
            return results.into_iter().filter(|r| r.path.is_dir()).collect();
        }

        // Get the extensions for this file type
        let extensions = file_type.extensions();
        if extensions.is_empty() {
            return results;
        }

        results
            .into_iter()
            .filter(|r| {
                if let Some(ext) = r.path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    extensions.contains(&ext_str.as_str())
                } else {
                    false
                }
            })
            .collect()
    }

    fn search_with_fd(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
        let mut results = Vec::new();

        for inclusion in &config.inclusions {
            if !inclusion.exists() {
                continue;
            }

            // Use exact fd path from system
            let fd_path = Command::new("which")
                .arg("fd")
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "fd".to_string());

            let mut cmd = Command::new(&fd_path);

            // Basic arguments
            let mut args = vec![
                "--type",
                "f",
                "--type",
                "d",
                "--no-ignore-vcs",
                "--max-results",
                "500",
            ];

            // Add hidden files flag if requested
            if config.search_hidden {
                args.push("--hidden");
            }

            cmd.args(&args);

            // Add exclusions
            for exclusion in &config.exclusions {
                cmd.args(&["--exclude", exclusion]);
            }

            // Use case-sensitivity flag if needed
            if !config.case_sensitive {
                cmd.args(&["--ignore-case"]);
            }

            // Build the search pattern - simplified for better results
            let pattern = if query.contains(" ") {
                // For multi-word queries, use simpler pattern
                query.to_string()
            } else {
                // Simple term, just search for it
                query.to_string()
            };

            cmd.arg(&pattern);
            cmd.current_dir(inclusion);

            tracing::info!("Running fd search command: {:?} in {:?}", cmd, inclusion);

            if let Ok(output) = cmd.output() {
                let output_str = String::from_utf8_lossy(&output.stdout);

                for line in output_str.lines() {
                    let path = inclusion.join(line);
                    if let Some(result) = Self::create_search_result(path, query) {
                        results.push(result);
                    }
                }

                if !output.status.success() {
                    tracing::warn!(
                        "fd command failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            } else {
                tracing::warn!("Failed to execute fd command");
            }
        }

        tracing::info!("fd search found {} results", results.len());
        results
    }

    fn search_with_ripgrep(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
        let mut results = Vec::new();

        for inclusion in &config.inclusions {
            if !inclusion.exists() {
                continue;
            }

            // Use exact ripgrep path from system
            let rg_path = Command::new("which")
                .arg("rg")
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_else(|| "rg".to_string());

            let mut cmd = Command::new(&rg_path);

            // Basic arguments
            let mut args = vec!["--files", "--no-ignore-vcs"];

            // Add hidden files flag if requested
            if config.search_hidden {
                args.push("--hidden");
            }

            cmd.args(&args);

            // Add exclusions
            for exclusion in &config.exclusions {
                cmd.args(&["--glob", &format!("!{}", exclusion)]);
            }

            // Filter by pattern if needed
            if !query.is_empty() {
                cmd.args(&["--glob", &format!("*{}*", query)]);
            }

            cmd.current_dir(inclusion);

            tracing::info!(
                "Running ripgrep search command: {:?} in {:?}",
                cmd,
                inclusion
            );

            if let Ok(output) = cmd.output() {
                let output_str = String::from_utf8_lossy(&output.stdout);

                // Filter results with fuzzy matching
                for line in output_str.lines() {
                    let (matches, score) = Self::fuzzy_match(line, query, config.case_sensitive);
                    if matches {
                        let path = inclusion.join(line);
                        if let Some(mut result) = Self::create_search_result(path, query) {
                            result.score = score;
                            results.push(result);
                        }
                    }
                }

                if !output.status.success() && !output.stderr.is_empty() {
                    tracing::warn!(
                        "ripgrep command failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            } else {
                tracing::warn!("Failed to execute ripgrep command");
            }
        }

        tracing::info!("ripgrep search found {} results", results.len());
        results
    }

    fn search_with_find(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
        let mut results = Vec::new();

        for inclusion in &config.inclusions {
            if !inclusion.exists() {
                continue;
            }

            let mut cmd = Command::new("find");
            cmd.arg(inclusion);
            cmd.args(&["-type", "f", "-o", "-type", "d"]);

            // Add exclusions
            for exclusion in &config.exclusions {
                cmd.args(&["-not", "-path", &format!("*{}*", exclusion)]);
            }

            // Skip hidden files if not requested
            if !config.search_hidden {
                cmd.args(&["-not", "-path", r"*/\.*"]);
            }

            // Add name pattern if there's a query
            if !query.is_empty() {
                let escaped_query = query
                    .replace("[", "\\[")
                    .replace("]", "\\]")
                    .replace("*", "\\*")
                    .replace("?", "\\?");
                cmd.args(&["-name", &format!("*{}*", escaped_query)]);
            }

            tracing::info!("Running find search command: {:?} in {:?}", cmd, inclusion);

            if let Ok(output) = cmd.output() {
                let output_str = String::from_utf8_lossy(&output.stdout);

                // Filter results with fuzzy matching
                for line in output_str.lines() {
                    let path = PathBuf::from(line);
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy();
                        let (matches, score) =
                            Self::fuzzy_match(&name_str, query, config.case_sensitive);
                        if matches {
                            if let Some(mut result) = Self::create_search_result(path, query) {
                                result.score = score;
                                results.push(result);
                            }
                        }
                    }
                }

                if !output.status.success() && !output.stderr.is_empty() {
                    tracing::warn!(
                        "find command failed: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
            } else {
                tracing::warn!("Failed to execute find command");
            }
        }

        tracing::info!("find search found {} results", results.len());
        results
    }

    fn fuzzy_match(text: &str, query: &str, case_sensitive: bool) -> (bool, i32) {
        // If query has multiple words, match each word separately
        if query.contains(' ') {
            let query_parts: Vec<&str> = query.split_whitespace().collect();
            let mut total_score = 0;

            for part in query_parts.iter() {
                let (matches, score) = Self::fuzzy_match_single(text, part, case_sensitive);
                if !matches {
                    return (false, 0); // All parts must match
                }
                total_score += score;
            }

            return (true, total_score);
        }

        // Single word query
        Self::fuzzy_match_single(text, query, case_sensitive)
    }

    fn fuzzy_match_single(text: &str, query: &str, case_sensitive: bool) -> (bool, i32) {
        // Handle empty query
        if query.is_empty() {
            return (true, 0);
        }

        // Normalize case if needed
        let (text_comp, query_comp) = if case_sensitive {
            (text.to_string(), query.to_string())
        } else {
            (text.to_lowercase(), query.to_lowercase())
        };

        // Exact match gets highest score
        if text_comp.contains(&query_comp) {
            let mut score = 100;

            // Bonus if it's at the start of the text or a word
            if text_comp.starts_with(&query_comp) {
                score += 50;
            } else {
                // Check if it's at a word boundary
                let query_start_pos = text_comp.find(&query_comp).unwrap_or(0);
                if query_start_pos > 0
                    && text
                        .chars()
                        .nth(query_start_pos - 1)
                        .map_or(true, |c| c.is_whitespace() || c == '-' || c == '_')
                {
                    score += 25;
                }
            }

            return (true, score);
        }

        // Fall back to character-by-character fuzzy matching
        let mut score = 0;
        let mut query_chars = query_comp.chars();
        let mut current_char = query_chars.next();
        let mut consecutive = 0;
        let mut last_match_idx = 0;

        for (idx, text_char) in text_comp.chars().enumerate() {
            if let Some(qc) = current_char {
                if text_char == qc {
                    // Higher score for consecutive matches
                    score += 10 + consecutive * 5;

                    // Bonus for matches at word boundaries
                    if idx == 0
                        || text.chars().nth(idx - 1).unwrap_or(' ').is_whitespace()
                        || text.chars().nth(idx - 1).unwrap_or(' ') == '_'
                        || text.chars().nth(idx - 1).unwrap_or(' ') == '-'
                    {
                        score += 15;
                    }

                    // Penalty for gaps between matches
                    if idx > last_match_idx + 1 {
                        score -= (idx - last_match_idx - 1) as i32;
                    }

                    consecutive += 1;
                    last_match_idx = idx;
                    current_char = query_chars.next();
                } else {
                    consecutive = 0;
                }
            }
        }

        // All query chars must be found
        (current_char.is_none(), score)
    }

    fn create_search_result(path: PathBuf, query: &str) -> Option<SearchResult> {
        let name = path.file_name()?.to_string_lossy().to_string();
        let (matches, score) = Self::fuzzy_match(&name, query, false); // Default to case insensitive for creation

        if !matches {
            return None;
        }

        // Path formatting for display
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));
        let parent = path
            .parent()
            .and_then(|p| p.strip_prefix(&home).ok())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| {
                path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            });

        // Boost score for exact name matches
        let final_score = if name.to_lowercase() == query.to_lowercase() {
            score + 200
        } else {
            score
        };

        Some(SearchResult {
            path,
            name,
            parent: if parent.is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", parent)
            },
            score: final_score,
        })
    }

    fn command_exists(cmd: &str) -> bool {
        // Use specific paths for common tools when available
        match cmd {
            "fd" => {
                if let Ok(output) = Command::new("which").arg("fd").output() {
                    if output.status.success() {
                        return true;
                    }
                }
                // Check specific Nix store locations
                std::path::Path::new("/nix/store").exists()
                    && Command::new("find")
                        .args(["/nix/store", "-name", "fd", "-type", "f", "-executable"])
                        .output()
                        .map(|output| !output.stdout.is_empty())
                        .unwrap_or(false)
            }
            "rg" => {
                if let Ok(output) = Command::new("which").arg("rg").output() {
                    if output.status.success() {
                        return true;
                    }
                }
                // Check specific Nix store locations
                std::path::Path::new("/nix/store").exists()
                    && Command::new("find")
                        .args(["/nix/store", "-name", "rg", "-type", "f", "-executable"])
                        .output()
                        .map(|output| !output.stdout.is_empty())
                        .unwrap_or(false)
            }
            _ => Command::new("which")
                .arg(cmd)
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false),
        }
    }

    fn create_result_row(
        result: SearchResult,
        popover_weak: gtk4::glib::WeakRef<Popover>,
    ) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("search-result-row");

        // Store path in widget name (safer than using data)
        row.set_widget_name(&result.path.to_string_lossy());

        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(10);
        hbox.set_margin_end(10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);

        // File/folder icon
        let icon_name = if result.path.is_dir() {
            "folder-symbolic"
        } else {
            Self::get_file_icon(&result.path)
        };

        let icon = Image::from_icon_name(icon_name);
        icon.set_pixel_size(24);
        hbox.append(&icon);

        // File info
        let vbox = Box::new(Orientation::Vertical, 2);
        vbox.set_hexpand(true);

        let name_label = Label::new(Some(&result.name));
        name_label.set_halign(gtk4::Align::Start);
        name_label.add_css_class("search-result-name");
        name_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        vbox.append(&name_label);

        let path_label = Label::new(Some(&result.parent));
        path_label.set_halign(gtk4::Align::Start);
        path_label.add_css_class("search-result-path");
        path_label.set_ellipsize(gtk4::pango::EllipsizeMode::Middle);
        vbox.append(&path_label);

        hbox.append(&vbox);

        // Action buttons
        let actions_box = Box::new(Orientation::Horizontal, 5);

        // Open button
        let open_button = Button::from_icon_name("document-open-symbolic");
        open_button.add_css_class("search-result-action");
        open_button.set_tooltip_text(Some("Open"));

        let path = result.path.clone();
        let popover_weak_open = popover_weak.clone();
        open_button.connect_clicked(move |_| {
            Self::open_file(&path);
            if let Some(popover) = popover_weak_open.upgrade() {
                popover.popdown();
            }
        });
        actions_box.append(&open_button);

        // Show in folder button
        let folder_button = Button::from_icon_name("folder-open-symbolic");
        folder_button.add_css_class("search-result-action");
        folder_button.set_tooltip_text(Some("Show in folder"));

        let path = result.path.clone();
        let popover_weak_folder = popover_weak.clone();
        folder_button.connect_clicked(move |_| {
            Self::show_in_folder(&path);
            if let Some(popover) = popover_weak_folder.upgrade() {
                popover.popdown();
            }
        });
        actions_box.append(&folder_button);

        // Copy path button
        let copy_button = Button::from_icon_name("edit-copy-symbolic");
        copy_button.add_css_class("search-result-action");
        copy_button.set_tooltip_text(Some("Copy path"));

        let path = result.path.clone();
        copy_button.connect_clicked(move |button| {
            if let Some(display) = gtk4::gdk::Display::default() {
                let clipboard = display.clipboard();
                clipboard.set_text(&path.to_string_lossy());

                // Visual feedback
                button.add_css_class("copied");
                let button_weak = button.downgrade();
                glib::timeout_add_local_once(Duration::from_millis(1000), move || {
                    if let Some(button) = button_weak.upgrade() {
                        button.remove_css_class("copied");
                    }
                });
            }
        });
        actions_box.append(&copy_button);

        hbox.append(&actions_box);

        row.set_child(Some(&hbox));
        row
    }

    fn get_file_icon(path: &PathBuf) -> &'static str {
        if let Some(ext) = path.extension() {
            match ext.to_string_lossy().to_lowercase().as_str() {
                "txt" | "md" | "rst" => "text-x-generic-symbolic",
                "pdf" => "application-pdf-symbolic",
                "doc" | "docx" | "odt" => "x-office-document-symbolic",
                "xls" | "xlsx" | "ods" => "x-office-spreadsheet-symbolic",
                "png" | "jpg" | "jpeg" | "gif" | "svg" => "image-x-generic-symbolic",
                "mp3" | "ogg" | "wav" | "flac" => "audio-x-generic-symbolic",
                "mp4" | "avi" | "mkv" | "webm" => "video-x-generic-symbolic",
                "zip" | "tar" | "gz" | "bz2" | "7z" => "package-x-generic-symbolic",
                "py" | "js" | "rs" | "c" | "cpp" | "java" => "text-x-script-symbolic",
                "nix" => "application-x-nix-symbolic",
                _ => "text-x-generic-symbolic",
            }
        } else {
            "text-x-generic-symbolic"
        }
    }

    fn open_file(path: &PathBuf) {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }

    fn show_in_folder(path: &PathBuf) {
        let folder = if path.is_dir() {
            path.clone()
        } else {
            path.parent().map(|p| p.to_path_buf()).unwrap_or_default()
        };

        // Try different file managers
        for fm in &["thunar", "nautilus", "nemo", "dolphin", "pcmanfm"] {
            if Command::new(fm).arg(&folder).spawn().is_ok() {
                return;
            }
        }

        // Fallback to xdg-open
        let _ = Command::new("xdg-open").arg(&folder).spawn();
    }

    pub fn widget(&self) -> &Button {
        &self.button
    }
}
