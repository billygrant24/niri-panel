use anyhow::{anyhow, Result};
use gtk4::glib;
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use tracing::{debug, error, info, warn};

#[derive(Debug, Clone)]
pub enum NiriEvent {
    WorkspacesChanged {
        workspaces: Vec<WorkspaceInfo>,
    },
    WindowsChanged {
        windows: Vec<WindowInfo>,
    },
    WorkspaceActivated {
        id: u64,
        focused: bool,
    },
    WindowFocusChanged {
        id: u64,
    },
    KeyboardLayoutsChanged {
        names: Vec<String>,
        current_idx: usize,
    },
    OverviewOpenedOrClosed {
        is_open: bool,
    },
    Unknown(Value),
}

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: u64,
    pub idx: u32,
    pub name: Option<String>,
    pub output: String,
    pub is_urgent: bool,
    pub is_active: bool,
    pub is_focused: bool,
    pub active_window_id: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u64,
    pub title: String,
    pub app_id: Option<String>,
    pub pid: u64,
    pub workspace_id: u64,
    pub is_focused: bool,
    pub is_floating: bool,
    pub is_urgent: bool,
}

/// Start the event stream and return a channel to receive events
pub fn start_event_stream() -> Result<(mpsc::Receiver<NiriEvent>, Child)> {
    // Create a channel for sending events back to the main thread
    let (tx, rx) = mpsc::channel();

    // Start the event-stream command
    let mut child = Command::new("niri")
        .args(["msg", "-j", "event-stream"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return Err(anyhow!("Failed to get stdout")),
    };
    
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => return Err(anyhow!("Failed to get stderr")),
    };

    // Clone the sender for the stderr handler
    let stderr_tx = tx.clone();

    // Handle stderr in a separate thread
    thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    warn!("Niri event stream stderr: {}", line);
                }
                Err(e) => {
                    error!("Error reading from niri event stream stderr: {}", e);
                    break;
                }
            }
        }
    });

    // Handle stdout in a separate thread
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(line) => {
                    debug!("Raw niri event: {}", line);
                    
                    // Parse the JSON event
                    match serde_json::from_str::<Value>(&line) {
                        Ok(json) => {
                            // Convert the JSON value to an event
                            if let Some(event) = parse_event(json) {
                                if let Err(e) = tx.send(event) {
                                    error!("Failed to send event to channel: {}", e);
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse JSON from event stream: {}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("Error reading from niri event stream: {}", e);
                    break;
                }
            }
        }
        info!("Niri event stream thread exited");
    });

    Ok((rx, child))
}

/// Parse a JSON value into a NiriEvent
fn parse_event(json: Value) -> Option<NiriEvent> {
    // The event stream contains objects with a single key
    // The key is the event type, and the value is the event data
    if let Some((event_type, event_data)) = json.as_object()?.iter().next() {
        match event_type.as_str() {
            "WorkspacesChanged" => parse_workspaces_changed(event_data),
            "WindowsChanged" => parse_windows_changed(event_data),
            "WorkspaceActivated" => parse_workspace_activated(event_data),
            "WindowFocusChanged" => parse_window_focus_changed(event_data),
            "KeyboardLayoutsChanged" => parse_keyboard_layouts_changed(event_data),
            "OverviewOpenedOrClosed" => parse_overview_opened_or_closed(event_data),
            _ => {
                warn!("Unknown event type: {}", event_type);
                Some(NiriEvent::Unknown(json.clone()))
            }
        }
    } else {
        warn!("Unexpected event format: {:?}", json);
        None
    }
}

fn parse_workspaces_changed(data: &Value) -> Option<NiriEvent> {
    let workspaces = data.get("workspaces")?.as_array()?;
    let mut parsed_workspaces = Vec::with_capacity(workspaces.len());

    for workspace in workspaces {
        parsed_workspaces.push(WorkspaceInfo {
            id: workspace.get("id")?.as_u64()?,
            idx: workspace.get("idx")?.as_u64()? as u32,
            name: workspace.get("name").and_then(|n| n.as_str()).map(String::from),
            output: workspace.get("output")?.as_str()?.to_string(),
            is_urgent: workspace.get("is_urgent")?.as_bool()?,
            is_active: workspace.get("is_active")?.as_bool()?,
            is_focused: workspace.get("is_focused")?.as_bool()?,
            active_window_id: workspace.get("active_window_id").and_then(|w| w.as_u64()),
        });
    }

    Some(NiriEvent::WorkspacesChanged {
        workspaces: parsed_workspaces,
    })
}

fn parse_windows_changed(data: &Value) -> Option<NiriEvent> {
    let windows = data.get("windows")?.as_array()?;
    let mut parsed_windows = Vec::with_capacity(windows.len());

    for window in windows {
        parsed_windows.push(WindowInfo {
            id: window.get("id")?.as_u64()?,
            title: window.get("title")?.as_str()?.to_string(),
            app_id: window
                .get("app_id")
                .and_then(|a| a.as_str())
                .map(String::from),
            pid: window.get("pid")?.as_u64()?,
            workspace_id: window.get("workspace_id")?.as_u64()?,
            is_focused: window.get("is_focused")?.as_bool()?,
            is_floating: window.get("is_floating")?.as_bool()?,
            is_urgent: window.get("is_urgent")?.as_bool()?,
        });
    }

    Some(NiriEvent::WindowsChanged {
        windows: parsed_windows,
    })
}

fn parse_workspace_activated(data: &Value) -> Option<NiriEvent> {
    Some(NiriEvent::WorkspaceActivated {
        id: data.get("id")?.as_u64()?,
        focused: data.get("focused")?.as_bool()?,
    })
}

fn parse_window_focus_changed(data: &Value) -> Option<NiriEvent> {
    Some(NiriEvent::WindowFocusChanged {
        id: data.get("id")?.as_u64()?,
    })
}

fn parse_keyboard_layouts_changed(data: &Value) -> Option<NiriEvent> {
    let keyboard_layouts = data.get("keyboard_layouts")?;
    let names = keyboard_layouts
        .get("names")?
        .as_array()?
        .iter()
        .filter_map(|n| n.as_str().map(String::from))
        .collect();
    
    Some(NiriEvent::KeyboardLayoutsChanged {
        names,
        current_idx: keyboard_layouts.get("current_idx")?.as_u64()? as usize,
    })
}

fn parse_overview_opened_or_closed(data: &Value) -> Option<NiriEvent> {
    Some(NiriEvent::OverviewOpenedOrClosed {
        is_open: data.get("is_open")?.as_bool()?,
    })
}

/// Attach an event stream to the GLib main context and provide a callback
pub fn attach_event_stream(
    callback: impl Fn(NiriEvent) + 'static,
) -> Result<(glib::SourceId, Child)> {
    let (rx, child) = start_event_stream()?;
    
    // Create a channel to forward events to the GLib main context
    let (sender, receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);
    
    // Spawn a thread to receive events and forward them to the GLib main context
    thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if sender.send(event).is_err() {
                break;
            }
        }
    });
    
    // Attach the receiver to the GLib main context
    let source_id = receiver.attach(None, move |event| {
        callback(event);
        glib::ControlFlow::Continue
    });
    
    Ok((source_id, child))
}