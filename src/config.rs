use serde::Deserialize;
use std::{fmt, fs, net::SocketAddr, path::Path};
use tracing::{info, warn};

pub const CONFIG_FILE: &str = "config.yaml";

const DEFAULT_CONFIG_YAML: &str = r#"# z-web configuration
# Edit this file to add/remove servers — changes are applied automatically (hot reload).

servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"
    # Set to true to execute .php files (and index.php) via php-cgi
    php: false
    # php_cgi_path: "php-cgi"   # override if php-cgi isn't on PATH
    # spa: false                # set true to fallback 404s to index.html for SPAs (React/Vue)

  # Uncomment to add more:
  # - ip: "127.0.0.1"
  #   port: 8081
  #   web_folder: "web_admin"
  #   php: true
  #   spa: false
"#;

/// Errors that can occur while loading, parsing, or validating config.yaml.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_yaml::Error),
    DuplicatePort(u16),
    InvalidPort(u16),
    EmptyWebFolder(u16),
    InvalidAddress { ip: String, port: u16 },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "I/O error: {e}"),
            ConfigError::Parse(e) => write!(f, "Failed to parse {CONFIG_FILE}: {e}"),
            ConfigError::DuplicatePort(port) => {
                write!(
                    f,
                    "Duplicate port {port} in config.yaml — each server needs a unique port"
                )
            }
            ConfigError::InvalidPort(port) => {
                write!(
                    f,
                    "Invalid port {port} in config.yaml — port must be between 1 and 65535"
                )
            }
            ConfigError::EmptyWebFolder(port) => {
                write!(
                    f,
                    "Server on port {port} has an empty web_folder in config.yaml"
                )
            }
            ConfigError::InvalidAddress { ip, port } => {
                write!(f, "Invalid address \"{ip}:{port}\" in config.yaml")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

impl From<std::io::Error> for ConfigError {
    fn from(e: std::io::Error) -> Self {
        ConfigError::Io(e)
    }
}

impl From<serde_yaml::Error> for ConfigError {
    fn from(e: serde_yaml::Error) -> Self {
        ConfigError::Parse(e)
    }
}

fn default_php_cgi_path() -> String {
    "php-cgi".to_string()
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct ServerConfig {
    pub ip: String,
    pub port: u16,
    pub web_folder: String,
    /// Execute `.php` files (and `index.php`) through `php-cgi`.
    /// Defaults to `false` so existing config.yaml files keep working unchanged.
    #[serde(default)]
    pub php: bool,
    /// Path to the `php-cgi` binary. Defaults to `php-cgi` (resolved via PATH).
    #[serde(default = "default_php_cgi_path")]
    pub php_cgi_path: String,
    /// Fallback 404 routes to index.html for Single Page Applications (React, Vue, Vite, etc.).
    #[serde(default)]
    pub spa: bool,
}

impl ServerConfig {
    /// Parse this server's `ip:port` into a `SocketAddr`.
    /// Returns an error instead of panicking so a single bad entry
    /// doesn't take down every other server.
    pub fn socket_addr(&self) -> Result<SocketAddr, ConfigError> {
        format!("{}:{}", self.ip, self.port)
            .parse()
            .map_err(|_| ConfigError::InvalidAddress {
                ip: self.ip.clone(),
                port: self.port,
            })
    }

    /// Create the web folder + a default index.html if it doesn't exist yet.
    /// Returns an error instead of panicking on I/O failure.
    pub fn ensure_folder(&self) -> std::io::Result<()> {
        let folder = &self.web_folder;
        let folder_path = Path::new(folder);
        if folder_path.exists() {
            return Ok(());
        }
        warn!(folder = %folder, "Web folder missing, creating with default index.html");
        fs::create_dir_all(folder_path)?;

        let html = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>z-web | :{port}</title>
  <style>
    *{{box-sizing:border-box;margin:0;padding:0}}
    body{{font-family:system-ui,-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,sans-serif;
         background:#0f172a;color:#f8fafc;display:flex;align-items:center;justify-content:center;min-height:100vh}}
    .card{{border:1px solid #334155;border-radius:12px;padding:2.5rem 3.5rem;text-align:center;
           background:#1e293b;box-shadow:0 10px 25px -5px rgba(0,0,0,0.3)}}
    h1{{font-size:2rem;margin-bottom:.5rem;color:#38bdf8}}
    p{{color:#94a3b8;margin-top:.5rem;font-size:1rem}}
    .badge{{display:inline-block;margin-top:1.25rem;padding:.35rem .85rem;
            background:#0284c7;border-radius:9999px;font-size:.8rem;font-weight:600;color:#fff}}
  </style>
</head>
<body>
  <div class="card">
    <h1>⚡ z-web</h1>
    <p>Serving <b>{folder}</b> on port <b>{port}</b></p>
    <span class="badge">Live Reload Active</span>
  </div>
</body>
</html>
"#,
            port = self.port,
            folder = folder,
        );

        fs::write(folder_path.join("index.html"), html)?;

        info!(folder = %folder, port = self.port, "Created default index.html");
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

impl Config {
    /// Load config from file path.
    /// If default path ("config.yaml") doesn't exist, create it with defaults.
    pub fn load(config_path: &str) -> Result<Self, ConfigError> {
        let path = Path::new(config_path);
        if !path.exists() {
            if config_path == CONFIG_FILE {
                info!(file = %config_path, "No config found, creating default");
                fs::write(config_path, DEFAULT_CONFIG_YAML)?;
            } else {
                return Err(ConfigError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Config file '{config_path}' not found"),
                )));
            }
        }

        info!(file = %config_path, "Loading config");
        let content = fs::read_to_string(config_path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Reload config from file path (used by hot-reload path).
    pub fn reload(config_path: &str) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(config_path)?;
        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate config: reject duplicate ports, port 0, and empty web folders.
    fn validate(&self) -> Result<(), ConfigError> {
        let mut seen = std::collections::HashSet::new();
        for server in &self.servers {
            if server.port == 0 {
                return Err(ConfigError::InvalidPort(0));
            }
            if !seen.insert(server.port) {
                return Err(ConfigError::DuplicatePort(server.port));
            }
            if server.web_folder.trim().is_empty() {
                return Err(ConfigError::EmptyWebFolder(server.port));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(ip: &str, port: u16, folder: &str) -> ServerConfig {
        ServerConfig {
            ip: ip.to_string(),
            port,
            web_folder: folder.to_string(),
            php: false,
            php_cgi_path: default_php_cgi_path(),
            spa: false,
        }
    }

    #[test]
    fn parses_spa_config() {
        let yaml = r#"
servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "dist"
    spa: true
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.servers[0].spa);
    }

    #[test]
    fn parses_minimal_yaml() {
        let yaml = r#"
servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.servers.len(), 1);
        assert_eq!(config.servers[0].port, 8080);
        assert_eq!(config.servers[0].web_folder, "web");
    }

    #[test]
    fn parses_multiple_servers() {
        let yaml = r#"
servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"
  - ip: "127.0.0.1"
    port: 8081
    web_folder: "web_admin"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.servers.len(), 2);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn rejects_duplicate_ports() {
        let config = Config {
            servers: vec![
                server("0.0.0.0", 8080, "web"),
                server("127.0.0.1", 8080, "web_admin"),
            ],
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::DuplicatePort(8080))
        ));
    }

    #[test]
    fn socket_addr_valid() {
        let s = server("127.0.0.1", 8080, "web");
        assert_eq!(s.socket_addr().unwrap().port(), 8080);
    }

    #[test]
    fn socket_addr_invalid_ip_errors_instead_of_panicking() {
        let s = server("not-an-ip", 8080, "web");
        assert!(s.socket_addr().is_err());
    }

    #[test]
    fn php_defaults_to_disabled_for_backward_compatibility() {
        let yaml = r#"
servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.servers[0].php);
        assert_eq!(config.servers[0].php_cgi_path, "php-cgi");
    }

    #[test]
    fn php_can_be_enabled_with_custom_cgi_path() {
        let yaml = r#"
servers:
  - ip: "0.0.0.0"
    port: 8080
    web_folder: "web"
    php: true
    php_cgi_path: "/usr/bin/php-cgi8.3"
"#;
        let config: Config = serde_yaml::from_str(yaml).unwrap();
        assert!(config.servers[0].php);
        assert_eq!(config.servers[0].php_cgi_path, "/usr/bin/php-cgi8.3");
    }

    #[test]
    fn rejects_port_zero() {
        let config = Config {
            servers: vec![server("0.0.0.0", 0, "web")],
        };
        assert!(matches!(config.validate(), Err(ConfigError::InvalidPort(0))));
    }

    #[test]
    fn rejects_empty_web_folder() {
        let config = Config {
            servers: vec![server("0.0.0.0", 8080, "   ")],
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::EmptyWebFolder(8080))
        ));
    }
}
