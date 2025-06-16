use anyhow::{Context, Result};
use gtk4::glib;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use tracing::{error, info};

use crate::popover_registry::PopoverRegistry;
use crate::Widget;

/// IPC server for niri-panel
pub struct IpcServer {
    socket_path: PathBuf,
}

impl IpcServer {
    /// Create a new IPC server
    pub fn new() -> Result<Self> {
        let socket_path = Self::socket_path()?;
        Ok(Self { socket_path })
    }

    /// Start the IPC server
    pub fn start(&self) -> Result<glib::SourceId> {
        // Make sure the directory exists
        if let Some(parent) = self.socket_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Remove the socket if it already exists
        if self.socket_path.exists() {
            fs::remove_file(&self.socket_path)?;
        }

        // Create the socket
        let listener = UnixListener::bind(&self.socket_path)
            .context(format!("Failed to bind to socket: {:?}", self.socket_path))?;

        info!("IPC server started on socket: {:?}", self.socket_path);

        // Create a channel to communicate with the GTK main loop
        let (sender, receiver) = glib::MainContext::channel(glib::Priority::DEFAULT);

        // Spawn a thread to listen for connections
        thread::spawn(move || {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let sender = sender.clone();
                        thread::spawn(move || {
                            if let Err(e) = Self::handle_connection(stream, sender) {
                                error!("Error handling IPC connection: {}", e);
                            }
                        });
                    }
                    Err(e) => error!("Error accepting connection: {}", e),
                }
            }
        });

        // Attach the receiver to the GTK main loop
        let source_id = receiver.attach(None, move |command| {
            if let Err(e) = Self::process_command(&command) {
                error!("Error processing IPC command: {}", e);
            }
            glib::ControlFlow::Continue
        });

        Ok(source_id)
    }

    /// Handle a connection
    fn handle_connection(
        stream: UnixStream,
        sender: glib::Sender<String>,
    ) -> Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut line = String::new();
        
        // Read a single line from the stream
        reader.read_line(&mut line)?;
        
        // Trim whitespace and send to main thread
        let command = line.trim().to_string();
        if !command.is_empty() {
            sender.send(command)?;
        }

        // Send an acknowledgment
        let mut writer = stream;
        writer.write_all(b"OK\n")?;
        
        Ok(())
    }

    /// Process a command in the main thread
    fn process_command(command: &str) -> Result<()> {
        info!("Processing IPC command: {}", command);
        
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok(());
        }

        match parts[0] {
            "show" => {
                if parts.len() < 2 {
                    error!("Missing widget name in 'show' command");
                    return Ok(());
                }
                
                let widget_name = parts[1];
                let registry = PopoverRegistry::global();
                registry.show(widget_name)?;
            }
            "hide" => {
                if parts.len() < 2 {
                    error!("Missing widget name in 'hide' command");
                    return Ok(());
                }
                
                let widget_name = parts[1];
                let registry = PopoverRegistry::global();
                registry.hide(widget_name)?;
            }
            "list" => {
                let registry = PopoverRegistry::global();
                let names = registry.get_names();
                info!("Available widgets: {:?}", names);
            }
            _ => {
                error!("Unknown IPC command: {}", parts[0]);
            }
        }
        
        Ok(())
    }

    /// Get the socket path
    pub fn socket_path() -> Result<PathBuf> {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR")
            .unwrap_or_else(|_| format!("/tmp/runtime-{}", std::env::var("USER").unwrap_or("user".to_string())));
            
        Ok(Path::new(&runtime_dir).join("niri-panel.sock"))
    }
}

/// IPC client for sending commands to niri-panel
pub struct IpcClient;

impl IpcClient {
    /// Send a command to niri-panel
    pub fn send_command(command: &str) -> Result<String> {
        let socket_path = IpcServer::socket_path()?;
        
        // Connect to the socket
        let mut stream = UnixStream::connect(&socket_path)
            .context(format!("Failed to connect to socket: {:?}", socket_path))?;
            
        // Send the command
        writeln!(stream, "{}", command)?;
        
        // Read the response
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?;
        
        Ok(response.trim().to_string())
    }
    
    /// Show a widget popover
    pub fn show_widget(widget: Widget) -> Result<()> {
        let widget_name = PopoverRegistry::widget_to_name(&widget);
        Self::send_command(&format!("show {}", widget_name))?;
        Ok(())
    }
}