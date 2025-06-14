use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, Scale, Switch, ApplicationWindow, ListBox, ListBoxRow, ToggleButton};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use anyhow::Result;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{info, warn};
use notify::{Watcher, RecursiveMode, Event, EventKind, RecommendedWatcher, Config};
use std::rc::Rc;
use std::cell::RefCell;

pub struct Sound {
    button: Button,
}

#[derive(Debug, Clone)]
struct AudioInfo {
    volume: u32,
    muted: bool,
    device_name: String,
    device_id: String,
}

#[derive(Debug, Clone)]
struct AudioDevice {
    id: String,
    name: String,
    description: String,
    is_default: bool,
}

impl Sound {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("sound");
        
        let container = Box::new(Orientation::Horizontal, 5);
        
        let icon = Image::new();
        let label = Label::new(None);
        label.add_css_class("sound-percentage");
        
        container.append(&icon);
        container.append(&label);
        button.set_child(Some(&container));
        
        // Create popover for volume control
        let popover = Popover::new();
        popover.set_parent(&button);
        
        // Handle popover show event - enable keyboard mode
        let window_weak_show = window_weak.clone();
        let active_popovers_show = active_popovers.clone();
        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!("Sound popover shown - keyboard mode set to OnDemand (active popovers: {})", 
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
                    info!("Sound popover hidden - keyboard mode set to None");
                }
            } else {
                info!("Sound popover hidden - keeping keyboard mode (active popovers: {})", count);
            }
        });
        
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(15);
        popover_box.set_margin_bottom(15);
        popover_box.set_margin_start(15);
        popover_box.set_margin_end(15);
        popover_box.set_size_request(320, -1);
        popover_box.add_css_class("sound-popover");
        
        // Volume slider
        let volume_box = Box::new(Orientation::Horizontal, 10);
        
        let volume_icon = Image::from_icon_name("audio-volume-medium-symbolic");
        volume_box.append(&volume_icon);
        
        let volume_scale = Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        volume_scale.set_hexpand(true);
        volume_scale.set_draw_value(false);
        volume_scale.add_css_class("volume-slider");
        
        let volume_label = Label::new(Some("50%"));
        volume_label.set_width_chars(4);
        volume_label.add_css_class("volume-label");
        
        volume_box.append(&volume_scale);
        volume_box.append(&volume_label);
        
        popover_box.append(&volume_box);
        
        // Mute switch
        let mute_box = Box::new(Orientation::Horizontal, 10);
        let mute_label = Label::new(Some("Mute"));
        mute_label.set_hexpand(true);
        mute_label.set_halign(gtk4::Align::Start);
        
        let mute_switch = Switch::new();
        mute_switch.set_halign(gtk4::Align::End);
        
        mute_box.append(&mute_label);
        mute_box.append(&mute_switch);
        
        popover_box.append(&mute_box);
        
        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        separator.set_margin_top(5);
        separator.set_margin_bottom(5);
        popover_box.append(&separator);
        
        // Output devices section
        let devices_label = Label::new(Some("Output Devices"));
        devices_label.set_halign(gtk4::Align::Start);
        devices_label.add_css_class("sound-device-title");
        popover_box.append(&devices_label);
        
        // Device list
        let device_list = ListBox::new();
        device_list.add_css_class("sound-device-list");
        device_list.set_selection_mode(gtk4::SelectionMode::None);
        
        // ScrolledWindow for device list (in case there are many devices)
        let device_scroll = gtk4::ScrolledWindow::new();
        device_scroll.set_policy(gtk4::PolicyType::Never, gtk4::PolicyType::Automatic);
        device_scroll.set_min_content_height(50);
        device_scroll.set_max_content_height(200);
        device_scroll.set_child(Some(&device_list));
        
        popover_box.append(&device_scroll);
        
        // Separator
        let separator2 = gtk4::Separator::new(Orientation::Horizontal);
        separator2.set_margin_top(5);
        separator2.set_margin_bottom(5);
        popover_box.append(&separator2);
        
        // Audio settings button
        let settings_button = Button::with_label("Sound Settings");
        settings_button.set_margin_top(5);
        settings_button.connect_clicked(|_| {
            Self::open_sound_settings();
        });
        popover_box.append(&settings_button);
        
        popover.set_child(Some(&popover_box));
        
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
        
        // Store references for updates
        let audio_info = Arc::new(Mutex::new(AudioInfo {
            volume: 50,
            muted: false,
            device_name: "Unknown".to_string(),
            device_id: String::new(),
        }));
        
        // Set initial state
        icon.set_from_icon_name(Some("audio-volume-medium-symbolic"));
        label.set_text("50%");
        volume_scale.set_value(50.0);
        volume_label.set_text("50%");
        mute_switch.set_active(false);
        
        // Schedule immediate update after widget is realized
        let icon_init = icon.clone();
        let label_init = label.clone();
        let volume_scale_init = volume_scale.clone();
        let volume_label_init = volume_label.clone();
        let mute_switch_init = mute_switch.clone();
        let device_list_init = device_list.clone();
        let audio_info_init = audio_info.clone();
        
        glib::idle_add_local_once(move || {
            Self::update_audio(&icon_init, &label_init, &volume_scale_init, &volume_label_init, 
                             &mute_switch_init, &device_list_init, audio_info_init);
        });
        
        // Flag to prevent feedback loops
        let volume_updating = std::rc::Rc::new(std::cell::RefCell::new(false));
        let volume_updating_clone = volume_updating.clone();
        
        // Handle volume scale changes
        let audio_info_scale = audio_info.clone();
        let volume_label_weak = volume_label.downgrade();
        let icon_weak = icon.downgrade();
        let label_weak = label.downgrade();
        let volume_updating_for_scale = volume_updating.clone();
        volume_scale.connect_value_changed(move |scale| {
            // Set flag to prevent feedback loop
            *volume_updating_for_scale.borrow_mut() = true;
            
            let volume = scale.value() as u32;
            
            // Update volume
            Self::set_volume(volume);
            
            // Update UI immediately
            if let Some(vol_label) = volume_label_weak.upgrade() {
                vol_label.set_text(&format!("{}%", volume));
            }
            
            // Update icon and label
            if let (Some(icon), Some(label)) = (icon_weak.upgrade(), label_weak.upgrade()) {
                if let Ok(mut info) = audio_info_scale.lock() {
                    info.volume = volume;
                    Self::update_icon(&icon, volume, info.muted);
                    if !info.muted {
                        label.set_text(&format!("{}%", volume));
                    }
                }
            }
            
            // Clear flag after a short delay
            let volume_updating_clear = volume_updating_for_scale.clone();
            glib::timeout_add_local_once(Duration::from_millis(200), move || {
                *volume_updating_clear.borrow_mut() = false;
            });
        });
        
        // Handle mute switch
        let audio_info_mute = audio_info.clone();
        let icon_weak_mute = icon.downgrade();
        let label_weak_mute = label.downgrade();
        let volume_scale_weak = volume_scale.downgrade();
        let mute_updating = std::rc::Rc::new(std::cell::RefCell::new(false));
        let mute_updating_for_switch = mute_updating.clone();
        mute_switch.connect_state_set(move |_switch, state| {
            // Set flag to prevent feedback loop
            *mute_updating_for_switch.borrow_mut() = true;
            
            Self::toggle_mute(state);
            
            // Update UI immediately
            if let (Some(icon), Some(label)) = (icon_weak_mute.upgrade(), label_weak_mute.upgrade()) {
                if let Ok(mut info) = audio_info_mute.lock() {
                    info.muted = state;
                    Self::update_icon(&icon, info.volume, state);
                    
                    if state {
                        label.set_text("Muted");
                    } else {
                        label.set_text(&format!("{}%", info.volume));
                    }
                    
                    // Update scale sensitivity
                    if let Some(scale) = volume_scale_weak.upgrade() {
                        scale.set_sensitive(!state);
                    }
                }
            }
            
            // Clear flag after a short delay
            let mute_updating_clear = mute_updating_for_switch.clone();
            glib::timeout_add_local_once(Duration::from_millis(200), move || {
                *mute_updating_clear.borrow_mut() = false;
            });
            
            glib::Propagation::Proceed
        });
        
        // Update handler for popover show event to refresh device list
        let device_list_for_show = device_list.clone();
        popover.connect_show(move |_| {
            Self::update_device_list(&device_list_for_show);
        });
        
        // Always use polling for primary updates (more reliable)
        let icon_weak = icon.downgrade();
        let label_weak = label.downgrade();
        let volume_scale_weak = volume_scale.downgrade();
        let volume_label_weak = volume_label.downgrade();
        let mute_switch_weak = mute_switch.downgrade();
        let device_list_weak = device_list.downgrade();
        let audio_info_clone = audio_info.clone();
        let volume_updating_for_monitor = volume_updating.clone();
        let mute_updating_for_monitor = mute_updating.clone();
        
        glib::timeout_add_local(Duration::from_millis(250), move || {
            // Only update if we're not currently updating from UI controls
            if !*volume_updating_for_monitor.borrow() && !*mute_updating_for_monitor.borrow() {
                if let (Some(icon), Some(label), Some(scale), Some(vol_label), Some(mute), Some(device_list)) = 
                    (icon_weak.upgrade(), label_weak.upgrade(), volume_scale_weak.upgrade(), 
                     volume_label_weak.upgrade(), mute_switch_weak.upgrade(), device_list_weak.upgrade()) {
                    Self::update_audio(&icon, &label, &scale, &vol_label, &mute, &device_list, audio_info_clone.clone());
                }
            }
            glib::ControlFlow::Continue
        });
        
        // Additionally try to set up file system monitoring for faster updates
        if let Ok(audio_rx) = Self::setup_audio_monitor() {
            info!("Audio monitoring initialized with file system watcher");
            
            let icon_weak2 = icon.downgrade();
            let label_weak2 = label.downgrade();
            let volume_scale_weak2 = volume_scale.downgrade();
            let volume_label_weak2 = volume_label.downgrade();
            let mute_switch_weak2 = mute_switch.downgrade();
            let device_list_weak2 = device_list.downgrade();
            let audio_info_clone2 = audio_info.clone();
            let volume_updating2 = volume_updating.clone();
            let mute_updating2 = mute_updating.clone();
            
            // Check for audio updates from file system monitor
            glib::timeout_add_local(Duration::from_millis(50), move || {
                // Check if we have any audio updates
                while let Ok(()) = audio_rx.try_recv() {
                    // Only update if we're not currently updating from UI controls
                    if !*volume_updating2.borrow() && !*mute_updating2.borrow() {
                        if let (Some(icon), Some(label), Some(scale), Some(vol_label), Some(mute), Some(device_list)) = 
                            (icon_weak2.upgrade(), label_weak2.upgrade(), volume_scale_weak2.upgrade(), 
                             volume_label_weak2.upgrade(), mute_switch_weak2.upgrade(), device_list_weak2.upgrade()) {
                            Self::update_audio(&icon, &label, &scale, &vol_label, &mute, &device_list, audio_info_clone2.clone());
                        }
                    }
                }
                glib::ControlFlow::Continue
            });
        } else {
            info!("File system monitoring not available, using polling only");
        }
        
        // Show popover on click
        button.connect_clicked(move |_| {
            popover.popup();
        });
        
        // Scroll to change volume
        let controller = gtk4::EventControllerScroll::new(gtk4::EventControllerScrollFlags::VERTICAL);
        let audio_info_scroll = audio_info.clone();
        controller.connect_scroll(move |_, _, dy| {
            if let Ok(info) = audio_info_scroll.lock() {
                let current_volume = info.volume as f64;
                let new_volume = (current_volume - dy * 5.0).clamp(0.0, 100.0) as u32;
                Self::set_volume(new_volume);
            }
            glib::Propagation::Stop
        });
        button.add_controller(controller);
        
        Ok(Self { button })
    }
    
    fn setup_audio_monitor() -> Result<mpsc::Receiver<()>> {
        let (tx, rx) = mpsc::channel();
        
        thread::spawn(move || {
            // Create channel for watcher events
            let (watch_tx, watch_rx) = mpsc::channel();
            
            // Try to find PipeWire/PulseAudio runtime directories to monitor
            let mut paths_to_watch = Vec::new();
            
            // Check for PipeWire runtime
            if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                let pipewire_path = PathBuf::from(&runtime_dir).join("pipewire-0");
                if pipewire_path.exists() {
                    paths_to_watch.push(pipewire_path);
                }
                
                // Also check for PulseAudio
                let pulse_path = PathBuf::from(&runtime_dir).join("pulse");
                if pulse_path.exists() {
                    paths_to_watch.push(pulse_path);
                }
            }
            
            // If no specific audio paths found, monitor /dev/snd for ALSA changes
            let dev_snd = PathBuf::from("/dev/snd");
            if dev_snd.exists() && paths_to_watch.is_empty() {
                paths_to_watch.push(dev_snd);
            }
            
            if paths_to_watch.is_empty() {
                warn!("No audio paths found to monitor");
                return;
            }
            
            // Create a watcher
            let mut watcher = match RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    if let Ok(event) = res {
                        let _ = watch_tx.send(event);
                    }
                },
                Config::default(),
            ) {
                Ok(w) => w,
                Err(e) => {
                    warn!("Failed to create file watcher: {}", e);
                    return;
                }
            };
            
            // Watch all audio paths
            for path in &paths_to_watch {
                if let Err(e) = watcher.watch(path, RecursiveMode::NonRecursive) {
                    warn!("Failed to watch audio path {:?}: {}", path, e);
                } else {
                    info!("Watching audio path: {:?}", path);
                }
            }
            
            // Process file change events
            while let Ok(event) = watch_rx.recv() {
                match event.kind {
                    EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_) => {
                        // Audio system state might have changed, notify the UI
                        let _ = tx.send(());
                    }
                    _ => {}
                }
            }
            
            warn!("Audio monitor thread exiting");
        });
        
        Ok(rx)
    }
    
    fn update_audio(
        icon: &Image,
        label: &Label,
        scale: &Scale,
        volume_label: &Label,
        mute_switch: &Switch,
        device_list: &ListBox,
        audio_info: Arc<Mutex<AudioInfo>>
    ) {
        if let Some(info) = Self::get_audio_info() {
            // Update stored info
            let mut should_update_ui = false;
            if let Ok(mut stored_info) = audio_info.lock() {
                // Only update UI if values actually changed
                if stored_info.volume != info.volume || stored_info.muted != info.muted || 
                   stored_info.device_name != info.device_name || stored_info.device_id != info.device_id {
                    should_update_ui = true;
                }
                *stored_info = info.clone();
            }
            
            if should_update_ui {
                // Update icon
                Self::update_icon(icon, info.volume, info.muted);
                
                // Update label
                if info.muted {
                    label.set_text("Muted");
                } else {
                    label.set_text(&format!("{}%", info.volume));
                }
                
                // Update popover controls
                scale.set_value(info.volume as f64);
                volume_label.set_text(&format!("{}%", info.volume));
                mute_switch.set_active(info.muted);
                scale.set_sensitive(!info.muted);
            }
        }
    }
    
    fn update_device_list(device_list: &ListBox) {
        // Clear existing items
        while let Some(child) = device_list.first_child() {
            device_list.remove(&child);
        }
        
        let devices = Self::get_audio_devices();
        let toggle_buttons: Rc<RefCell<Vec<ToggleButton>>> = Rc::new(RefCell::new(Vec::new()));
        
        for device in devices {
            let row = ListBoxRow::new();
            row.add_css_class("sound-device-row");
            
            let hbox = Box::new(Orientation::Horizontal, 10);
            hbox.set_margin_start(5);
            hbox.set_margin_end(5);
            hbox.set_margin_top(8);
            hbox.set_margin_bottom(8);
            
            // Toggle button for selection
            let toggle = ToggleButton::new();
            toggle.set_active(device.is_default);
            toggle.add_css_class("sound-device-toggle");
            
            // Store reference to this toggle button
            toggle_buttons.borrow_mut().push(toggle.clone());
            
            hbox.append(&toggle);
            
            // Device info
            let vbox = Box::new(Orientation::Vertical, 2);
            vbox.set_hexpand(true);
            
            let name_label = Label::new(Some(&device.name));
            name_label.set_halign(gtk4::Align::Start);
            name_label.add_css_class("sound-device-name");
            vbox.append(&name_label);
            
            if !device.description.is_empty() && device.description != device.name {
                let desc_label = Label::new(Some(&device.description));
                desc_label.set_halign(gtk4::Align::Start);
                desc_label.add_css_class("sound-device-description");
                desc_label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
                vbox.append(&desc_label);
            }
            
            hbox.append(&vbox);
            
            // Handle device selection
            let device_id = device.id.clone();
            let toggle_buttons_clone = toggle_buttons.clone();
            let current_toggle = toggle.clone();
            toggle.connect_toggled(move |toggle| {
                if toggle.is_active() {
                    // Deactivate all other toggles
                    for other_toggle in toggle_buttons_clone.borrow().iter() {
                        if other_toggle != &current_toggle {
                            other_toggle.set_active(false);
                        }
                    }
                    Self::set_default_device(&device_id);
                }
            });
            
            row.set_child(Some(&hbox));
            device_list.append(&row);
        }
    }
    
    fn update_icon(icon: &Image, volume: u32, muted: bool) {
        let icon_name = if muted {
            "audio-volume-muted-symbolic"
        } else {
            match volume {
                0 => "audio-volume-muted-symbolic",
                1..=33 => "audio-volume-low-symbolic",
                34..=66 => "audio-volume-medium-symbolic",
                _ => "audio-volume-high-symbolic",
            }
        };
        icon.set_from_icon_name(Some(icon_name));
    }
    
    fn get_audio_devices() -> Vec<AudioDevice> {
        let mut devices = Vec::new();
        
        // Try wpctl first (WirePlumber/PipeWire)
        if let Ok(output) = Command::new("wpctl")
            .args(&["status"])
            .output()
        {
            if output.status.success() {
                let status_str = String::from_utf8_lossy(&output.stdout);
                let mut in_sinks_section = false;
                let mut current_default_id = String::new();
                
                for line in status_str.lines() {
                    if line.contains("Sinks:") {
                        in_sinks_section = true;
                        continue;
                    }
                    
                    if in_sinks_section && (line.starts_with(" ├─") || line.starts_with(" └─") || line.starts_with(" │")) {
                        // Parse format: " │  *   60. Family 17h/19h/1ah HD Audio Controller Analog Stereo [vol: 0.17]"
                        let is_default = line.contains("*");
                        
                        // Extract ID and name
                        if let Some(dot_pos) = line.find('.') {
                            // Get ID (number before the dot)
                            let prefix = &line[..dot_pos];
                            let id = prefix.split_whitespace().last().unwrap_or("").to_string();
                            
                            // Get name (after the dot)
                            let after_dot = &line[dot_pos+1..].trim();
                            let name = if let Some(bracket_pos) = after_dot.find('[') {
                                after_dot[..bracket_pos].trim().to_string()
                            } else {
                                after_dot.to_string()
                            };
                            
                            if is_default {
                                current_default_id = id.clone();
                            }
                            
                            devices.push(AudioDevice {
                                id: id.clone(),
                                name: name.clone(),
                                description: name,
                                is_default,
                            });
                        }
                    }
                    
                    if in_sinks_section && !line.starts_with(" ") && !line.trim().is_empty() {
                        break;
                    }
                }
                
                return devices;
            }
        }
        
        // Try pactl as fallback (PulseAudio/older PipeWire)
        if let Ok(output) = Command::new("pactl")
            .args(&["list", "sinks", "short"])
            .output()
        {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout);
                
                // Get default sink
                let default_sink = if let Ok(default_output) = Command::new("pactl")
                    .args(&["get-default-sink"])
                    .output()
                {
                    String::from_utf8_lossy(&default_output.stdout).trim().to_string()
                } else {
                    String::new()
                };
                
                // Get detailed sink info
                if let Ok(detailed_output) = Command::new("pactl")
                    .args(&["list", "sinks"])
                    .output()
                {
                    let detailed_str = String::from_utf8_lossy(&detailed_output.stdout);
                    let mut current_sink_name = String::new();
                    let mut current_description = String::new();
                    
                    for line in detailed_str.lines() {
                        if line.starts_with("Sink #") {
                            // If we have a previous sink, add it
                            if !current_sink_name.is_empty() {
                                devices.push(AudioDevice {
                                    id: current_sink_name.clone(),
                                    name: current_description.clone(),
                                    description: current_description.clone(),
                                    is_default: current_sink_name == default_sink,
                                });
                            }
                            current_sink_name.clear();
                            current_description.clear();
                        } else if line.trim().starts_with("Name:") {
                            current_sink_name = line.split(':').nth(1).unwrap_or("").trim().to_string();
                        } else if line.trim().starts_with("Description:") {
                            current_description = line.split(':').nth(1).unwrap_or("").trim().to_string();
                        }
                    }
                    
                    // Add the last sink
                    if !current_sink_name.is_empty() {
                        devices.push(AudioDevice {
                            id: current_sink_name.clone(),
                            name: current_description.clone(),
                            description: current_description,
                            is_default: current_sink_name == default_sink,
                        });
                    }
                }
                
                return devices;
            }
        }
        
        // If no devices found, return a default entry
        if devices.is_empty() {
            devices.push(AudioDevice {
                id: "default".to_string(),
                name: "Default Audio Device".to_string(),
                description: "System default".to_string(),
                is_default: true,
            });
        }
        
        devices
    }
    
    fn get_audio_info() -> Option<AudioInfo> {
        // Try wpctl first (WirePlumber/PipeWire) - using @DEFAULT_AUDIO_SINK@
        if let Ok(volume_output) = Command::new("wpctl")
            .args(&["get-volume", "@DEFAULT_AUDIO_SINK@"])
            .output()
        {
            if volume_output.status.success() && !volume_output.stdout.is_empty() {
                let volume_str = String::from_utf8_lossy(&volume_output.stdout);
                
                // Parse volume (format: "Volume: 0.50 [MUTED]" or "Volume: 0.50")
                let muted = volume_str.contains("[MUTED]");
                let volume = if let Some(vol_str) = volume_str.split(':').nth(1) {
                    if let Some(vol_val) = vol_str.split_whitespace().next() {
                        (vol_val.parse::<f32>().unwrap_or(0.0) * 100.0) as u32
                    } else {
                        0
                    }
                } else {
                    0
                };
                
                // Get device name and ID from wpctl status
                let (device_name, device_id) = if let Ok(status_output) = Command::new("wpctl")
                    .arg("status")
                    .output()
                {
                    let status_str = String::from_utf8_lossy(&status_output.stdout);
                    let mut in_sinks_section = false;
                    let mut found_device_name = "Unknown".to_string();
                    let mut found_device_id = String::new();
                    
                    for line in status_str.lines() {
                        if line.contains("Sinks:") {
                            in_sinks_section = true;
                            continue;
                        }
                        if in_sinks_section && (line.starts_with(" ├─") || line.starts_with(" └─") || line.starts_with(" │")) {
                            if line.contains("*") {
                                // Parse format: " │  *   60. Family 17h/19h/1ah HD Audio Controller Analog Stereo [vol: 0.17]"
                                // Find the part after the number and dot
                                if let Some(dot_pos) = line.find('.') {
                                    // Get ID
                                    let prefix = &line[..dot_pos];
                                    found_device_id = prefix.split_whitespace().last().unwrap_or("").to_string();
                                    
                                    let after_dot = &line[dot_pos+1..].trim();
                                    // Find where [vol: starts
                                    if let Some(vol_pos) = after_dot.find("[vol:") {
                                        found_device_name = after_dot[..vol_pos].trim().to_string();
                                    } else if let Some(bracket_pos) = after_dot.find('[') {
                                        found_device_name = after_dot[..bracket_pos].trim().to_string();
                                    } else {
                                        found_device_name = after_dot.to_string();
                                    }
                                    break;
                                }
                            }
                        }
                        if in_sinks_section && !line.starts_with(" ") && !line.trim().is_empty() {
                            break;
                        }
                    }
                    
                    (found_device_name, found_device_id)
                } else {
                    ("Unknown".to_string(), String::new())
                };
                
                return Some(AudioInfo {
                    volume,
                    muted,
                    device_name,
                    device_id,
                });
            }
        }
        
        // Try pactl as fallback (PulseAudio/older PipeWire)
        if let Ok(output) = Command::new("pactl")
            .args(&["get-sink-volume", "@DEFAULT_SINK@"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            let volume = output_str
                .split('/')
                .nth(1)
                .and_then(|s| s.trim().trim_end_matches('%').parse::<u32>().ok())
                .unwrap_or(0);
            
            let muted = if let Ok(mute_output) = Command::new("pactl")
                .args(&["get-sink-mute", "@DEFAULT_SINK@"])
                .output()
            {
                String::from_utf8_lossy(&mute_output.stdout).contains("yes")
            } else {
                false
            };
            
            // Get device name and ID
            let (device_name, device_id) = if let Ok(device_output) = Command::new("pactl")
                .args(&["get-default-sink"])
                .output()
            {
                let sink_name = String::from_utf8_lossy(&device_output.stdout).trim().to_string();
                
                // Get human-readable name
                if let Ok(desc_output) = Command::new("pactl")
                    .args(&["list", "sinks"])
                    .output()
                {
                    let desc_str = String::from_utf8_lossy(&desc_output.stdout);
                    let mut found_sink = false;
                    for desc_line in desc_str.lines() {
                        if desc_line.contains(&format!("Name: {}", sink_name)) {
                            found_sink = true;
                        }
                        if found_sink && desc_line.trim().starts_with("Description:") {
                            let description = desc_line
                                .split(':')
                                .nth(1)
                                .unwrap_or("Unknown")
                                .trim()
                                .to_string();
                            return Some(AudioInfo {
                                volume,
                                muted,
                                device_name: description,
                                device_id: sink_name.clone(),
                            });
                        }
                    }
                }
                (sink_name.clone(), sink_name)
            } else {
                ("Unknown".to_string(), String::new())
            };
            
            return Some(AudioInfo {
                volume,
                muted,
                device_name,
                device_id,
            });
        }
        
        // Fallback to amixer (ALSA)
        if let Ok(output) = Command::new("amixer")
            .args(&["get", "Master"])
            .output()
        {
            let output_str = String::from_utf8_lossy(&output.stdout);
            for line in output_str.lines() {
                if line.contains("Playback") && line.contains('%') {
                    let volume = line
                        .split('[')
                        .nth(1)
                        .and_then(|s| s.split('%').next())
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(0);
                    
                    let muted = line.contains("[off]");
                    
                    return Some(AudioInfo {
                        volume,
                        muted,
                        device_name: "Master".to_string(),
                        device_id: "master".to_string(),
                    });
                }
            }
        }
        
        None
    }
    
    fn set_default_device(device_id: &str) {
        // Try wpctl first (WirePlumber/PipeWire)
        let _ = Command::new("wpctl")
            .args(&["set-default", device_id])
            .spawn();
        
        // Fallback to pactl (PulseAudio)
        let _ = Command::new("pactl")
            .args(&["set-default-sink", device_id])
            .spawn();
    }
    
    fn set_volume(volume: u32) {
        // Try wpctl first (matches your niri config)
        let volume_float = (volume as f32 / 100.0).to_string();
        let _ = Command::new("wpctl")
            .args(&["set-volume", "@DEFAULT_AUDIO_SINK@", &volume_float])
            .spawn();
        
        // Fallback to pactl
        let _ = Command::new("pactl")
            .args(&["set-sink-volume", "@DEFAULT_SINK@", &format!("{}%", volume)])
            .spawn();
        
        // Fallback to amixer
        let _ = Command::new("amixer")
            .args(&["set", "Master", &format!("{}%", volume)])
            .spawn();
    }
    
    fn toggle_mute(mute: bool) {
        // Try wpctl first (matches your niri config)
        let _ = Command::new("wpctl")
            .args(&["set-mute", "@DEFAULT_AUDIO_SINK@", if mute { "1" } else { "0" }])
            .spawn();
        
        // Fallback to pactl
        let _ = Command::new("pactl")
            .args(&["set-sink-mute", "@DEFAULT_SINK@", if mute { "1" } else { "0" }])
            .spawn();
        
        // Fallback to amixer
        let _ = Command::new("amixer")
            .args(&["set", "Master", if mute { "mute" } else { "unmute" }])
            .spawn();
    }
    
    fn open_sound_settings() {
        // Try different sound settings commands
        let commands = vec![
            ("gnome-control-center", vec!["sound"]),
            ("pavucontrol", vec![]),
            ("pavucontrol-qt", vec![]),
            ("alsamixer", vec![]),
        ];
        
        for (cmd, args) in commands {
            if Command::new(cmd).args(&args).spawn().is_ok() {
                return;
            }
        }
        
        warn!("Could not find sound settings application");
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}