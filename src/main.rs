#![allow(non_snake_case)]

#[macro_use]
mod error;
mod config;
mod post;
mod sqlite_connection;
mod data;
mod app;
mod post_pipeline;

use std::path::Path;

use log::warn;

use error::Error;
use config::Configuration;

fn main() -> Result<(), Error>
{
    env_logger::Builder::from_default_env().format_timestamp(None).init();
    let opts = clap::Command::new("MeTube")
        .about("A naively simple self-hosted video hosting service")
        .arg(clap::Arg::new("config")
             .long("config")
             .short('c')
             .value_name("FILE")
             .default_value("/etc/metube.toml")
             .help("Path of config file."))
        .get_matches();

    let config_path = opts.get_one::<String>("config").unwrap();
    let config = if Path::new(&config_path).exists()
    {
        Configuration::fromFile(&config_path)?
    }
    else
    {
        warn!("Config file not found. Using default config...");
        Configuration::default()
    };

    let a = app::App::new(config)?;
    tokio::runtime::Runtime::new().unwrap().block_on(a.serve())?;
    Ok(())
}
