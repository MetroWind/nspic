use std::error::Error as StdError;
use std::fmt;

use log::error as log_error;
use warp::http::status::StatusCode;

#[macro_export]
macro_rules! error
{
    ( $err_type:ident, $msg:literal ) =>
    {
        {
            Error::$err_type(String::from($msg))
        }
    };
    ( $err_type:ident, $msg:literal $(, $x:expr)+) =>
    {
        {
            Error::$err_type(format!($msg $(, $x)+))
        }
    };
}

// Construct a RuntimeError
#[macro_export]
macro_rules! rterr
{
    ($msg:literal $(, $x:expr)*) =>
    {
        error!(RuntimeError, $msg $(, $x)*)
    };
}

#[derive(Debug, Clone)]
pub enum Error
{
    /// An error from the underlying data source. This could be a
    /// database connection issue, or disk I/O failure, or invalid
    /// data from the data source, etc. This is not a “logic error”
    /// such as an error from generating SQL statement due to invalid
    /// backlinks.
    DataError(String),
    RuntimeError(String),
    HTTPStatus(StatusCode, String),
}

pub fn reject(e: Error) -> warp::Rejection
{
    log_error!("{}", e);
    warp::reject::reject()
}

impl fmt::Display for Error
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        match self
        {
            Error::DataError(msg) => write!(f, "Data error: {}", msg),
            Error::RuntimeError(msg) => write!(f, "Runtime error: {}", msg),
            Error::HTTPStatus(c, msg) =>
                write!(f, "HTTP status with code {}: {}", c, msg),
        }
    }
}

impl StdError for Error
{
    fn source(&self) -> Option<&(dyn StdError + 'static)> {None}
}

impl warp::reply::Reply for Error
{
    fn into_response(self) -> warp::reply::Response
    {
        match self
        {
            Error::DataError(_) => warp::reply::with_status(
                self.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                .into_response(),
            Error::RuntimeError(_) => warp::reply::with_status(
                self.to_string(), StatusCode::INTERNAL_SERVER_ERROR)
                .into_response(),
            Error::HTTPStatus(c, _) => warp::reply::with_status(
                self.to_string(), c).into_response(),
        }
    }
}
