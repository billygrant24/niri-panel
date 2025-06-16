use anyhow::Result;
use gtk4::glib::WeakRef;
use gtk4::prelude::*;
use gtk4::{ApplicationWindow, Box, Orientation};
use std::cell::RefCell;
use std::rc::Rc;

use crate::config::PanelConfig;
use crate::widgets::{
    Battery, Bluetooth, Clock, Git, Launcher, Network, Overview, Places, Power, Search, Secrets,
    Sound, Workspaces,
};

pub struct Panel {
    container: Box,
    _config: PanelConfig,
}

impl Panel {
    pub fn new(
        config: PanelConfig,
        window_weak: WeakRef<ApplicationWindow>,
        active_popovers: Rc<RefCell<i32>>,
    ) -> Result<Self> {
        let container = Box::new(Orientation::Horizontal, 0);
        container.add_css_class("panel");
        container.set_margin_top(0);
        container.set_margin_bottom(0);
        container.set_margin_start(5);
        container.set_margin_end(0);

        // Create left box for launcher and workspaces
        let left_box = Box::new(Orientation::Horizontal, 10);
        left_box.add_css_class("panel-left");
        left_box.set_halign(gtk4::Align::Start);
        left_box.set_hexpand(true);

        // Create center box (empty for now, can add window title later)
        let center_box = Box::new(Orientation::Horizontal, 10);
        center_box.add_css_class("panel-center");
        center_box.set_halign(gtk4::Align::Center);
        center_box.set_hexpand(false);

        // Create right box for clock and system tray
        let right_box = Box::new(Orientation::Horizontal, 10);
        right_box.add_css_class("panel-right");
        right_box.set_halign(gtk4::Align::End);
        right_box.set_hexpand(true);

        // Add widgets with keyboard mode management where needed
        let overview = Overview::new()?;
        left_box.append(overview.widget());

        if config.show_launcher {
            let launcher = Launcher::new(window_weak.clone(), active_popovers.clone())?;
            left_box.append(launcher.widget());
        }

        if config.show_places {
            let places = Places::new(window_weak.clone(), active_popovers.clone())?;
            left_box.append(places.widget());
        }

        if config.show_git {
            let git = Git::new(window_weak.clone(), active_popovers.clone(), &config)?;
            left_box.append(git.widget());
        }

        if config.show_search {
            let search = Search::new(window_weak.clone(), active_popovers.clone())?;
            left_box.append(search.widget());
        }

        if config.show_secrets {
            let secrets = Secrets::new(window_weak.clone(), active_popovers.clone())?;
            left_box.append(secrets.widget());
        }

        if config.show_workspaces {
            let workspaces = Workspaces::new()?;
            left_box.append(workspaces.widget());
        }

        if config.show_sound {
            let sound = Sound::new(window_weak.clone(), active_popovers.clone())?;
            right_box.append(sound.widget());
        }

        if config.show_bluetooth {
            let bluetooth = Bluetooth::new(window_weak.clone(), active_popovers.clone())?;
            right_box.append(bluetooth.widget());
        }

        if config.show_network {
            let network = Network::new(window_weak.clone(), active_popovers.clone())?;
            right_box.append(network.widget());
        }

        if config.show_battery {
            let battery = Battery::new(window_weak.clone(), active_popovers.clone())?;
            right_box.append(battery.widget());
        }

        if config.show_clock {
            let clock = Clock::new(
                &config.clock_format,
                window_weak.clone(),
                active_popovers.clone(),
            )?;
            center_box.append(clock.widget());
        }

        if config.show_power {
            let power = Power::new(window_weak.clone(), active_popovers.clone())?;
            center_box.append(power.widget());
        }

        // Pack everything
        container.append(&left_box);
        container.append(&right_box);
        container.append(&center_box);

        Ok(Self {
            container,
            _config: config,
        })
    }

    pub fn container(&self) -> &Box {
        &self.container
    }
}
