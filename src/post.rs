use std::path::PathBuf;

use time::OffsetDateTime;

pub struct Image
{
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub struct Post
{
    pub id: String,
    pub images: Vec<Image>,
    pub desc: String,
    pub upload_time: OffsetDateTime,
    pub album_id: i64,
}
