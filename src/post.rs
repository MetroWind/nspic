use std::path::PathBuf;

use serde::Serialize;
use serde::ser::{Serializer, SerializeStruct};

use time::OffsetDateTime;

#[derive(Serialize)]
pub struct Image
{
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub struct Post
{
    pub images: Vec<Image>,
    pub desc: String,
    pub upload_time: OffsetDateTime,
    pub album_id: Option<i64>,
}

impl Post
{
    pub fn new() -> Self
    {
        Self {
            images: Vec::new(),
            desc: String::new(),
            upload_time: OffsetDateTime::UNIX_EPOCH,
            album_id: None,
        }
    }
}

impl Serialize for Post
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Video", 4)?;
        state.serialize_field("images", &self.images)?;
        state.serialize_field("desc", &self.desc)?;
        state.serialize_field("upload_time",
                              &self.upload_time.unix_timestamp())?;
        let format: Vec<time::format_description::FormatItem> =
            time::format_description::parse(
                "[year]-[month]-[day] [hour]:[minute]:[second] UTC").unwrap();
        state.serialize_field(
            "upload_time_utc_str", &self.upload_time.format(&format).map_err(
                |_| serde::ser::Error::custom("Invalid upload time"))?)?;
        state.serialize_field("album_id", &self.album_id)?;
        state.end()
    }
}
