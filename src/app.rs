use std::path::{PathBuf, Path};
use std::collections::HashMap;

use log::{info, debug};
use log::error as log_error;
use tera::Tera;
use time::OffsetDateTime;
use warp::{Filter, Reply};
use warp::http::status::StatusCode;
use warp::reply::Response;
use base64::engine::Engine;
use futures_util::TryStreamExt;

use crate::error;
use crate::error::Error;
use crate::config::Configuration;
use crate::data;
use crate::post::{Image, Post};
use crate::post_pipeline::{UploadingImage, RawImage, uploadPart};

static BASE64: &base64::engine::general_purpose::GeneralPurpose =
    &base64::engine::general_purpose::STANDARD;
static BASE64_NO_PAD: &base64::engine::general_purpose::GeneralPurpose =
    &base64::engine::general_purpose::STANDARD_NO_PAD;
static TOKEN_COOKIE: &str = "nspic-token";

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

fn handleIndex(templates: &Tera, params: &HashMap<String, String>,
               data_manager: &data::Manager,
               config: &Configuration) -> Result<Response, Error>
{
    let start: u64 = if let Some(index) =  params.get("start")
    {
        index.parse().map_err(|_| rterr!("Invalid parameter"))?
    }
    else
    {
        0
    };
    let posts = data_manager.getPosts(start, 50, data::PostOrder::NewFirst)?;
    let mut context = tera::Context::new();
    context.insert("posts", &posts);
    context.insert("site_info", &config.site_info);
    let html = templates.render("index.html", &context).map_err(
        |e| rterr!("Failed to render template: {}", e))?;
    Ok(warp::reply::html(html).into_response())
}

fn handleUploadPage(templates: &Tera, config: &Configuration) ->
    Result<Response, Error>
{
    let mut context = tera::Context::new();
    context.insert("site_info", &config.site_info);
    let html = templates.render("upload.html", &context).map_err(
        |e| rterr!("Failed to render template: {}", e))?;
    Ok(warp::reply::html(html).into_response())
}

enum UploadPart
{
    Desc(String),
    Image(RawImage),
}

async fn handleUpload(form_data: warp::multipart::FormData,
                      data_manager: &data::Manager,
                      config: &Configuration) ->
    Result<String, warp::Rejection>
{
    let mut desc = String::new();
    let parts: Vec<_> = form_data.and_then(
        |part| async move {
            debug!("Got part: {}, {}, {}", part.name(),
                   part.filename().or(Some("<no filename>")).unwrap(),
                   part.content_type().or(Some("<no content type>")).unwrap());
            let p: Result<UploadPart, Error> = match part.name()
            {
                "Desc" => {
                    match uploadPart(part).await
                    {
                        Ok(data) => String::from_utf8(data)
                            .map(|s| UploadPart::Desc(s))
                            .map_err(|_| rterr!("Invalid description")),
                        Err(e) => Err(e),
                    }
                },
                "FileToUpload" => {
                    let img = UploadingImage { part };
                    let img = img.saveToTemp(config).await.map(
                        |i| UploadPart::Image(i));
                    img
                },
                _ => Err(rterr!("Unrecognized part: {}", part.name()))
            };
            // p is a Result<_, error::Error>. But this async stream
            // thing requires a Result<_, warp::Error>. So here we
            // just wrap an extra layer of Result<_, warp::Error>.
            // Later we will just unwrap it.
            Ok(p)
        }).try_collect().await
        // Unwrap the Result<_, warp::Error> here.
        .unwrap();

    let mut images: Vec<Image> = Vec::new();
    for part in parts
    {
        let part = part.map_err(error::reject)?;
        match part
        {
            UploadPart::Desc(s) => {desc = s;},
            UploadPart::Image(img) => {
                let image = img.moveToLibrary(config).map_err(error::reject)?
                    .makeRelativePath(config).map_err(error::reject)?
                    .probeMetadata(config).map_err(error::reject)?
                    .generateThumbnail(config).map_err(error::reject)?;
                images.push(image);
            }
        }
    }
    let mut post = Post::new();
    post.desc = desc;
    post.upload_time = OffsetDateTime::now_utc();
    post.images = images;
    // post.album_id = ???;
    data_manager.addPost(&post, None).map_err(error::reject)?;
    Ok::<_, warp::Rejection>(String::from("OK"))
}

fn urlFor(name: &str, arg: &str) -> String
{
    match name
    {
        "index" => String::from("/"),
        "upload" => String::from("/upload"),
        "static" => String::from("/static/") + arg,
        "image_file" => String::from("/image/") + arg,
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
    data_manager: data::Manager,
    config: Configuration,
}

impl App
{
    pub fn new(config: Configuration) -> Result<Self, Error>
    {
        let db_path = Path::new(&config.data_dir).join("db.sqlite");
        let mut result = Self {
            templates: Tera::default(),
            data_manager: data::Manager::newWithFilename(&db_path),
            config,
        };
        result.init()?;
        Ok(result)
    }

    fn init(&mut self) -> Result<(), Error>
    {
        self.data_manager.connect()?;
        self.data_manager.init()?;
        let template_path = PathBuf::from(&self.config.data_dir)
            .join("templates").canonicalize()
            .map_err(|_| rterr!("Invalid template dir"))?
            .join("**").join("*");
        info!("Template dir is {}", template_path.display());
        let template_dir = template_path.to_str().ok_or_else(
                || rterr!("Invalid template path"))?;
        self.templates = Tera::new(template_dir).map_err(
            |e| rterr!("Failed to compile templates: {}", e))?;
        self.templates.register_function(
            "url_for", makeURLFor(self.config.serve_under_path.clone()));

        if !Path::new(&self.config.data_dir).exists()
        {
            std::fs::create_dir_all(&self.config.data_dir)
                .map_err(|e| rterr!("Failed to create data dir: {}", e))?;
        }
        if !Path::new(&self.config.image_dir).exists()
        {
            std::fs::create_dir_all(&self.config.image_dir)
                .map_err(|e| rterr!("Failed to create image dir: {}", e))?;
        }
        Ok(())
    }

    pub async fn serve(self) -> Result<(), Error>
    {
        let static_dir = PathBuf::from(&self.config.static_dir);
        info!("Static dir is {}", static_dir.display());
        let statics = warp::get().and(warp::path("static"))
            .and(warp::fs::dir(static_dir));
        let statics = statics.or(warp::get().and(warp::path("image")).and(
            warp::fs::dir(PathBuf::from(&self.config.image_dir))));

        let temp = self.templates.clone();
        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let index = warp::get().and(warp::query::<HashMap<String, String>>())
            .and(warp::path::end()).map(move |query: HashMap<String, String>| {
            handleIndex(&temp, &query, &data_manager, &config).toResponse()
        });

        let temp = self.templates.clone();
        let config = self.config.clone();
        let upload_page = warp::get().and(warp::path("upload"))
            .and(warp::path::end()).map(move || {
            handleUploadPage(&temp, &config).toResponse()
        });

        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let upload = warp::post().and(warp::path("upload"))
            .and(warp::path::end())
            // .and(warp::filters::cookie::optional(TOKEN_COOKIE))
            .and(warp::multipart::form().max_length(self.config.upload_bytes_max))
            .and_then(move |data: warp::multipart::FormData| {
                let config = config.clone();
                let data_manager = data_manager.clone();
                async move {
                    handleUpload(data, &data_manager, &config).await
                }
            });

        let route = if self.config.serve_under_path == String::from("/") ||
            self.config.serve_under_path.is_empty()
        {
            statics.or(index).or(upload_page).or(upload).boxed()
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
            r.and(statics.or(index).or(upload_page).or(upload)).boxed()
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
