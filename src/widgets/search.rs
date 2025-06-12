use gtk4::prelude::*;
use gtk4::{Button, Image, Popover, Box, Orientation, Label, Entry, ListBox, ListBoxRow, ScrolledWindow, Separator};
use anyhow::Result;
use std::process::{Command, Stdio};
use std::io::Write;
use std::thread;
use std::sync::mpsc;
use tracing::{info, warn};
use std::path::PathBuf;
use std::time::Duration;

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
        }
    }
}

impl Search {
    pub fn new() -> Result<Self> {
        let button = Button::new();
        button.add_css_class("search");
        
        // Try multiple search icon fallbacks
        let icon_names = vec![
            "system-search-symbolic",
            "edit-find-symbolic",
            "search-symbolic",
            "gtk-find"
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
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        
        let main_box = Box::new(Orientation::Vertical, 10);
        main_box.set_margin_top(15);
        main_box.set_margin_bottom(15);
        main_box.set_margin_start(15);
        main_box.set_margin_end(15);
        main_box.set_size_request(550, 450);
        
        // Search entry
        let search_entry = Entry::new();
        search_entry.set_placeholder_text(Some("Type to search files and folders..."));
        search_entry.add_css_class("search-entry");
        search_entry.set_hexpand(true);
        
        // Add search icon to entry
        search_entry.set_icon_from_icon_name(gtk4::EntryIconPosition::Primary, Some("system-search-symbolic"));
        
        main_box.append(&search_entry);
        
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
        
        // Track the last search to avoid duplicate searches
        let last_query = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
        
        // Handle search input with debouncing
        let tx_clone = tx.clone();
        let status_weak = status_label.downgrade();
        let results_list_weak = results_list.downgrade();
        let config_clone = config.clone();
        let last_query_clone = last_query.clone();
        
        let search_timeout_handle = std::rc::Rc::new(std::cell::RefCell::new(None::<glib::SourceId>));
        let search_timeout_handle_clone = search_timeout_handle.clone();
        
        search_entry.connect_changed(move |entry| {
            let query = entry.text().to_string();
            
            // Cancel previous timeout safely
            let mut timeout_handle = search_timeout_handle_clone.borrow_mut();
            *timeout_handle = None; // This automatically drops and cancels the old timeout
            
            if query.is_empty() {
                // Clear results immediately for empty query
                if let Some(list) = results_list_weak.upgrade() {
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }
                    
                    let empty_label = Label::new(Some("Start typing to search..."));
                    empty_label.add_css_class("search-empty-label");
                    empty_label.set_vexpand(true);
                    list.append(&empty_label);
                }
                
                if let Some(status) = status_weak.upgrade() {
                    status.set_text("Ready to search");
                }
                return;
            }
            
            let tx = tx_clone.clone();
            let config = config_clone.clone();
            let status_weak_for_timeout = status_weak.clone();
            let last_query = last_query_clone.clone();
            
            // Set up new timeout for debouncing
            let timeout_id = glib::timeout_add_local(Duration::from_millis(300), move || {
                let current_query = query.clone();
                
                // Check if query changed
                if *last_query.borrow() == current_query {
                    return glib::ControlFlow::Break;
                }
                
                *last_query.borrow_mut() = current_query.clone();
                
                if let Some(status) = status_weak_for_timeout.upgrade() {
                    status.set_text("Searching...");
                }
                
                let tx = tx.clone();
                let config = config.clone();
                
                // Run search in background thread
                thread::spawn(move || {
                    let results = Self::fuzzy_search_files(&current_query, &config);
                    let _ = tx.send(results);
                });
                
                glib::ControlFlow::Break
            });
            
            *timeout_handle = Some(timeout_id);
        });
        
        // Handle search results
        let results_list_weak = results_list.downgrade();
        let status_weak = status_label.downgrade();
        let popover_weak = popover.downgrade();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            if let Ok(results) = rx.try_recv() {
                if let (Some(list), Some(status)) = (results_list_weak.upgrade(), status_weak.upgrade()) {
                    // Clear previous results
                    while let Some(child) = list.first_child() {
                        list.remove(&child);
                    }
                    
                    if results.is_empty() {
                        let no_results = Label::new(Some("No results found"));
                        no_results.add_css_class("search-empty-label");
                        no_results.set_vexpand(true);
                        list.append(&no_results);
                        status.set_text("No results");
                    } else {
                        let count = results.len();
                        let displayed = count.min(100);
                        
                        for result in results.into_iter().take(displayed) {
                            let row = Self::create_result_row(result, popover_weak.clone());
                            list.append(&row);
                        }
                        
                        if count > displayed {
                            status.set_text(&format!("Showing {} of {} results", displayed, count));
                        } else {
                            status.set_text(&format!("{} results", count));
                        }
                    }
                }
            }
            glib::ControlFlow::Continue
        });
        
        // Handle Enter key to activate selected item
        let results_list_for_enter = results_list.clone();
        let popover_for_enter = popover.downgrade();
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
    
    fn fuzzy_search_files(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
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
        
        // Sort by score (best matches first)
        results.sort_by(|a, b| b.score.cmp(&a.score));
        
        results
    }
    
    fn search_with_fd(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
        let mut results = Vec::new();
        
        for inclusion in &config.inclusions {
            if !inclusion.exists() {
                continue;
            }
            
            let mut cmd = Command::new("fd");
            cmd.args(&[
                "--type", "f",
                "--type", "d",
                "--hidden",
                "--no-ignore-vcs",
                "--max-results", "200",
            ]);
            
            // Add exclusions
            for exclusion in &config.exclusions {
                cmd.args(&["--exclude", exclusion]);
            }
            
            // Use regex for fuzzy matching
            let pattern = query.chars()
                .map(|c| format!(".*{}", regex::escape(&c.to_string())))
                .collect::<String>();
            
            cmd.arg(&pattern);
            cmd.current_dir(inclusion);
            
            if let Ok(output) = cmd.output() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                
                for line in output_str.lines() {
                    let path = inclusion.join(line);
                    if let Some(result) = Self::create_search_result(path, query) {
                        results.push(result);
                    }
                }
            }
        }
        
        results
    }
    
    fn search_with_ripgrep(query: &str, config: &SearchConfig) -> Vec<SearchResult> {
        let mut results = Vec::new();
        
        for inclusion in &config.inclusions {
            if !inclusion.exists() {
                continue;
            }
            
            let mut cmd = Command::new("rg");
            cmd.args(&[
                "--files",
                "--hidden",
                "--no-ignore-vcs",
                "--max-count", "200",
            ]);
            
            // Add exclusions
            for exclusion in &config.exclusions {
                cmd.args(&["--glob", &format!("!{}", exclusion)]);
            }
            
            cmd.current_dir(inclusion);
            
            if let Ok(output) = cmd.output() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                
                // Filter results with fuzzy matching
                for line in output_str.lines() {
                    if Self::fuzzy_match(line, query).0 {
                        let path = inclusion.join(line);
                        if let Some(result) = Self::create_search_result(path, query) {
                            results.push(result);
                        }
                    }
                }
            }
        }
        
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
            
            if let Ok(output) = cmd.output() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                
                // Filter results with fuzzy matching
                for line in output_str.lines() {
                    let path = PathBuf::from(line);
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy();
                        if Self::fuzzy_match(&name_str, query).0 {
                            if let Some(result) = Self::create_search_result(path, query) {
                                results.push(result);
                            }
                        }
                    }
                }
            }
        }
        
        results
    }
    
    fn fuzzy_match(text: &str, query: &str) -> (bool, i32) {
        let text_lower = text.to_lowercase();
        let query_lower = query.to_lowercase();
        
        let mut score = 0;
        let mut query_chars = query_lower.chars();
        let mut current_char = query_chars.next();
        let mut consecutive = 0;
        let mut last_match_idx = 0;
        
        for (idx, text_char) in text_lower.chars().enumerate() {
            if let Some(qc) = current_char {
                if text_char == qc {
                    // Higher score for consecutive matches
                    score += 10 + consecutive * 5;
                    
                    // Bonus for matches at word boundaries
                    if idx == 0 || text.chars().nth(idx - 1).unwrap_or(' ').is_whitespace() {
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
        let (matches, score) = Self::fuzzy_match(&name, query);
        
        if !matches {
            return None;
        }
        
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));
        let parent = path.parent()
            .and_then(|p| p.strip_prefix(&home).ok())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| {
                path.parent()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default()
            });
        
        Some(SearchResult {
            path,
            name,
            parent: if parent.is_empty() { "~".to_string() } else { format!("~/{}", parent) },
            score,
        })
    }
    
    fn command_exists(cmd: &str) -> bool {
        Command::new("which")
            .arg(cmd)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
    
    fn create_result_row(result: SearchResult, popover_weak: gtk4::glib::WeakRef<Popover>) -> ListBoxRow {
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
        let _ = Command::new("xdg-open")
            .arg(path)
            .spawn();
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
