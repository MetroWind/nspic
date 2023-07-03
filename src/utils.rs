use crate::error;
use crate::error::Error;

pub fn uriFromStr(s: &str) -> Result<warp::http::uri::Uri, Error>
{
    s.parse::<warp::http::uri::Uri>().map_err(|_| rterr!("Invalid URI: {}", s))
}
