use gtk4::prelude::*;
use gtk4::{Button, Image, Popover, Box, Orientation, Label, ListBox, ListBoxRow};
use anyhow::Result;
use std::process::Command;
use tracing::warn;

pub struct Power {
    button: Button,
}

#[derive(Debug, Clone)]
enum PowerAction {
    Lock,
    Logout,
    Sleep,
    Reboot,
    Shutdown,
}

impl PowerAction {
    fn label(&self) -> &str {
        match self {
            PowerAction::Lock => "Lock Screen",
            PowerAction::Logout => "Log Out",
            PowerAction::Sleep => "Sleep",
            PowerAction::Reboot => "Restart",
            PowerAction::Shutdown => "Shut Down",
        }
    }
    
    fn icon(&self) -> &str {
        match self {
            PowerAction::Lock => "system-lock-screen-symbolic",
            PowerAction::Logout => "system-log-out-symbolic",
            PowerAction::Sleep => "system-suspend-symbolic",
            PowerAction::Reboot => "system-reboot-symbolic",
            PowerAction::Shutdown => "system-shutdown-symbolic",
        }
    }
    
    fn execute(&self) {
        match self {
            PowerAction::Lock => {
                // Try swaylock first, then other lock screens
                if Command::new("swaylock")
                    .args(&["-c", "2e3440", "-f"])
                    .spawn()
                    .is_err()
                {
                    // Try other lock screens
                    let _ = Command::new("loginctl").arg("lock-session").spawn();
                }
            }
            PowerAction::Logout => {
                let user = std::env::var("USER").unwrap_or_default();
                let _ = Command::new("loginctl")
                    .args(&["kill-user", &user])
                    .spawn();
            }
            PowerAction::Sleep => {
                // Lock first
                if Command::new("swaylock")
                    .args(&["-c", "2e3440", "-f"])
                    .spawn()
                    .is_ok()
                {
                    // Wait a moment for lock to engage
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
                
                // Then hibernate
                let _ = Command::new("systemctl").arg("hibernate").spawn();
            }
            PowerAction::Reboot => {
                let _ = Command::new("systemctl").arg("reboot").spawn();
            }
            PowerAction::Shutdown => {
                let _ = Command::new("systemctl").arg("poweroff").spawn();
            }
        }
    }
    
    fn needs_confirmation(&self) -> bool {
        match self {
            PowerAction::Lock => false,
            _ => true,
        }
    }
}

impl Power {
    pub fn new() -> Result<Self> {
        let button = Button::new();
        button.add_css_class("power");
        
        // Try multiple power icon fallbacks
        let icon_names = vec![
            "system-shutdown-symbolic",
            "application-exit-symbolic",
            "system-power-symbolic",
            "gtk-quit"
        ];
        
        let image = Image::new();
        for icon_name in icon_names {
            if gtk4::IconTheme::default().has_icon(icon_name) {
                image.set_from_icon_name(Some(icon_name));
                break;
            }
        }
        
        if image.icon_name().is_none() {
            let label = Label::new(Some("⏻"));
            label.add_css_class("icon-fallback");
            button.set_child(Some(&label));
        } else {
            image.set_icon_size(gtk4::IconSize::Large);
            button.set_child(Some(&image));
        }
        
        // Create popover for power menu
        let popover = Popover::new();
        popover.set_parent(&button);
        popover.add_css_class("power-popover");
        popover.set_has_arrow(false);
        popover.set_autohide(true);
        
        let popover_box = Box::new(Orientation::Vertical, 0);
        popover_box.set_margin_top(5);
        popover_box.set_margin_bottom(5);
        popover_box.set_size_request(200, -1);
        
        // User info section
        let user_box = Box::new(Orientation::Horizontal, 10);
        user_box.set_margin_start(15);
        user_box.set_margin_end(15);
        user_box.set_margin_top(10);
        user_box.set_margin_bottom(10);
        
        let user_icon = Image::from_icon_name("avatar-default-symbolic");
        user_icon.set_pixel_size(32);
        user_box.append(&user_icon);
        
        let user_label = Label::new(Some(&std::env::var("USER").unwrap_or_else(|_| "User".to_string())));
        user_label.add_css_class("power-user-label");
        user_label.set_halign(gtk4::Align::Start);
        user_box.append(&user_label);
        
        popover_box.append(&user_box);
        
        // Separator
        let separator = gtk4::Separator::new(Orientation::Horizontal);
        popover_box.append(&separator);
        
        // Power actions
        let actions_list = ListBox::new();
        actions_list.add_css_class("power-actions-list");
        actions_list.set_selection_mode(gtk4::SelectionMode::None);
        
        let actions = vec![
            PowerAction::Lock,
            PowerAction::Logout,
            PowerAction::Sleep,
            PowerAction::Reboot,
            PowerAction::Shutdown,
        ];
        
        for action in actions {
            let row = Self::create_action_row(action, popover.downgrade());
            actions_list.append(&row);
        }
        
        popover_box.append(&actions_list);
        popover.set_child(Some(&popover_box));
        
        // Show popover on click
        button.connect_clicked(move |_| {
            popover.popup();
        });
        
        Ok(Self { button })
    }
    
    fn create_action_row(action: PowerAction, popover_weak: gtk4::glib::WeakRef<Popover>) -> ListBoxRow {
        let row = ListBoxRow::new();
        row.add_css_class("power-action-row");
        
        let button = Button::new();
        button.add_css_class("power-action-button");
        
        let hbox = Box::new(Orientation::Horizontal, 10);
        hbox.set_margin_start(10);
        hbox.set_margin_end(10);
        hbox.set_margin_top(8);
        hbox.set_margin_bottom(8);
        
        let icon = Image::from_icon_name(action.icon());
        icon.set_pixel_size(20);
        hbox.append(&icon);
        
        let label = Label::new(Some(action.label()));
        label.set_hexpand(true);
        label.set_halign(gtk4::Align::Start);
        hbox.append(&label);
        
        button.set_child(Some(&hbox));
        
        button.connect_clicked(move |_| {
            if action.needs_confirmation() {
                Self::show_confirmation_dialog(action.clone(), popover_weak.clone());
            } else {
                action.execute();
                if let Some(popover) = popover_weak.upgrade() {
                    popover.popdown();
                }
            }
        });
        
        row.set_child(Some(&button));
        row
    }
    
    fn show_confirmation_dialog(action: PowerAction, popover_weak: gtk4::glib::WeakRef<Popover>) {
        // Close the main popover first
        if let Some(main_popover) = popover_weak.upgrade() {
            main_popover.popdown();
        }
        
        // Create a new window for confirmation
        let dialog = gtk4::Window::new();
        dialog.set_title(Some("Confirm Action"));
        dialog.set_modal(true);
        dialog.set_resizable(false);
        dialog.set_decorated(false);
        dialog.add_css_class("power-confirm-dialog");
        
        // Center the window on screen
        dialog.set_default_size(300, 150);
        
        let confirm_box = Box::new(Orientation::Vertical, 20);
        confirm_box.set_margin_top(30);
        confirm_box.set_margin_bottom(30);
        confirm_box.set_margin_start(30);
        confirm_box.set_margin_end(30);
        
        // Icon and message
        let icon = Image::from_icon_name(action.icon());
        icon.set_pixel_size(48);
        confirm_box.append(&icon);
        
        // Confirmation message
        let message = Label::new(Some(&format!("Are you sure you want to {}?", action.label().to_lowercase())));
        message.add_css_class("power-confirm-message");
        message.set_wrap(true);
        confirm_box.append(&message);
        
        // Buttons
        let button_box = Box::new(Orientation::Horizontal, 10);
        button_box.set_halign(gtk4::Align::Center);
        button_box.set_margin_top(10);
        
        let cancel_button = Button::with_label("Cancel");
        cancel_button.add_css_class("power-confirm-cancel");
        
        let confirm_button = Button::with_label("Confirm");
        confirm_button.add_css_class("power-confirm-button");
        
        match &action {
            PowerAction::Shutdown | PowerAction::Reboot => {
                confirm_button.add_css_class("destructive-action");
            }
            _ => {}
        }
        
        button_box.append(&cancel_button);
        button_box.append(&confirm_button);
        confirm_box.append(&button_box);
        
        dialog.set_child(Some(&confirm_box));
        
        // Handle button clicks
        let dialog_weak = dialog.downgrade();
        cancel_button.connect_clicked(move |_| {
            if let Some(dialog) = dialog_weak.upgrade() {
                dialog.close();
            }
        });
        
        let dialog_weak2 = dialog.downgrade();
        confirm_button.connect_clicked(move |_| {
            action.execute();
            if let Some(dialog) = dialog_weak2.upgrade() {
                dialog.close();
            }
        });
        
        // Handle Escape key
        let controller = gtk4::EventControllerKey::new();
        let dialog_weak3 = dialog.downgrade();
        controller.connect_key_pressed(move |_, key, _, _| {
            if key == gtk4::gdk::Key::Escape {
                if let Some(dialog) = dialog_weak3.upgrade() {
                    dialog.close();
                }
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        });
        dialog.add_controller(controller);
        
        // Show the dialog centered
        dialog.present();
        
        // Focus the cancel button by default
        cancel_button.grab_focus();
    }
    
    pub fn widget(&self) -> &Button {
        &self.button
    }
}
