use anyhow::Result;
use gtk4::prelude::*;
use gtk4::{Box, Button, Image, Label, ListBox, ListBoxRow, Orientation, Popover};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::process::{Child, Command};
use std::rc::Rc;
use tracing::{debug, error, info, warn};

use crate::niri_ipc::{self, NiriEvent, WindowInfo, WorkspaceInfo};

pub struct Workspaces {
    container: Box,
    // Shared state for workspaces and windows
    state: Rc<RefCell<WorkspacesState>>,
    // Keep the child process alive
    _event_stream_child: Option<Child>,
    // Keep the GLib source ID for cleanup
    _event_source_id: Option<gtk4::glib::SourceId>,
}

#[derive(Default)]
struct WorkspacesState {
    workspaces: Vec<WorkspaceInfo>,
    windows: Vec<WindowInfo>,
    workspace_id_to_button: HashMap<u64, gtk4::glib::WeakRef<Button>>,
}

impl Workspaces {
    pub fn new() -> Result<Self> {
        let container = Box::new(Orientation::Horizontal, 5);
        container.add_css_class("workspaces");
        
        let state = Rc::new(RefCell::new(WorkspacesState::default()));
        
        // Get initial workspace info
        let (initial_workspaces, initial_windows) = Self::get_initial_state()?;
        
        // Update state with initial data
        {
            let mut state_mut = state.borrow_mut();
            state_mut.workspaces = initial_workspaces;
            state_mut.windows = initial_windows;
        }
        
        // Create workspace buttons
        Self::update_workspace_ui(&container, state.clone());
        
        // Set up event stream
        let (event_source_id, event_stream_child) = Self::setup_event_stream(container.clone(), state.clone())?;
        
        Ok(Self { 
            container, 
            state,
            _event_stream_child: Some(event_stream_child),
            _event_source_id: Some(event_source_id),
        })
    }

    fn create_workspace_button(workspace: &WorkspaceInfo, state: Rc<RefCell<WorkspacesState>>) -> Button {
        let button = Button::with_label(&workspace.id.to_string());
        button.add_css_class("workspace");

        if workspace.is_focused {
            button.add_css_class("active");
        }

        // Store workspace info in widget name (safer than set_data)
        button.set_widget_name(&format!("{}:{}", workspace.id, workspace.idx));
        
        // Store button reference in state
        state.borrow_mut().workspace_id_to_button.insert(workspace.id, button.downgrade());

        info!(
            "Creating button for workspace {} with id {}",
            workspace.idx, workspace.id
        );

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

    fn show_window_picker(
        button: &Button,
        workspace_id: u64,
        workspace_idx: u32,
        _x: f64,
        _y: f64,
    ) {
        info!(
            "Showing window picker for workspace {} (id: {})",
            workspace_idx, workspace_id
        );

        // Create popover on demand
        let popover = Popover::new();
        popover.set_parent(button);
        popover.add_css_class("workspace-popover");
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

        // Get current windows for this workspace using niri msg windows
        let windows = Self::get_windows_for_workspace(workspace_id);
        info!("Found {} windows for workspace {}", windows.len(), workspace_id);

        if windows.is_empty() {
            let empty_label = Label::new(Some("No windows"));
            empty_label.add_css_class("workspace-empty-label");
            empty_label.set_margin_top(20);
            empty_label.set_margin_bottom(20);
            popover_box.append(&empty_label);
        } else {
            let window_list = ListBox::new();
            window_list.add_css_class("workspace-window-list");
            window_list.set_selection_mode(gtk4::SelectionMode::Single);

            for window in &windows {
                info!("Adding window: {} (id: {})", window.title, window.id);
                let row = Self::create_window_row(window, popover.downgrade());
                window_list.append(&row);
            }
            popover_box.append(&window_list);
        }

        popover.set_child(Some(&popover_box));

        // Clean up popover when closed
        popover.connect_closed(|popover| {
            popover.unparent();
        });

        popover.popup();
    }

    fn create_window_row(
        window: &WindowInfo,
        popover_weak: gtk4::glib::WeakRef<Popover>,
    ) -> ListBoxRow {
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

        // Close button
        let close_button = Button::from_icon_name("window-close-symbolic");
        close_button.add_css_class("workspace-window-close");
        close_button.set_tooltip_text(Some("Close this window"));

        let window_id_close = window.id;
        let popover_weak_close = popover_weak.clone();
        close_button.connect_clicked(move |_| {
            Self::close_window(window_id_close);
            if let Some(popover) = popover_weak_close.upgrade() {
                popover.popdown();
            }
        });
        hbox.append(&close_button);

        row.set_child(Some(&hbox));
        
        // Add click handler to the row itself
        let window_id = window.id;
        let popover_weak_focus = popover_weak.clone();
        row.connect_activate(move |_| {
            Self::focus_window(window_id);
            if let Some(popover) = popover_weak_focus.upgrade() {
                popover.popdown();
            }
        });
        
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

    fn update_workspace_ui(container: &Box, state: Rc<RefCell<WorkspacesState>>) {
        // Clone and sort workspaces by ID
        let mut workspaces = state.borrow().workspaces.clone();
        workspaces.sort_by_key(|w| w.id);

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
                    
                    // Update button reference in state
                    state.borrow_mut().workspace_id_to_button.insert(workspace.id, button.downgrade());
                }
            } else {
                // Add new button if needed
                let button = Self::create_workspace_button(workspace, state.clone());
                container.append(&button);
            }
        }

        // Remove extra buttons
        while current_buttons.len() > workspaces.len() {
            if let Some(button) = current_buttons.pop() {
                // Remove from state if it exists
                if let Some(button) = button.downcast_ref::<Button>() {
                    let name = button.widget_name();
                    if let Some(id_str) = name.split(':').next() {
                        if let Ok(id) = id_str.parse::<u64>() {
                            state.borrow_mut().workspace_id_to_button.remove(&id);
                        }
                    }
                }
                container.remove(&button);
            }
        }
    }

    fn get_initial_state() -> Result<(Vec<WorkspaceInfo>, Vec<WindowInfo>)> {
        let mut workspaces = Vec::new();
        let mut windows = Vec::new();

        // Get workspace information first
        if let Ok(output) = Command::new("niri")
            .args(&["msg", "-j", "workspaces"])
            .output()
        {
            if let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(workspaces_array) = json.as_array() {
                    // Create workspace info with proper IDs
                    for (idx, workspace_json) in workspaces_array.iter().enumerate() {
                        // Extract the actual workspace ID from the Niri JSON
                        let workspace_id = match workspace_json["id"].as_u64() {
                            Some(id) => id,
                            None => {
                                warn!("Missing id for workspace at index {}, using fallback", idx);
                                (idx + 1) as u64 // Fallback to index+1 if id is missing
                            }
                        };
                        
                        // Get the idx field from the JSON (display number)
                        let idx_from_json = workspace_json["idx"].as_u64().unwrap_or_else(|| {
                            warn!("Missing idx for workspace ID {}, using index+1", workspace_id);
                            (idx + 1) as u64
                        });
                        
                        workspaces.push(WorkspaceInfo {
                            id: workspace_id,
                            idx: idx_from_json as u32,
                            name: workspace_json["name"].as_str().map(ToString::to_string),
                            output: workspace_json["output"]
                                .as_str()
                                .unwrap_or("eDP-1")
                                .to_string(),
                            is_urgent: workspace_json["is_urgent"].as_bool().unwrap_or(false),
                            is_active: workspace_json["is_active"].as_bool().unwrap_or(false),
                            is_focused: workspace_json["is_focused"].as_bool().unwrap_or(false),
                            active_window_id: workspace_json["active_window_id"].as_u64(),
                        });
                    }
                    
                    // Sort workspaces by ID
                    workspaces.sort_by_key(|w| w.id);
                }
            }
        } else {
            // Fallback: create default empty workspaces (1-4)
            warn!("Could not get workspace info from niri, using defaults");
            for i in 1..=4 {
                let workspace_id = i as u64;
                let display_idx = i;
                workspaces.push(WorkspaceInfo {
                    id: workspace_id,
                    idx: display_idx,
                    name: None,
                    output: "eDP-1".to_string(),
                    is_urgent: false,
                    is_active: i == 1,
                    is_focused: i == 1,
                    active_window_id: None,
                });
            }
            // Sort workspaces by ID (in fallback case this is already sorted,
            // but we include it for consistency)
            workspaces.sort_by_key(|w| w.id);
        }

        // Get all windows
        if let Ok(output) = Command::new("niri").args(&["msg", "-j", "windows"]).output() {
            if let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(windows_array) = json.as_array() {
                    for window_json in windows_array {
                        if let (Some(id), Some(title), Some(workspace_id)) = (
                            window_json["id"].as_u64(),
                            window_json["title"].as_str(),
                            window_json["workspace_id"].as_u64(),
                        ) {
                            windows.push(WindowInfo {
                                id,
                                title: title.to_string(),
                                app_id: window_json["app_id"].as_str().map(ToString::to_string),
                                pid: window_json["pid"].as_u64().unwrap_or(0),
                                workspace_id,
                                is_focused: window_json["is_focused"].as_bool().unwrap_or(false),
                                is_floating: window_json["is_floating"].as_bool().unwrap_or(false),
                                is_urgent: window_json["is_urgent"].as_bool().unwrap_or(false),
                            });
                        }
                    }
                }
            }
        }

        Ok((workspaces, windows))
    }

    fn get_windows_for_workspace(workspace_id: u64) -> Vec<WindowInfo> {
        let mut windows = Vec::new();
        
        if let Ok(output) = Command::new("niri").args(&["msg", "-j", "windows"]).output() {
            if let Ok(json) = serde_json::from_slice::<Value>(&output.stdout) {
                if let Some(windows_array) = json.as_array() {
                    for window_json in windows_array {
                        if let (Some(id), Some(title), Some(wid)) = (
                            window_json["id"].as_u64(),
                            window_json["title"].as_str(),
                            window_json["workspace_id"].as_u64(),
                        ) {
                            // Only include windows for the requested workspace
                            if wid == workspace_id {
                                windows.push(WindowInfo {
                                    id,
                                    title: title.to_string(),
                                    app_id: window_json["app_id"].as_str().map(ToString::to_string),
                                    pid: window_json["pid"].as_u64().unwrap_or(0),
                                    workspace_id,
                                    is_focused: window_json["is_focused"].as_bool().unwrap_or(false),
                                    is_floating: window_json["is_floating"].as_bool().unwrap_or(false),
                                    is_urgent: window_json["is_urgent"].as_bool().unwrap_or(false),
                                });
                            }
                        }
                    }
                }
            }
        }
        
        windows
    }
    
    fn setup_event_stream(
        container: Box, 
        state: Rc<RefCell<WorkspacesState>>
    ) -> Result<(gtk4::glib::SourceId, Child)> {
        // Set up event stream
        let container_weak = container.downgrade();
        let callback = move |event: NiriEvent| {
            match event {
                NiriEvent::WorkspacesChanged { mut workspaces } => {
                    debug!("Workspaces changed: {} workspaces", workspaces.len());
                    // Update workspace state
                    state.borrow_mut().workspaces = workspaces;
                    
                    // Update UI
                    if let Some(container) = container_weak.upgrade() {
                        Self::update_workspace_ui(&container, state.clone());
                    }
                },
                NiriEvent::WindowsChanged { windows } => {
                    debug!("Windows changed: {} windows", windows.len());
                    // Update window state
                    state.borrow_mut().windows = windows;
                },
                NiriEvent::WorkspaceActivated { id, focused } => {
                    debug!("Workspace activated: {} (focused: {})", id, focused);
                    
                    // Update active state in workspace info
                    {
                        let mut state_mut = state.borrow_mut();
                        for workspace in &mut state_mut.workspaces {
                            workspace.is_active = workspace.id == id;
                            if workspace.id == id {
                                workspace.is_focused = focused;
                            } else if focused {
                                workspace.is_focused = false;
                            }
                        }
                        
                        // Update button active state directly without full UI refresh
                        if focused {
                            // If this workspace is focused, update button states
                            for (ws_id, button_weak) in &state_mut.workspace_id_to_button {
                                if let Some(button) = button_weak.upgrade() {
                                    if *ws_id == id {
                                        button.add_css_class("active");
                                    } else {
                                        button.remove_css_class("active");
                                    }
                                }
                            }
                        }
                    }
                },
                NiriEvent::WindowFocusChanged { id: _ } => {
                    // We don't need to handle this directly for the workspaces widget
                    // since we get workspace activation events
                },
                _ => {}
            }
        };
        
        niri_ipc::attach_event_stream(callback)
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
            .args(&[
                "msg",
                "action",
                "focus-window",
                "--id",
                &window_id.to_string(),
            ])
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
            .args(&[
                "msg",
                "action",
                "close-window",
                "--id",
                &window_id.to_string(),
            ])
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