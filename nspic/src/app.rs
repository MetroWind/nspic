use std::path::{PathBuf, Path};
use std::collections::HashMap;

use log::{info, debug};
use log::error as log_err;
use tera::Tera;
use time::OffsetDateTime;
use warp::{Filter, Reply};
use warp::http::status::StatusCode;
use warp::reply::Response;
use futures_util::TryStreamExt;
use serde_json::json;

use crate::error;
use crate::error::Error;
use crate::config::Configuration;
use crate::data;
use crate::post::{Image, Post};
use crate::utils::uriFromStr;
use crate::auth::{handleLogin, validateSession, TOKEN_COOKIE};
use crate::to_response::ToResponse;
use crate::post_pipeline::{UploadingImage, RawImage, uploadPart, imagePath};

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
    let page_size = 16;
    let post_count = data_manager.countPosts()?;
    let posts = data_manager.getPosts(
        start, page_size, data::PostOrder::NewFirst)?;
    let mut context = tera::Context::new();
    if post_count > start + page_size
    {
        context.insert("next", &(start + page_size));
    }
    if start > 0
    {
        let prev = if start < page_size
        {
            0
        }
        else
        {
            start - page_size
        };
        context.insert("prev", &prev);
    }
    context.insert("posts", &posts);
    context.insert("site_info", &config.site_info);
    let html = templates.render("index.html", &context).map_err(
        |e| rterr!("Failed to render template: {}", e))?;
    Ok(warp::reply::html(html).into_response())
}

fn handlePost(templates: &Tera, post_id: i64, data_manager: &data::Manager,
              config: &Configuration) -> Result<Response, Error>
{
    let post = data_manager.findPostByID(post_id)?.ok_or_else(
        || Error::HTTPStatus(StatusCode::NOT_FOUND, String::new()))?;
    let mut context = tera::Context::new();
    context.insert("post", &post);
    context.insert("site_info", &config.site_info);
    let html = templates.render("post.html", &context).map_err(
        |e| rterr!("Failed to render template: {}", e))?;
    Ok(warp::reply::html(html).into_response())
}

fn handleFeed(templates: &Tera, data_manager: &data::Manager,
              config: &Configuration) -> Result<Response, Error>
{
    let feed_size = 10;
    let posts = data_manager.getPosts(
        0, feed_size, data::PostOrder::NewFirst)?;
    let mut context = tera::Context::new();
    context.insert("posts", &posts);
    context.insert("site_info", &config.site_info);
    let feed_str = templates.render("atom.xml", &context).map_err(
        |e| rterr!("Failed to render template: {}", e))?;
    Ok(warp::reply::with_header(feed_str, "Content-Type",
                                "application/atom+xml")
       .into_response())
}

fn handleDeleteConfirm(
    templates: &Tera, post_id: i64, data_manager: &data::Manager,
    config: &Configuration, token: Option<String>) -> Result<Response, Error>
{
    if validateSession(&token, data_manager, config)?
    {
        let post = data_manager.findPostByID(post_id)?.ok_or_else(
            || Error::HTTPStatus(StatusCode::NOT_FOUND, String::new()))?;
        let mut context = tera::Context::new();
        context.insert("post", &post);
        context.insert("site_info", &config.site_info);
        let html = templates.render("delete_confirm.html", &context).map_err(
            |e| rterr!("Failed to render template: {}", e))?;
        Ok(warp::reply::html(html).into_response())
    }
    else
    {
        Err(Error::HTTPStatus(StatusCode::UNAUTHORIZED, String::new()))
    }
}

fn handleDelete(post_id: i64, data_manager: &data::Manager,
                config: &Configuration, token: Option<String>) ->
    Result<Response, Error>
{
    if validateSession(&token, data_manager, config)?
    {
        let post = data_manager.findPostByID(post_id)?.ok_or_else(
            || Error::HTTPStatus(StatusCode::NOT_FOUND, String::new()))?;
        info!("Deleting post {}...", post_id);
        data_manager.deletePost(post_id)?;
        for image in post.images
        {
            info!("Deleting image file at {}...", image.path.display());
            std::fs::remove_file(imagePath(&image, config))
                .map_err(|_| rterr!("Failed to delete image file."))?
        }
        Ok(warp::redirect::found(uriFromStr(&config.serve_under_path)?)
           .into_response())
    }
    else
    {
        Err(Error::HTTPStatus(StatusCode::UNAUTHORIZED, String::new()))
    }
}

fn handleUploadPage(data_manager: &data::Manager, templates: &Tera,
                    config: &Configuration, token: Option<String>) ->
    Result<Response, Error>
{
    if validateSession(&token, data_manager, config)?
    {
        let mut context = tera::Context::new();
        context.insert("site_info", &config.site_info);
        let html = templates.render("upload.html", &context).map_err(
            |e| rterr!("Failed to render template: {}", e))?;
        Ok(warp::reply::html(html).into_response())
    }
    else
    {
        Err(Error::HTTPStatus(StatusCode::UNAUTHORIZED, String::new()))
    }
}

enum UploadPart
{
    Desc(String),
    Image(RawImage),
}

fn webhookPayload(post: &Post) -> serde_json::value::Value
{
    let mut payload = json!({
        "desc": post.desc,
        "images": [],
        "url": urlFor("post", &post.id.to_string()),
        "time": post.upload_time.unix_timestamp(),
    });
    for img in &post.images
    {
        payload["images"].as_array_mut().unwrap()
            .push(json!(urlFor("image", img.path.to_str().unwrap())));
    }
    return payload
}

async fn handleUpload(token: Option<String>,
                      form_data: warp::multipart::FormData,
                      data_manager: &data::Manager,
                      config: &Configuration) ->
    Result<String, warp::Rejection>
{
    if !validateSession(&token, data_manager, config).map_err(
        |_| warp::reject::reject())?
    {
        return Err(warp::reject::reject());
    }
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
                let image = img.resize(config).map_err(error::reject)?
                    .makeThumbnail(config).map_err(error::reject)?
                    .moveToLibrary(config).map_err(error::reject)?
                    .makeRelativePath(config).map_err(error::reject)?
                    .probeMetadata(config).map_err(error::reject)?;
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

    // Call webhook
    if let Some(url) = &config.webhook_url
    {
        let payload = serde_json::to_vec(&webhookPayload(&post));
        if let Err(e) = payload
        {
            log_err!("Invalid payload: {}.", e);
        }
        else
        {
            let result = ureq::post(url).set("Content-Type", "application/json")
                .send_bytes(&payload.unwrap());
            if let Err(e) = result
            {
                log_err!("Webhook failed: {}.", e);
            }
            else
            {
                let status = result.unwrap().status();
                if status < 200 || status >= 300
                {
                    log_err!("Webhook failed with status {}.", status);
                }
            }
        }
    }

    Ok::<_, warp::Rejection>(String::from("Ok"))
}

fn urlFor(name: &str, arg: &str) -> String
{
    match name
    {
        "index" => String::from("/"),
        "upload" => String::from("/upload"),
        "post" => String::from("/p/") + arg,
        "feed" => String::from("/feed.xml"),
        "delete_confirm" => String::from("/delete-confirm/") + arg,
        "delete" => String::from("/delete/") + arg,
        "login" => String::from("/login/"),
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
        let data_manager = self.data_manager.clone();
        let post = warp::get().and(warp::path("p")).and(warp::path::param())
            .and(warp::path::end()).map(move |id: i64| {
            handlePost(&temp, id, &data_manager, &config).toResponse()
        });

        let temp = self.templates.clone();
        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let feed = warp::get().and(warp::path("feed.xml"))
            .and(warp::path::end()).map(move || {
            handleFeed(&temp, &data_manager, &config).toResponse()
        });

        let temp = self.templates.clone();
        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let delete_confirm = warp::get().and(warp::path("delete-confirm"))
            .and(warp::path::param()).and(warp::path::end())
            .and(warp::filters::cookie::optional(TOKEN_COOKIE))
            .map(move |id: i64, token: Option<String>| {
                handleDeleteConfirm(&temp, id, &data_manager, &config, token)
                    .toResponse()
            });

        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let delete = warp::post().and(warp::path("delete"))
            .and(warp::path::param()).and(warp::path::end())
            .and(warp::filters::cookie::optional(TOKEN_COOKIE))
            .map(move |id: i64, token: Option<String>| {
                handleDelete(id, &data_manager, &config, token).toResponse()
            });

        let temp = self.templates.clone();
        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let upload_page = warp::get().and(warp::path("upload"))
            .and(warp::path::end())
            .and(warp::filters::cookie::optional(TOKEN_COOKIE)).map(
                move |token: Option<String>|
                handleUploadPage(&data_manager, &temp, &config, token)
                    .toResponse());

        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let upload = warp::post().and(warp::path("upload"))
            .and(warp::path::end())
            .and(warp::filters::cookie::optional(TOKEN_COOKIE))
            .and(warp::multipart::form()
                 .max_length(self.config.upload_bytes_max))
            .and_then(
                move |token: Option<String>, data: warp::multipart::FormData| {
                let config = config.clone();
                let data_manager = data_manager.clone();
                async move {
                    handleUpload(token, data, &data_manager, &config).await
                }
            });

        let config = self.config.clone();
        let data_manager = self.data_manager.clone();
        let login = warp::get().and(warp::path("login")).and(warp::path::end())
            .and(warp::header::optional::<String>("Authorization"))
            .map(move |auth_value: Option<String>| {
                handleLogin(auth_value, &data_manager, &config).toResponse()
            });

        let bare_route = statics.or(index).or(post).or(feed).or(delete_confirm)
            .or(delete).or(upload_page).or(upload).or(login);
        let route = if self.config.serve_under_path == String::from("/") ||
            self.config.serve_under_path.is_empty()
        {
            bare_route.boxed()
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
            r.and(bare_route).boxed()
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
