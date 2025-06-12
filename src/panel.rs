use gtk4::prelude::*;
use gtk4::{Box, Orientation};
use anyhow::Result;

use crate::config::PanelConfig;
use crate::widgets::{Clock, Workspaces, Launcher, Battery, Network, Sound, Places, Power, Search};

pub struct Panel {
    container: Box,
    _config: PanelConfig,
}

impl Panel {
    pub fn new(config: PanelConfig) -> Result<Self> {
        let container = Box::new(Orientation::Horizontal, 0);
        container.add_css_class("panel");
        container.set_margin_top(10);
        container.set_margin_bottom(10);
        container.set_margin_start(10);
        container.set_margin_end(10);
        
        // Create left box for launcher and workspaces
        let left_box = Box::new(Orientation::Horizontal, 10);
        left_box.add_css_class("panel-left");
        left_box.set_halign(gtk4::Align::Start);
        left_box.set_hexpand(false);
        
        // Create center box (empty for now, can add window title later)
        let center_box = Box::new(Orientation::Horizontal, 10);
        center_box.add_css_class("panel-center");
        center_box.set_halign(gtk4::Align::Center);
        center_box.set_hexpand(true);
        
        // Create right box for clock and system tray
        let right_box = Box::new(Orientation::Horizontal, 10);
        right_box.add_css_class("panel-right");
        right_box.set_halign(gtk4::Align::End);
        right_box.set_hexpand(false);
        
        // Add widgets
        if config.show_launcher {
            let launcher = Launcher::new()?;
            left_box.append(launcher.widget());
        }
        
        if config.show_places {
            let places = Places::new()?;
            left_box.append(places.widget());
        }
        
        if config.show_search {
            let search = Search::new()?;
            left_box.append(search.widget());
        }
        
        if config.show_workspaces {
            let workspaces = Workspaces::new()?;
            left_box.append(workspaces.widget());
        }
        
        if config.show_sound {
            let sound = Sound::new()?;
            right_box.append(sound.widget());
        }
        
        if config.show_network {
            let network = Network::new()?;
            right_box.append(network.widget());
        }
        
        if config.show_battery {
            let battery = Battery::new()?;
            right_box.append(battery.widget());
        }
        
        if config.show_clock {
            let clock = Clock::new(&config.clock_format)?;
            right_box.append(clock.widget());
        }
        
        if config.show_power {
            let power = Power::new()?;
            right_box.append(power.widget());
        }
        
        // Pack everything
        container.append(&left_box);
        container.append(&center_box);
        container.append(&right_box);
        
        Ok(Self {
            container,
            _config: config,
        })
    }
    
    pub fn container(&self) -> &Box {
        &self.container
    }
}
