use anyhow::Result;
use gtk4::prelude::*;
use gtk4::Popover;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tracing::info;

use crate::Widget;

/// Registry to store and access all panel widget popovers
#[derive(Debug, Default)]
pub struct PopoverRegistry {
    inner: RefCell<HashMap<String, Rc<Popover>>>,
}

// Singleton instance of the registry
static mut INSTANCE: Option<PopoverRegistry> = None;

impl PopoverRegistry {
    /// Get the global registry instance
    pub fn global() -> &'static PopoverRegistry {
        unsafe {
            if INSTANCE.is_none() {
                INSTANCE = Some(PopoverRegistry::default());
            }
            INSTANCE.as_ref().unwrap()
        }
    }

    /// Register a popover with a name
    pub fn register(&self, name: &str, popover: Popover) -> Result<()> {
        info!("Registering popover: {}", name);
        self.inner.borrow_mut().insert(name.to_string(), Rc::new(popover));
        Ok(())
    }

    /// Show a popover by name
    pub fn show(&self, name: &str) -> Result<bool> {
        if let Some(popover) = self.inner.borrow().get(name) {
            info!("Showing popover: {}", name);
            popover.popup();
            return Ok(true);
        }
        
        Ok(false)
    }

    /// Hide a popover by name
    pub fn hide(&self, name: &str) -> Result<bool> {
        if let Some(popover) = self.inner.borrow().get(name) {
            info!("Hiding popover: {}", name);
            popover.popdown();
            return Ok(true);
        }
        
        Ok(false)
    }

    /// Get all registered popover names
    pub fn get_names(&self) -> Vec<String> {
        self.inner.borrow().keys().cloned().collect()
    }

    /// Convert Widget enum to popover name
    pub fn widget_to_name(widget: &Widget) -> &'static str {
        match widget {
            Widget::Launcher => "launcher",
            Widget::Places => "places",
            Widget::Servers => "servers",
            Widget::Search => "search",
            Widget::Git => "git",
            Widget::Secrets => "secrets",
            Widget::Sound => "sound",
            Widget::Bluetooth => "bluetooth",
            Widget::Network => "network",
            Widget::Battery => "battery",
            Widget::Clock => "clock",
            Widget::Power => "power",
        }
    }
}