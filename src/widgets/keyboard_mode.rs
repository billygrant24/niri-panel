// src/widgets/keyboard_mode.rs - Add this file to your project
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Popover};
use gtk4_layer_shell::{Layer, LayerShell};
use std::cell::RefCell;
use std::rc::Rc;
use tracing::info;

/// Helper to manage keyboard mode for popovers
pub struct KeyboardModeManager {
    window_weak: WeakRef<ApplicationWindow>,
    active_popovers: Rc<RefCell<i32>>,
}

impl KeyboardModeManager {
    pub fn new(window_weak: WeakRef<ApplicationWindow>, active_popovers: Rc<RefCell<i32>>) -> Self {
        Self {
            window_weak,
            active_popovers,
        }
    }

    /// Connect this to a popover to automatically manage keyboard mode
    pub fn connect_to_popover(&self, popover: &Popover, widget_name: &str) {
        let window_weak_show = self.window_weak.clone();
        let active_popovers_show = self.active_popovers.clone();
        let widget_name_show = widget_name.to_string();

        popover.connect_show(move |_| {
            *active_popovers_show.borrow_mut() += 1;
            if let Some(window) = window_weak_show.upgrade() {
                window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::OnDemand);
                info!(
                    "{} popover shown - keyboard mode set to OnDemand (active popovers: {})",
                    widget_name_show,
                    *active_popovers_show.borrow()
                );
            }
        });

        let window_weak_hide = self.window_weak.clone();
        let active_popovers_hide = self.active_popovers.clone();
        let widget_name_hide = widget_name.to_string();

        popover.connect_hide(move |_| {
            *active_popovers_hide.borrow_mut() -= 1;
            let count = *active_popovers_hide.borrow();
            if count == 0 {
                if let Some(window) = window_weak_hide.upgrade() {
                    window.set_keyboard_mode(gtk4_layer_shell::KeyboardMode::None);
                    info!(
                        "{} popover hidden - keyboard mode set to None",
                        widget_name_hide
                    );
                }
            } else {
                info!(
                    "{} popover hidden - keeping keyboard mode (active popovers: {})",
                    widget_name_hide, count
                );
            }
        });
    }

    /// Clone the manager for use in another widget
    pub fn clone(&self) -> Self {
        Self {
            window_weak: self.window_weak.clone(),
            active_popovers: self.active_popovers.clone(),
        }
    }
}
