use warp::Reply;
use warp::reply::Response;
use log::error as log_error;

use crate::error::Error;

pub trait ToResponse
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
