use clap::ValueEnum;

pub mod config;
pub mod ipc;
pub mod niri_ipc;
pub mod panel;
pub mod popover_registry;
pub mod widgets;

/// Available panel widgets that can be controlled
#[derive(ValueEnum, Clone, Debug)]
pub enum Widget {
    Launcher,
    Places,
    Servers,
    Search,
    Git,
    Secrets,
    Sound,
    Bluetooth,
    Network,
    Battery,
    Clock,
    Power,
}

impl ToString for Widget {
    fn to_string(&self) -> String {
        match self {
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
        }.to_string()
    }
}