use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelConfig {
    pub height: i32,
    pub show_launcher: bool,
    pub show_places: bool,
    pub show_search: bool,
    pub show_workspaces: bool,
    pub show_clock: bool,
    pub show_battery: bool,
    pub show_network: bool,
    pub show_sound: bool,
    pub show_bluetooth: bool,
    pub show_power: bool,
    pub show_git: bool,
    pub show_secrets: bool,
    pub clock_format: String,
    pub launcher_icon: String,
    pub git: GitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    pub repositories: Vec<GitRepository>,
    pub services: Vec<GitService>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitRepository {
    pub name: String,
    pub path: String,
    pub service: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitService {
    pub name: String,
    pub url_pattern: String,
    pub issues_pattern: String,
}

impl Default for PanelConfig {
    fn default() -> Self {
        Self {
            height: 32,
            show_launcher: true,
            show_places: true,
            show_search: true,
            show_workspaces: true,
            show_clock: true,
            show_battery: true,
            show_network: true,
            show_sound: true,
            show_bluetooth: true,
            show_power: true,
            show_git: true,
            show_secrets: true,
            clock_format: "%a %b %e %l:%M %p".to_string(),
            launcher_icon: "view-app-grid-symbolic".to_string(),
            git: GitConfig::default(),
        }
    }
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            repositories: vec![
                GitRepository {
                    name: "Example Repo".to_string(),
                    path: "~/Projects/example-repo".to_string(),
                    service: "github".to_string(),
                    url: "https://github.com/username/example-repo".to_string(),
                },
            ],
            services: vec![
                GitService {
                    name: "github".to_string(),
                    url_pattern: "https://github.com/{owner}/{repo}".to_string(),
                    issues_pattern: "https://github.com/{owner}/{repo}/issues".to_string(),
                },
                GitService {
                    name: "gitlab".to_string(),
                    url_pattern: "https://gitlab.com/{owner}/{repo}".to_string(),
                    issues_pattern: "https://gitlab.com/{owner}/{repo}/issues".to_string(),
                },
            ],
        }
    }
}

impl PanelConfig {
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;
        
        if config_path.exists() {
            let content = fs::read_to_string(&config_path)?;
            let config: Self = toml::from_str(&content)?;
            Ok(config)
        } else {
            // Create default config
            let config = Self::default();
            let _ = config.save(); // Ignore save errors for now
            Ok(config)
        }
    }
    
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;
        let config_dir = config_path.parent().unwrap();
        
        fs::create_dir_all(config_dir)?;
        let content = toml::to_string_pretty(self)?;
        fs::write(config_path, content)?;
        
        Ok(())
    }
    
    fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find config directory"))?;
        Ok(config_dir.join("niri-panel").join("config.toml"))
    }
}