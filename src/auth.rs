use warp::http::status::StatusCode;
use warp::Reply;
use warp::reply::Response;
use base64::engine::Engine;

use crate::error::Error;
use crate::config::Configuration;
use crate::data;
use crate::utils::uriFromStr;

static BASE64: &base64::engine::general_purpose::GeneralPurpose =
    &base64::engine::general_purpose::STANDARD;
static BASE64_NO_PAD: &base64::engine::general_purpose::GeneralPurpose =
    &base64::engine::general_purpose::STANDARD_NO_PAD;
pub static TOKEN_COOKIE: &str = "nspic-token";

fn createToken() -> String
{
    BASE64_NO_PAD.encode(rand::random::<i128>().to_ne_bytes())
}

fn makeCookie(token: String, session_life_time: u64) -> String
{
    format!("{}={}; Max-Age={}; Path=/", TOKEN_COOKIE, token, session_life_time)
}

pub fn validateSession(token: &Option<String>, data_manager: &data::Manager,
                   config: &Configuration) -> Result<bool, Error>
{
    if let Some(token) = token
    {
        data_manager.expireSessions(config.session_life_time_sec)?;
        data_manager.hasSession(&token)?;
        Ok(true)
    }
    else
    {
        Ok(false)
    }
}

pub fn handleLogin(
    auth_value_maybe: Option<String>, data_manager: &data::Manager,
    config: &Configuration) -> Result<Response, Error>
{
    if let Some(auth_value) = auth_value_maybe
    {
        if !auth_value.starts_with("Basic ")
        {
            return Err(Error::HTTPStatus(
                StatusCode::UNAUTHORIZED,
                "Not using basic authentication".to_owned()));
        }
        let expeced = BASE64.encode(format!("user:{}", config.password));
        if expeced.as_str() == &auth_value[6..]
        {
            // Authentication is good.
            let token = createToken();
            data_manager.createSession(&token)?;
            return Ok(warp::reply::with_header(
                warp::redirect::found(uriFromStr(&config.serve_under_path)?),
                "Set-Cookie", makeCookie(token, config.session_life_time_sec))
                      .into_response());
        }
        else
        {
            return Err(Error::HTTPStatus(
                StatusCode::UNAUTHORIZED,
                "Invalid credential".to_owned()));
        }
    }

    Ok(warp::reply::with_header(
        warp::reply::with_status(warp::reply::reply(), StatusCode::UNAUTHORIZED),
        "WWW-Authenticate",
        r#"Basic realm="nspic", charset="UTF-8""#).into_response())
}
