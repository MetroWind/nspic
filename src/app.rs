use std::path::{PathBuf, Path};
use std::collections::HashMap;

use log::info;
use log::error as log_error;
use tera::Tera;
use warp::{Filter, Reply};
use warp::http::status::StatusCode;
use warp::reply::Response;
use base64::engine::Engine;

use crate::error;
use crate::error::Error;
use crate::config::Configuration;

static BASE64: &base64::engine::general_purpose::GeneralPurpose =
    &base64::engine::general_purpose::STANDARD;
static BASE64_NO_PAD: &base64::engine::general_purpose::GeneralPurpose =
    &base64::engine::general_purpose::STANDARD_NO_PAD;
static TOKEN_COOKIE: &str = "metube-token";

trait ToResponse
{
    fn toResponse(self) -> Response;
}

impl ToResponse for Result<String, Error>
{
    fn toResponse(self) -> Response
    {
        match self
        {
            Ok(s) => warp::reply::html(s).into_response(),
            Err(e) => {
                log_error!("{}", e);
                e.into_response()
            },
        }
    }
}

impl ToResponse for Result<Response, Error>
{
    fn toResponse(self) -> Response
    {
        match self
        {
            Ok(s) => s,
            Err(e) => {
                log_error!("{}", e);
                e.into_response()
            }
        }
    }
}

// fn validateSession(token: &Option<String>, data_manager: &data::Manager,
//                    config: &Configuration) -> Result<bool, Error>
// {
//     if let Some(token) = token
//     {
//         data_manager.expireSessions(config.session_life_time_sec)?;
//         data_manager.hasSession(&token)?;
//         Ok(true)
//     }
//     else
//     {
//         Ok(false)
//     }
// }

fn handleIndex(templates: &Tera, config: &Configuration) -> Result<Response, Error>
{
    Ok(warp::reply::html("Ok").into_response())
}

fn urlFor(name: &str, arg: &str) -> String
{
    match name
    {
        "index" => String::from("/"),
        _ => String::from("/"),
    }
}

fn getTeraFuncArgs(args: &HashMap<String, tera::Value>, arg_name: &str) ->
    tera::Result<String>
{
    let value = args.get(arg_name);
    if value.is_none()
    {
        return Err(format!("Argument {} not found in function call.", arg_name)
                   .into());
    }
    let value: String = tera::from_value(value.unwrap().clone())?;
    Ok(value)
}

fn makeURLFor(serve_path: String) -> impl tera::Function
{
    move |args: &HashMap<String, tera::Value>| ->
        tera::Result<tera::Value> {
            let path_prefix: String = if serve_path == "" || serve_path == "/"
            {
                String::new()
            }
            else if serve_path.starts_with("/")
            {
                serve_path.to_owned()
            }
            else
            {
                String::from("/") + &serve_path
            };

            let name = getTeraFuncArgs(args, "name")?;
            let arg = getTeraFuncArgs(args, "arg")?;
            Ok(tera::to_value(path_prefix + &urlFor(&name, &arg)).unwrap())
    }
}

pub struct App
{
    templates: Tera,
    config: Configuration,
}

impl App
{
    pub fn new(config: Configuration) -> Result<Self, Error>
    {
        let mut result = Self {
            templates: Tera::default(),
            config,
        };
        result.init()?;
        Ok(result)
    }

    fn init(&mut self) -> Result<(), Error>
    {
        Ok(())
    }

    pub async fn serve(self) -> Result<(), Error>
    {
        let static_dir = PathBuf::from(&self.config.static_dir);
        info!("Static dir is {}", static_dir.display());
        let statics = warp::get().and(warp::path("static"))
            .and(warp::fs::dir(static_dir));
        // let statics = statics.or(warp::get().and(warp::path("video")).and(
        //     warp::fs::dir(PathBuf::from(&self.config.video_dir))));

        let temp = self.templates.clone();
        let config = self.config.clone();
        let index = warp::get().and(warp::path::end()).map(move || {
            handleIndex(&temp, &config).toResponse()
        });

        let route = if self.config.serve_under_path == String::from("/") ||
            self.config.serve_under_path.is_empty()
        {
            statics.or(index).boxed()
        }
        else
        {
            let mut segs = self.config.serve_under_path.split('/');
            if self.config.serve_under_path.starts_with("/")
            {
                segs.next();
            }
            let first: String = segs.next().unwrap().to_owned();
            let mut r = warp::path(first).boxed();
            for seg in segs
            {
                r = r.and(warp::path(seg.to_owned())).boxed();
            }
            r.and(statics.or(index)).boxed()
        };

        info!("Listening at {}:{}...", self.config.listen_address,
              self.config.listen_port);

        warp::serve(route).run(
            std::net::SocketAddr::new(
                self.config.listen_address.parse().map_err(
                    |_| rterr!("Invalid listen address: {}",
                               self.config.listen_address))?,
                self.config.listen_port)).await;
        Ok(())
    }
}
