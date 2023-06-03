use serde::{Deserialize, Serialize};

use crate::error::Error;

fn defaultListenAddr() -> String
{
    String::from("127.0.0.1")
}

fn defaultServePath() -> String
{
    String::from("/")
}

fn defaultListenPort() -> u16 { 8080 }

#[derive(Deserialize, Clone)]
pub struct Configuration
{
    #[serde(default = "defaultListenAddr")]
    pub listen_address: String,
    #[serde(default = "defaultListenPort")]
    pub listen_port: u16,
    /// Must starts with `/`, and does not end with `/`, unless it’s
    /// just `/`.
    #[serde(default = "defaultServePath")]
    pub serve_under_path: String,
    pub static_dir: String,
}

impl Configuration
{
    pub fn fromFile(path: &str) -> Result<Self, Error>
    {
        let content = std::fs::read_to_string(path).map_err(
            |_| rterr!("Failed to read config file at {}", path))?;
        toml::from_str(&content).map_err(
            |_| rterr!("Failed to parse config file"))
    }
}

impl Default for Configuration
{
    fn default() -> Self
    {
        Self {
            listen_address: defaultListenAddr(),
            listen_port: defaultListenPort(),
            serve_under_path: defaultServePath(),
            static_dir: String::from("static"),
        }
    }
}
