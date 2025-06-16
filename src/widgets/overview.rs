use gtk4::prelude::*;
use gtk4::{Button, Image, Label};
use anyhow::Result;
use std::process::Command;
use tracing::warn;

pub struct Overview {
    button: Button,
}

impl Overview {
    pub fn new() -> Result<Self> {
        let button = Button::new();
        button.add_css_class("overview");
        
        // Try multiple overview/activities icon fallbacks
        let icon_names = vec![
            "go-up-symbloc",
            "activities-overview-symbolic",
            "view-fullscreen-symbolic",
            "view-restore-symbolic",
            "view-app-grid-symbolic",
            "window-restore-symbolic"
        ];
        
        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                break;
            }
        }
        
        if image.icon_name().is_none() {
            // Use a grid-like fallback character
            let label = Label::new(Some("⊞"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }
        
        button.set_tooltip_text(Some("Toggle Overview"));
        
        // Handle click to toggle overview
        button.connect_clicked(|_| {
            Self::toggle_overview();
        });
        
        Ok(Self { button })
    }
    
    fn toggle_overview() {
        match Command::new("niri")
            .args(&["msg", "action", "toggle-overview"])
            .output()
        {
            Ok(output) => {
                if !output.status.success() {
                    warn!("Failed to toggle overview: {:?}", String::from_utf8_lossy(&output.stderr));
                }
            }
            Err(e) => {
                warn!("Failed to execute niri command: {}", e);
            }
        }
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}
