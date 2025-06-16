use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Popover, ApplicationWindow, Entry, ScrolledWindow, ListBox, ListBoxRow, Image, SearchEntry, Separator};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::{WeakRef, clone};
use std::rc::Rc;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn, error};
use anyhow::Result;
use crate::config::{PanelConfig, GitRepository, GitService};

pub struct Git {
    button: Button,
    repositories: Rc<RefCell<Vec<GitRepository>>>,
    services: Rc<RefCell<Vec<GitService>>>,
}

impl Git {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
        config: &PanelConfig,
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("git");
        
        let container = Box::new(Orientation::Horizontal, 5);
        
        // Nerd Font icon for Git
        let icon = Label::new(Some("󰊢"));
        icon.add_css_class("git-icon");
        
        container.append(&icon);
        button.set_child(Some(&container));
        
        // Create popover for git details
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("git-popover");
        
        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Git popover shown - keyboard mode set to OnDemand (active popovers: {})", 
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
                    info!("Git popover hidden - keyboard mode set to None");
                }
            } else {
                info!("Git popover hidden - keeping keyboard mode (active popovers: {})", count);
            }
        });
        
        // Main popover container
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(15);
        popover_box.set_margin_bottom(15);
        popover_box.set_margin_start(15);
        popover_box.set_margin_end(15);
        popover_box.set_size_request(500, 400);
        
        // Title
        let title_label = Label::new(Some("Git Repositories"));
        title_label.add_css_class("git-title");
        title_label.set_halign(gtk4::Align::Start);
        popover_box.append(&title_label);
        
        // Search and filter area
        let search_box = Box::new(Orientation::Horizontal, 10);
        search_box.set_margin_top(10);
        search_box.set_margin_bottom(10);
        
        // Search entry
        let search_entry = SearchEntry::new();
        search_entry.set_placeholder_text(Some("Search repositories..."));
        search_entry.set_hexpand(true);
        search_box.append(&search_entry);
        
        // Service filter dropdown
        let service_box = Box::new(Orientation::Horizontal, 5);
        let service_label = Label::new(Some("Service:"));
        service_box.append(&service_label);
        
        let service_combo = gtk4::DropDown::new(None::<gtk4::StringList>, None::<gtk4::Expression>);
        let mut service_names = vec!["All"];
        service_names.extend(config.git.services.iter().map(|s| s.name.as_str()));
        let service_list = gtk4::StringList::new(&service_names);
        service_combo.set_model(Some(&service_list));
        service_combo.set_selected(0);
        service_box.append(&service_combo);
        search_box.append(&service_box);
        
        popover_box.append(&search_box);
        
        // Repositories list
        let scrolled_window = ScrolledWindow::new();
        scrolled_window.set_vexpand(true);
        scrolled_window.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        
        let repos_list = ListBox::new();
        repos_list.add_css_class("git-repos-list");
        repos_list.set_selection_mode(gtk4::SelectionMode::None);
        scrolled_window.set_child(Some(&repos_list));
        
        popover_box.append(&scrolled_window);
        
        // Store repositories and services from config
        let repositories = Rc::new(RefCell::new(config.git.repositories.clone()));
        let services = Rc::new(RefCell::new(config.git.services.clone()));
        
        // Populate the list with repositories
        Self::populate_repo_list(&repos_list, &repositories.borrow(), None, None);
        
        // Handle search
        let repositories_for_search = repositories.clone();
        let services_for_search = services.clone();
        let repos_list_for_search = repos_list.clone();
        search_entry.connect_search_changed(move |entry| {
            let search_text = entry.text().to_string();
            let search_term = if search_text.is_empty() { None } else { Some(search_text) };
            Self::populate_repo_list(&repos_list_for_search, &repositories_for_search.borrow(), search_term, None);
        });
        
        // Handle service filter
        let repositories_for_filter = repositories.clone();
        let services_for_filter = services.clone();
        let repos_list_for_filter = repos_list.clone();
        let search_entry_for_filter = search_entry.clone();
        service_combo.connect_selected_notify(move |combo| {
            let selected = combo.selected();
            let service_filter = if selected == 0 {
                None
            } else {
                let services = services_for_filter.borrow();
                if selected as usize <= services.len() {
                    Some(services[selected as usize - 1].name.clone())
                } else {
                    None
                }
            };
            
            let search_text = search_entry_for_filter.text().to_string();
            let search_term = if search_text.is_empty() { None } else { Some(search_text) };
            
            Self::populate_repo_list(&repos_list_for_filter, &repositories_for_filter.borrow(), search_term, service_filter);
        });
        
        // Handle Escape key to close popover
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
        
        popover.set_child(Some(&popover_box));
        
        // Show popover on click
        let search_entry_weak = search_entry.downgrade();
        button.connect_clicked(move |_| {
            popover.popup();
            // Focus search entry
            if let Some(entry) = search_entry_weak.upgrade() {
                entry.grab_focus();
            }
        });
        
        Ok(Self { 
            button,
            repositories,
            services,
        })
    }
    
    fn populate_repo_list(
        list_box: &ListBox, 
        repositories: &[GitRepository], 
        search_term: Option<String>,
        service_filter: Option<String>
    ) {
        // Clear existing items
        while let Some(child) = list_box.first_child() {
            list_box.remove(&child);
        }
        
        let filtered_repos: Vec<&GitRepository> = repositories.iter()
            .filter(|repo| {
                // Apply service filter if any
                if let Some(service) = &service_filter {
                    if repo.service != *service {
                        return false;
                    }
                }
                
                // Apply search term if any
                if let Some(term) = &search_term {
                    let term_lower = term.to_lowercase();
                    let name_lower = repo.name.to_lowercase();
                    
                    name_lower.contains(&term_lower) || 
                    repo.path.to_lowercase().contains(&term_lower) ||
                    repo.url.to_lowercase().contains(&term_lower)
                } else {
                    true
                }
            })
            .collect();
        
        if filtered_repos.is_empty() {
            let no_results = Label::new(Some("No repositories found"));
            no_results.add_css_class("git-no-results");
            no_results.set_margin_top(20);
            no_results.set_margin_bottom(20);
            no_results.set_hexpand(true);
            no_results.set_halign(gtk4::Align::Center);
            list_box.append(&no_results);
            return;
        }
        
        for (index, repo) in filtered_repos.iter().enumerate() {
            let row = Self::create_repo_row(repo);
            list_box.append(&row);
            
            // Add separator after each repository except the last one
            if index < filtered_repos.len() - 1 {
                let separator = Separator::new(Orientation::Horizontal);
                separator.set_margin_top(5);
                separator.set_margin_bottom(5);
                list_box.append(&separator);
            }
        }
    }
    
    fn create_repo_row(repo: &GitRepository) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("git-repo-row");
        
        let vbox = Box::new(Orientation::Vertical, 5);
        vbox.set_margin_start(10);
        vbox.set_margin_end(10);
        vbox.set_margin_top(10);
        vbox.set_margin_bottom(10);
        
        // Header with name and service
        let header_box = Box::new(Orientation::Horizontal, 5);
        
        // Repository name
        let name_label = Label::new(Some(&repo.name));
        name_label.add_css_class("git-repo-name");
        name_label.set_halign(gtk4::Align::Start);
        name_label.set_hexpand(true);
        header_box.append(&name_label);
        
        // Service badge
        let service_badge = Label::new(Some(&repo.service));
        service_badge.add_css_class("git-service-badge");
        header_box.append(&service_badge);
        
        vbox.append(&header_box);
        
        // Path
        let path_box = Box::new(Orientation::Horizontal, 5);
        
        let path_icon = Image::from_icon_name("folder-symbolic");
        path_icon.set_pixel_size(14);
        path_box.append(&path_icon);
        
        let expanded_path = Self::expand_tilde(&repo.path);
        let path_label = Label::new(Some(&expanded_path.to_string_lossy()));
        path_label.add_css_class("git-repo-path");
        path_label.set_halign(gtk4::Align::Start);
        path_box.append(&path_label);
        
        // Open folder button
        let folder_button = Button::from_icon_name("folder-open-symbolic");
        folder_button.add_css_class("git-action-button");
        folder_button.set_tooltip_text(Some("Open folder"));
        
        let path_clone = expanded_path.clone();
        folder_button.connect_clicked(move |_| {
            Self::open_folder(&path_clone);
        });
        
        path_box.append(&folder_button);
        vbox.append(&path_box);
        
        // Actions
        let actions_box = Box::new(Orientation::Horizontal, 10);
        actions_box.set_margin_top(5);
        
        // Repository URL
        let url_button = Button::with_label("Repository");
        url_button.add_css_class("git-url-button");
        
        let url = repo.url.clone();
        url_button.connect_clicked(move |_| {
            Self::open_url(&url);
        });
        
        actions_box.append(&url_button);
        
        // Issues URL
        let issues_url = Self::build_issues_url(repo);
        let issues_button = Button::with_label("Issues");
        issues_button.add_css_class("git-issues-button");
        
        let issues_url_clone = issues_url.clone();
        issues_button.connect_clicked(move |_| {
            Self::open_url(&issues_url_clone);
        });
        
        actions_box.append(&issues_button);
        
        // Copy URL button
        let copy_button = Button::with_label("Copy URL");
        copy_button.add_css_class("git-copy-button");
        
        let url_to_copy = repo.url.clone();
        copy_button.connect_clicked(move |button| {
            if let Some(display) = gtk4::gdk::Display::default() {
                let clipboard = display.clipboard();
                clipboard.set_text(&url_to_copy);
                
                // Visual feedback
                button.add_css_class("copied");
                let button_weak = button.downgrade();
                glib::timeout_add_local_once(std::time::Duration::from_millis(1000), move || {
                    if let Some(button) = button_weak.upgrade() {
                        button.remove_css_class("copied");
                    }
                });
            }
        });
        
        actions_box.append(&copy_button);
        
        vbox.append(&actions_box);
        
        row.set_child(Some(&vbox));
        row
    }
    
    fn expand_tilde(path: &str) -> PathBuf {
        if path.starts_with("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(&path[2..]);
            }
        }
        PathBuf::from(path)
    }
    
    fn open_folder(path: &Path) {
        // Try to open with default file manager
        let file_managers = vec![
            "nautilus", "nemo", "thunar", "dolphin", "pcmanfm", "caja"
        ];
        
        for fm in file_managers {
            if Command::new(fm).arg(path).spawn().is_ok() {
                return;
            }
        }
        
        // Fallback to xdg-open
        if Command::new("xdg-open").arg(path).spawn().is_err() {
            error!("Failed to open folder: {}", path.display());
        }
    }
    
    fn open_url(url: &str) {
        let browsers = vec![
            "xdg-open", "firefox", "chromium", "google-chrome", "brave-browser"
        ];
        
        for browser in browsers {
            if Command::new(browser).arg(url).spawn().is_ok() {
                return;
            }
        }
        
        error!("Failed to open URL: {}", url);
    }
    
    fn build_issues_url(repo: &GitRepository) -> String {
        // For now, just append /issues to the repository URL
        // In a more complete implementation, we would use the service's issue pattern
        format!("{}/issues", repo.url)
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}