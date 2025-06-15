use gtk4::prelude::*;
use gtk4::{Box, Label, Button, Orientation, Image, Popover, ApplicationWindow};
use gtk4_layer_shell::{LayerShell};
use gtk4::glib::WeakRef;
use std::rc::Rc;
use std::cell::RefCell;
use tracing::info;
use anyhow::Result;

pub struct Git {
    button: Button,
}

impl Git {
    pub fn new(
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>
    ) -> Result<Self> {
        let button = Button::new();
        button.add_css_class("git");
        
        let container = Box::new(Orientation::Horizontal, 5);
        
        // Placeholder for Nerd Font icon
        let icon = Label::new(Some("[NF]"));
        icon.add_css_class("git-icon");
        
        let arrow = Image::from_icon_name("pan-down-symbolic");
        arrow.set_pixel_size(10);
        
        container.append(&icon);
        container.append(&arrow);
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
        
        let popover_box = Box::new(Orientation::Vertical, 10);
        popover_box.set_margin_top(10);
        popover_box.set_margin_bottom(10);
        popover_box.set_margin_start(10);
        popover_box.set_margin_end(10);
        popover_box.set_size_request(300, -1);
        
        // Add placeholder content
        let placeholder = Label::new(Some("Git widget - placeholder"));
        placeholder.set_halign(gtk4::Align::Start);
        popover_box.append(&placeholder);
        
        popover.set_child(Some(&popover_box));
        
        // Show popover on click
        button.connect_clicked(move |_| {
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}