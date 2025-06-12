use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, Scale, Switch};
use glib::timeout_add_local;
use anyhow::Result;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{info, warn};

pub struct Sound {
    button: Button,
}

#[derive(Debug, Clone)]
struct AudioInfo {
    volume: u32,
    muted: bool,
    device_name: String,
}

impl Sound {
    pub fn new() -> Result<Self> {
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
        
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(15);
        popover_box.set_margin_bottom(15);
        popover_box.set_margin_start(15);
        popover_box.set_margin_end(15);
        popover_box.set_size_request(280, -1);
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
        
        // Device info
        let device_label = Label::new(Some("Output Device"));
        device_label.set_halign(gtk4::Align::Start);
        device_label.add_css_class("sound-device-title");
        popover_box.append(&device_label);
        
        let device_name_label = Label::new(Some("Built-in Audio"));
        device_name_label.set_halign(gtk4::Align::Start);
        device_name_label.add_css_class("sound-device-name");
        popover_box.append(&device_name_label);
        
        // Audio settings button
        let settings_button = Button::with_label("Sound Settings");
        settings_button.set_margin_top(10);
        settings_button.connect_clicked(|_| {
            Self::open_sound_settings();
        });
        popover_box.append(&settings_button);
        
        popover.set_child(Some(&popover_box));
        
        // Store references for updates
        let audio_info = Arc::new(Mutex::new(AudioInfo {
            volume: 50,
            muted: false,
            device_name: "Unknown".to_string(),
        }));
        
        // Set initial state
        icon.set_from_icon_name(Some("audio-volume-medium-symbolic"));
        label.set_text("50%");
        volume_scale.set_value(50.0);
        volume_label.set_text("50%");
        mute_switch.set_active(false);
        device_name_label.set_text("Unknown");
        
        // Schedule immediate update after widget is realized
        let icon_init = icon.clone();
        let label_init = label.clone();
        let volume_scale_init = volume_scale.clone();
        let volume_label_init = volume_label.clone();
        let mute_switch_init = mute_switch.clone();
        let device_name_label_init = device_name_label.clone();
        let audio_info_init = audio_info.clone();
        
        glib::idle_add_local_once(move || {
            Self::update_audio(&icon_init, &label_init, &volume_scale_init, &volume_label_init, 
                             &mute_switch_init, &device_name_label_init, audio_info_init);
        });
        
        // Handle volume scale changes
        let audio_info_scale = audio_info.clone();
        let volume_label_weak = volume_label.downgrade();
        let icon_weak = icon.downgrade();
        let label_weak = label.downgrade();
        let mute_switch_weak = mute_switch.downgrade();
        let device_name_label_weak = device_name_label.downgrade();
        volume_scale.connect_value_changed(move |scale| {
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
            
            // Schedule an update after a short delay to catch any system changes
            let icon_weak2 = icon_weak.clone();
            let label_weak2 = label_weak.clone();
            let scale_weak = scale.downgrade();
            let volume_label_weak2 = volume_label_weak.clone();
            let mute_switch_weak2 = mute_switch_weak.clone();
            let device_name_label_weak2 = device_name_label_weak.clone();
            let audio_info_clone = audio_info_scale.clone();
            
            glib::timeout_add_local_once(Duration::from_millis(100), move || {
                if let (Some(icon), Some(label), Some(scale), Some(vol_label), Some(mute), Some(device)) = 
                    (icon_weak2.upgrade(), label_weak2.upgrade(), scale_weak.upgrade(), 
                     volume_label_weak2.upgrade(), mute_switch_weak2.upgrade(), device_name_label_weak2.upgrade()) {
                    Self::update_audio(&icon, &label, &scale, &vol_label, &mute, &device, audio_info_clone);
                }
            });
        });
        
        // Handle mute switch
        let audio_info_mute = audio_info.clone();
        let icon_weak_mute = icon.downgrade();
        let label_weak_mute = label.downgrade();
        let volume_scale_weak = volume_scale.downgrade();
        let volume_label_weak_mute = volume_label.downgrade();
        let device_name_label_weak_mute = device_name_label.downgrade();
        mute_switch.connect_state_set(move |switch, state| {
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
            
            // Schedule an update after a short delay
            let icon_weak2 = icon_weak_mute.clone();
            let label_weak2 = label_weak_mute.clone();
            let scale_weak2 = volume_scale_weak.clone();
            let volume_label_weak2 = volume_label_weak_mute.clone();
            let mute_switch_weak2 = switch.downgrade();
            let device_name_label_weak2 = device_name_label_weak_mute.clone();
            let audio_info_clone = audio_info_mute.clone();
            
            glib::timeout_add_local_once(Duration::from_millis(100), move || {
                if let (Some(icon), Some(label), Some(scale), Some(vol_label), Some(mute), Some(device)) = 
                    (icon_weak2.upgrade(), label_weak2.upgrade(), scale_weak2.upgrade(), 
                     volume_label_weak2.upgrade(), mute_switch_weak2.upgrade(), device_name_label_weak2.upgrade()) {
                    Self::update_audio(&icon, &label, &scale, &vol_label, &mute, &device, audio_info_clone);
                }
            });
            
            glib::Propagation::Proceed
        });
        
        // Update every 500ms for more responsive feedback
        let icon_weak = icon.downgrade();
        let label_weak = label.downgrade();
        let volume_scale_weak = volume_scale.downgrade();
        let volume_label_weak = volume_label.downgrade();
        let mute_switch_weak = mute_switch.downgrade();
        let device_name_label_weak = device_name_label.downgrade();
        let audio_info_clone = audio_info.clone();
        
        timeout_add_local(Duration::from_millis(250), move || {
            if let (Some(icon), Some(label), Some(scale), Some(vol_label), Some(mute), Some(device)) = 
                (icon_weak.upgrade(), label_weak.upgrade(), volume_scale_weak.upgrade(), 
                 volume_label_weak.upgrade(), mute_switch_weak.upgrade(), device_name_label_weak.upgrade()) {
                Self::update_audio(&icon, &label, &scale, &vol_label, &mute, &device, audio_info_clone.clone());
                
                // Now schedule the regular updates at 500ms intervals
                let icon_weak2 = icon.downgrade();
                let label_weak2 = label.downgrade();
                let volume_scale_weak2 = scale.downgrade();
                let volume_label_weak2 = vol_label.downgrade();
                let mute_switch_weak2 = mute.downgrade();
                let device_name_label_weak2 = device.downgrade();
                let audio_info_clone2 = audio_info_clone.clone();
                
                timeout_add_local(Duration::from_millis(500), move || {
                    if let (Some(icon), Some(label), Some(scale), Some(vol_label), Some(mute), Some(device)) = 
                        (icon_weak2.upgrade(), label_weak2.upgrade(), volume_scale_weak2.upgrade(), 
                         volume_label_weak2.upgrade(), mute_switch_weak2.upgrade(), device_name_label_weak2.upgrade()) {
                        Self::update_audio(&icon, &label, &scale, &vol_label, &mute, &device, audio_info_clone2.clone());
                        glib::ControlFlow::Continue
                    } else {
                        glib::ControlFlow::Break
                    }
                });
                
                glib::ControlFlow::Break
            } else {
                glib::ControlFlow::Break
            }
        });
        
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
    
    fn update_audio(
        icon: &Image,
        label: &Label,
        scale: &Scale,
        volume_label: &Label,
        mute_switch: &Switch,
        device_label: &Label,
        audio_info: Arc<Mutex<AudioInfo>>
    ) {
        if let Some(info) = Self::get_audio_info() {
            // Update stored info
            if let Ok(mut stored_info) = audio_info.lock() {
                *stored_info = info.clone();
            }
            
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
            device_label.set_text(&info.device_name);
        } else {
            // If we can't get audio info, at least ensure the UI is in a valid state
            warn!("Failed to get audio info");
            if let Ok(stored_info) = audio_info.lock() {
                Self::update_icon(icon, stored_info.volume, stored_info.muted);
                if stored_info.muted {
                    label.set_text("Muted");
                } else {
                    label.set_text(&format!("{}%", stored_info.volume));
                }
            }
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
                
                // Get device name from wpctl status
                let device_name = if let Ok(status_output) = Command::new("wpctl")
                    .arg("status")
                    .output()
                {
                    let status_str = String::from_utf8_lossy(&status_output.stdout);
                    let mut in_sinks_section = false;
                    let mut found_device_name = "Unknown".to_string();
                    
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
                    
                    found_device_name
                } else {
                    "Unknown".to_string()
                };
                
                return Some(AudioInfo {
                    volume,
                    muted,
                    device_name,
                });
            }
        }
        
        // Try wpctl with sink ID parsing (older method)
        if let Ok(status_output) = Command::new("wpctl")
            .arg("status")
            .output()
        {
            let status_str = String::from_utf8_lossy(&status_output.stdout);
            
            // Find the default sink
            let mut default_sink_id = None;
            let mut in_sinks_section = false;
            let mut device_name = "Unknown".to_string();
            
            for line in status_str.lines() {
                if line.contains("Sinks:") {
                    in_sinks_section = true;
                    continue;
                }
                if in_sinks_section && (line.starts_with(" ├─") || line.starts_with(" └─")) {
                    // Check if this is the default sink (marked with *)
                    if line.contains("*") {
                        info!("Found default sink line: {}", line);
                        // Extract ID and name
                        if let Some(id_start) = line.find('.') {
                            if let Some(id_str) = line[..id_start].split_whitespace().last() {
                                default_sink_id = id_str.parse::<u32>().ok();
                                info!("Parsed sink ID: {:?}", default_sink_id);
                                // Extract device name
                                if let Some(name_start) = line.find('[') {
                                    if let Some(name_end) = line.find(']') {
                                        device_name = line[name_start+1..name_end].to_string();
                                    }
                                }
                            }
                        }
                        break;
                    }
                }
                if in_sinks_section && !line.starts_with(" ") && !line.is_empty() {
                    break;
                }
            }
            
            // Get volume using wpctl
            if let Some(sink_id) = default_sink_id {
                if let Ok(volume_output) = Command::new("wpctl")
                    .args(&["get-volume", &sink_id.to_string()])
                    .output()
                {
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
                    
                    return Some(AudioInfo {
                        volume,
                        muted,
                        device_name,
                    });
                }
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
            
            // Get device name
            let device_name = if let Ok(device_output) = Command::new("pactl")
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
                            return Some(AudioInfo {
                                volume,
                                muted,
                                device_name: desc_line
                                    .split(':')
                                    .nth(1)
                                    .unwrap_or("Unknown")
                                    .trim()
                                    .to_string(),
                            });
                        }
                    }
                }
                sink_name
            } else {
                "Unknown".to_string()
            };
            
            return Some(AudioInfo {
                volume,
                muted,
                device_name,
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
                    });
                }
            }
        }
        
        None
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
