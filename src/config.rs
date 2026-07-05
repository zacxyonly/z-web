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

  # Uncomment to add more:
  # - ip: "127.0.0.1"
  #   port: 8081
  #   web_folder: "web_admin"
"#;

/// Errors that can occur while loading, parsing, or validating config.yaml.
#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(serde_yaml::Error),
    DuplicatePort(u16),
    InvalidAddress { ip: String, port: u16 },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "I/O error: {e}"),
            ConfigError::Parse(e) => write!(f, "Failed to parse {CONFIG_FILE}: {e}"),
            ConfigError::DuplicatePort(port) => {
                write!(f, "Duplicate port {port} in config.yaml — each server needs a unique port")
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

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct ServerConfig {
    pub ip: String,
    pub port: u16,
    pub web_folder: String,
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
        if Path::new(folder).exists() {
            return Ok(());
        }
        warn!(folder = %folder, "Web folder missing, creating with default index.html");
        fs::create_dir_all(folder)?;

        let html = format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>z-web | :{port}</title>
  <style>
    *{{box-sizing:border-box;margin:0;padding:0}}
    body{{font-family:'Courier New',monospace;background:#0d0d0d;color:#00ffcc;
         display:flex;align-items:center;justify-content:center;min-height:100vh}}
    .card{{border:1px solid #00ffcc33;border-radius:8px;padding:2rem 3rem;
           text-align:center;background:#111}}
    h1{{font-size:1.8rem;margin-bottom:.5rem}}
    p{{color:#888;margin-top:.5rem;font-size:.9rem}}
    .badge{{display:inline-block;margin-top:1rem;padding:.25rem .75rem;
            border:1px solid #00ffcc55;border-radius:4px;font-size:.75rem;color:#00ffcc99}}
  </style>
</head>
<body>
  <div class="card">
    <h1>⚡ z-web</h1>
    <p>Serving <b>{folder}</b> on port <b>{port}</b></p>
    <span class="badge">Hot Reload Active</span>
  </div>
  <script>
    (function(){{
      var ws=new WebSocket('ws://'+location.host+'/livereload');
      ws.onmessage=function(e){{if(e.data==='reload')location.reload();}};
      ws.onclose=function(){{setTimeout(function(){{location.reload();}},2000);}};
    }})();
  </script>
</body>
</html>
"#,
            port = self.port,
            folder = folder,
        );

        fs::write(format!("{folder}/index.html"), html)?;

        info!(folder = %folder, port = self.port, "Created default index.html");
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub servers: Vec<ServerConfig>,
}

impl Config {
    /// Load config exclusively from config.yaml.
    /// If it doesn't exist, create it with defaults.
    pub fn load() -> Result<Self, ConfigError> {
        if !Path::new(CONFIG_FILE).exists() {
            info!(file = %CONFIG_FILE, "No config found, creating default");
            fs::write(CONFIG_FILE, DEFAULT_CONFIG_YAML)?;
        }

        info!(file = %CONFIG_FILE, "Loading config");
        let content = fs::read_to_string(CONFIG_FILE)?;
        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Reload config from config.yaml (used by hot-reload path).
    pub fn reload() -> Result<Self, ConfigError> {
        let content = fs::read_to_string(CONFIG_FILE)?;
        let config: Config = serde_yaml::from_str(&content)?;
        config.validate()?;
        Ok(config)
    }

    /// Reject configs with duplicate ports before they ever reach the server spawner.
    fn validate(&self) -> Result<(), ConfigError> {
        let mut seen = std::collections::HashSet::new();
        for server in &self.servers {
            if !seen.insert(server.port) {
                return Err(ConfigError::DuplicatePort(server.port));
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
        }
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
}
