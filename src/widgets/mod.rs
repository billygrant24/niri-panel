use gtk4::Popover;

mod battery;
mod bluetooth;
mod clock;
mod git;
mod keyboard_mode;
mod launcher;
mod network;
mod overview;
mod places;
mod power;
mod search;
mod secrets;
mod servers;
mod sound;
mod workspaces;

/// Common trait for all widgets that have popovers
pub trait Widget {
    /// Get the popover for this widget, if any
    fn popover(&self) -> Option<&Popover> {
        None
    }
}

pub use battery::Battery;
pub use bluetooth::Bluetooth;
pub use clock::Clock;
pub use git::Git;
pub use keyboard_mode::KeyboardModeManager;
pub use launcher::Launcher;
pub use network::Network;
pub use overview::Overview;
pub use places::Places;
pub use power::Power;
pub use search::Search;
pub use secrets::Secrets;
pub use servers::Servers;
pub use sound::Sound;
pub use workspaces::Workspaces;
