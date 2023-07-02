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
fn defaultDataDir() -> String { String::from("test") }
fn defaultImageDir() -> String { String::from("test") }
fn defaultUploadBytesMax() -> u64 { 1024 * 1024 * 100 }
fn defaultImagePixelSize() -> u32 { 1280 }

fn defaultSiteTitle() -> String { String::from("NSPic") }
fn defaultFootnote() -> String { String::new() }
fn defaultUrlDomain() -> String { String::from("http://example.org") }

#[derive(Deserialize, Serialize, Clone)]
pub struct SiteInfo
{
    #[serde(default = "defaultSiteTitle")]
    pub site_title: String,
    #[serde(default = "defaultFootnote")]
    pub footnote: String,
    /// The beginning part of the URL of the website, including only
    /// the protocol and domain, without the trailing slash. This is
    /// only used in the OGP metadata. Example: http://example.org.
    #[serde(default = "defaultUrlDomain")]
    pub url_domain: String,
}

impl Default for SiteInfo
{
    fn default() -> Self
    {
        Self {
            site_title: defaultSiteTitle(),
            footnote: defaultFootnote(),
            url_domain: defaultUrlDomain(),
        }
    }
}

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
    #[serde(default = "defaultDataDir")]
    pub data_dir: String,
    #[serde(default = "defaultUploadBytesMax")]
    pub upload_bytes_max: u64,
    #[serde(default = "defaultImageDir")]
    pub image_dir: String,
    #[serde(default = "defaultImagePixelSize")]
    pub image_pixel_size: u32,
    pub site_info: SiteInfo,
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
            data_dir: defaultDataDir(),
            upload_bytes_max: defaultUploadBytesMax(),
            image_dir: defaultImageDir(),
            image_pixel_size: defaultImagePixelSize(),
            site_info: SiteInfo::default(),
        }
    }
}
