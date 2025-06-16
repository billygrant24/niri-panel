use anyhow::Result;
use clap::{Parser, Subcommand};
use niri_panel::ipc::IpcClient;
use niri_panel::Widget;

/// Control utility for Niri Panel
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show a specific widget popover
    Show {
        /// Widget to show
        #[arg(value_enum)]
        widget: Widget,
    },
    /// Hide a specific widget popover
    Hide {
        /// Widget to hide
        #[arg(value_enum)]
        widget: Widget,
    },
    /// List available widgets
    List,
}

fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Process the command
    match args.command {
        Commands::Show { widget } => {
            let widget_name = widget.to_string().to_lowercase();
            let response = IpcClient::send_command(&format!("show {}", widget_name))?;
            println!("{}", response);
        }
        Commands::Hide { widget } => {
            let widget_name = widget.to_string().to_lowercase();
            let response = IpcClient::send_command(&format!("hide {}", widget_name))?;
            println!("{}", response);
        }
        Commands::List => {
            let response = IpcClient::send_command("list")?;
            println!("Available widgets: {}", response);
        }
    }

    Ok(())
}