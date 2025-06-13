use gtk4::prelude::*;
use gtk4::{Box, Button, Orientation, Popover, ListBox, ListBoxRow, Label, Image};
use anyhow::Result;
use std::process::Command;
use tracing::{info, warn};
use serde_json::Value;

pub struct Workspaces {
    container: Box,
}

#[derive(Debug, Clone)]
struct WindowInfo {
    id: u64,
    title: String,
    app_id: Option<String>,
    workspace_id: u64,
}

#[derive(Debug, Clone)]
struct WorkspaceInfo {
    id: u64,
    idx: u32,
    #[allow(dead_code)]
    is_active: bool,
    is_focused: bool,
    windows: Vec<WindowInfo>,
}

impl Workspaces {
    pub fn new() -> Result<Self> {
        let container = Box::new(Orientation::Horizontal, 5);
        container.add_css_class("workspaces");
        
        // Get initial workspace info
        let workspaces = Self::get_workspace_info();
        
        // Create workspace buttons
        for workspace in &workspaces {
            let button = Self::create_workspace_button(workspace);
            container.append(&button);
        }
        
        // Set up periodic updates
        let container_weak = container.downgrade();
        glib::timeout_add_seconds_local(2, move || {
            if let Some(container) = container_weak.upgrade() {
                Self::update_workspaces(&container);
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
        
        Ok(Self { container })
    }
    
    fn create_workspace_button(workspace: &WorkspaceInfo) -> Button {
        let button = Button::with_label(&workspace.idx.to_string());
        button.add_css_class("workspace");
        
        if workspace.is_focused {
            button.add_css_class("active");
        }
        
        // Store workspace info in widget name (safer than set_data)
        button.set_widget_name(&format!("{}:{}", workspace.id, workspace.idx));
        
        info!("Creating button for workspace {} with id {}", workspace.idx, workspace.id);
        
        // Left click - switch workspace
        let workspace_idx = workspace.idx;
        button.connect_clicked(move |_| {
            Self::switch_workspace(workspace_idx);
        });
        
        // Right click - show window picker
        let gesture = gtk4::GestureClick::new();
        gesture.set_button(3); // Right mouse button
        
        let workspace_id = workspace.id;
        let workspace_idx_for_popover = workspace.idx;
        gesture.connect_released(move |gesture, _, x, y| {
            let widget = gesture.widget();
            if let Some(button) = widget.downcast_ref::<Button>() {
                Self::show_window_picker(button, workspace_id, workspace_idx_for_popover, x, y);
            }
        });
        button.add_controller(gesture);
        
        button
    }
    
    fn show_window_picker(button: &Button, workspace_id: u64, workspace_idx: u32, _x: f64, _y: f64) {
        info!("Showing window picker for workspace {} (id: {})", workspace_idx, workspace_id);
        
        // Create popover on demand
        let popover = Popover::new();
        popover.set_parent(button);
        popover.add_css_class("workspace-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        popover.set_position(gtk4::PositionType::Bottom);
        
        let popover_box = Box::new(Orientation::Vertical, 5);
        popover_box.set_margin_top(10);
        popover_box.set_margin_bottom(10);
        popover_box.set_margin_start(10);
        popover_box.set_margin_end(10);
        popover_box.set_size_request(300, -1);
        
        // Workspace title
        let title_label = Label::new(Some(&format!("Workspace {} Windows", workspace_idx)));
        title_label.add_css_class("workspace-popover-title");
        title_label.set_halign(gtk4::Align::Start);
        popover_box.append(&title_label);
        
        // Get current windows for this workspace
        let workspaces = Self::get_workspace_info();
        info!("Found {} workspaces total", workspaces.len());
        
        if let Some(workspace) = workspaces.iter().find(|w| w.id == workspace_id) {
            info!("Found workspace with {} windows", workspace.windows.len());
            
            if workspace.windows.is_empty() {
                let empty_label = Label::new(Some("No windows"));
                empty_label.add_css_class("workspace-empty-label");
                empty_label.set_margin_top(20);
                empty_label.set_margin_bottom(20);
                popover_box.append(&empty_label);
            } else {
                let window_list = ListBox::new();
                window_list.add_css_class("workspace-window-list");
                window_list.set_selection_mode(gtk4::SelectionMode::None);
                
                for window in &workspace.windows {
                    info!("Adding window: {} (id: {})", window.title, window.id);
                    let row = Self::create_window_row(window, popover.downgrade());
                    window_list.append(&row);
                }
                popover_box.append(&window_list);
            }
        } else {
            warn!("Could not find workspace with id {}", workspace_id);
            let error_label = Label::new(Some("Error loading windows"));
            error_label.add_css_class("workspace-empty-label");
            popover_box.append(&error_label);
        }
        
        popover.set_child(Some(&popover_box));
        
        // Clean up popover when closed
        popover.connect_closed(|popover| {
            popover.unparent();
        });
        
        popover.popup();
    }
    
    fn create_window_row(window: &WindowInfo, popover_weak: gtk4::glib::WeakRef<Popover>) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("workspace-window-row");
        
        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(5);
        hbox.set_margin_end(5);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);
        
        // App icon (use generic if app_id not available)
        let icon = if let Some(app_id) = &window.app_id {
            Image::from_icon_name(&Self::get_app_icon(app_id))
        } else {
            Image::from_icon_name("application-x-executable-symbolic")
        };
        icon.set_pixel_size(24);
        hbox.append(&icon);
        
        // Window title
        let vbox = Box::new(Orientation::Vertical, 2);
        vbox.set_hexpand(true);
        
        let title_label = Label::new(Some(&window.title));
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        title_label.add_css_class("workspace-window-title");
        vbox.append(&title_label);
        
        if let Some(app_id) = &window.app_id {
            let app_label = Label::new(Some(app_id));
            app_label.set_halign(gtk4::Align::Start);
            app_label.add_css_class("workspace-window-app");
            vbox.append(&app_label);
        }
        
        hbox.append(&vbox);
        
        // Focus button
        let focus_button = Button::from_icon_name("go-jump-symbolic");
        focus_button.add_css_class("workspace-window-focus");
        focus_button.set_tooltip_text(Some("Focus this window"));
        
        let window_id = window.id;
        let popover_weak_focus = popover_weak.clone();
        focus_button.connect_clicked(move |_| {
            Self::focus_window(window_id);
            if let Some(popover) = popover_weak_focus.upgrade() {
                popover.popdown();
            }
        });
        hbox.append(&focus_button);
        
        // Close button
        let close_button = Button::from_icon_name("window-close-symbolic");
        close_button.add_css_class("workspace-window-close");
        close_button.set_tooltip_text(Some("Close this window"));
        
        let window_id_close = window.id;
        close_button.connect_clicked(move |_| {
            Self::close_window(window_id_close);
            if let Some(popover) = popover_weak.upgrade() {
                popover.popdown();
            }
        });
        hbox.append(&close_button);
        
        row.set_child(Some(&hbox));
        row
    }
    
    fn get_app_icon(app_id: &str) -> String {
        // Common app_id to icon mappings
        let icon = match app_id.to_lowercase().as_str() {
            "firefox" | "firefox-esr" => "firefox",
            "google-chrome" | "chrome" => "google-chrome",
            "chromium" | "chromium-browser" => "chromium",
            "code" | "vscode" => "code",
            "terminal" | "gnome-terminal" => "utilities-terminal",
            "nautilus" | "files" => "system-file-manager",
            "thunderbird" => "thunderbird",
            "slack" => "slack",
            "discord" => "discord",
            "spotify" => "spotify",
            _ => {
                // Try the app_id directly, fall back to generic
                return format!("{}-symbolic", app_id);
            }
        };
        
        format!("{}-symbolic", icon)
    }
    
    fn update_workspaces(container: &Box) {
        let workspaces = Self::get_workspace_info();
        
        // Get current buttons
        let mut current_buttons = Vec::new();
        let mut child = container.first_child();
        while let Some(widget) = child {
            if widget.is::<Button>() {
                current_buttons.push(widget.clone());
            }
            child = widget.next_sibling();
        }
        
        // Update existing buttons or add new ones
        for (idx, workspace) in workspaces.iter().enumerate() {
            if let Some(button) = current_buttons.get(idx) {
                if let Some(button) = button.downcast_ref::<Button>() {
                    // Update label
                    button.set_label(&workspace.idx.to_string());
                    
                    // Update active state
                    if workspace.is_focused {
                        button.add_css_class("active");
                    } else {
                        button.remove_css_class("active");
                    }
                    
                    // Update stored data
                    button.set_widget_name(&format!("{}:{}", workspace.id, workspace.idx));
                }
            } else {
                // Add new button if needed
                let button = Self::create_workspace_button(workspace);
                container.append(&button);
            }
        }
        
        // Remove extra buttons
        while current_buttons.len() > workspaces.len() {
            if let Some(button) = current_buttons.pop() {
                container.remove(&button);
            }
        }
    }
    
    fn get_workspace_info() -> Vec<WorkspaceInfo> {
        let mut workspaces = Vec::new();
        
        // Get workspace information first
        if let Ok(output) = Command::new("niri")
            .args(&["msg", "-j", "workspaces"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            info!("Raw workspace output: {}", output_str);
            
            if let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(workspaces_array) = json.as_array() {
                    // Get all windows
                    let mut all_windows: Vec<WindowInfo> = Vec::new();
                    if let Ok(windows_output) = Command::new("niri")
                        .args(&["msg", "-j", "windows"])
                        .output()
                    {
                        if let Ok(windows_json) = serde_json::from_slice::<Value>(&windows_output.stdout) {
                            if let Some(windows_array) = windows_json.as_array() {
                                for window_json in windows_array {
                                    if let (Some(id), Some(title), Some(workspace_id)) = (
                                        window_json["id"].as_u64(),
                                        window_json["title"].as_str(),
                                        window_json["workspace_id"].as_u64()
                                    ) {
                                        all_windows.push(WindowInfo {
                                            id,
                                            title: title.to_string(),
                                            app_id: window_json["app_id"].as_str().map(|s| s.to_string()),
                                            workspace_id,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    
                    // Create workspace info with proper IDs
                    for (idx, workspace_json) in workspaces_array.iter().enumerate() {
                        let workspace_id = workspace_json["id"].as_u64().unwrap_or(idx as u64);
                        let is_active = workspace_json["is_active"].as_bool().unwrap_or(false);
                        let is_focused = workspace_json["is_focused"].as_bool().unwrap_or(false);
                        
                        info!("Workspace {} has id {}", idx + 1, workspace_id);
                        
                        // Get windows for this workspace
                        let windows: Vec<WindowInfo> = all_windows
                            .iter()
                            .filter(|w| w.workspace_id == workspace_id)
                            .cloned()
                            .collect();
                        
                        info!("Workspace {} (id {}) has {} windows", idx + 1, workspace_id, windows.len());
                        
                        workspaces.push(WorkspaceInfo {
                            id: workspace_id,
                            idx: (idx + 1) as u32,
                            is_active,
                            is_focused,
                            windows,
                        });
                    }
                    
                    return workspaces;
                }
            }
        }
        
        // Fallback: create default empty workspaces
        warn!("Could not get workspace info from niri, using defaults");
        for i in 1..=4 {
            workspaces.push(WorkspaceInfo {
                id: i as u64,
                idx: i,
                is_active: i == 1,
                is_focused: i == 1,
                windows: Vec::new(),
            });
        }
        
        workspaces
    }
    
    fn switch_workspace(idx: u32) {
        match Command::new("niri")
            .args(&["msg", "action", "focus-workspace", &idx.to_string()])
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    warn!("Failed to switch workspace: {:?}", output);
                } else {
                    info!("Switched to workspace {}", idx);
                }
            }
            Err(e) => {
                warn!("Failed to execute niri command: {}", e);
            }
        }
    }
    
    fn focus_window(window_id: u64) {
        match Command::new("niri")
            .args(&["msg", "action", "focus-window", "--id", &window_id.to_string()])
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    warn!("Failed to focus window: {:?}", output);
                }
            }
            Err(e) => {
                warn!("Failed to execute niri command: {}", e);
            }
        }
    }
    
    fn close_window(window_id: u64) {
        match Command::new("niri")
            .args(&["msg", "action", "close-window", "--id", &window_id.to_string()])
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    warn!("Failed to close window: {:?}", output);
                }
            }
            Err(e) => {
                warn!("Failed to execute niri command: {}", e);
            }
        }
    }
    
    pub fn widget(&self) -> &Box {
        &self.container
    }
}
