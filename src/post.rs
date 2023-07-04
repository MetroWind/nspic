use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::ser::{Serializer, SerializeStruct};
use time::OffsetDateTime;

use crate::error::Error;

pub struct Image
{
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
}

impl Image
{
    pub fn thumbnail(&self) -> Result<PathBuf, Error>
    {
        let dir = self.path.parent().or(Some(&Path::new(""))).unwrap();
        let stem = self.path.file_stem().ok_or_else(
            || rterr!("Invalid image path: {}", self.path.display()))?
            .to_str().ok_or_else(
                || rterr!("Invalid image path: {}", self.path.display()))?;
        let ext = self.path.extension().or(Some(std::ffi::OsStr::new("")))
            .unwrap();
        Ok(dir.to_owned().join(Path::new(&(String::from(stem) + "_t")))
             .with_extension(ext))
    }
}

impl Serialize for Image
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Image", 4)?;
        state.serialize_field("path", self.path.to_str().ok_or_else(
            || serde::ser::Error::custom("Invalid image path"))?)?;
        state.serialize_field("thumbnail", self.thumbnail().map_err(
            |e| serde::ser::Error::custom(e))?.to_str().ok_or_else(
            || serde::ser::Error::custom("Invalid thumbnail path"))?)?;
        state.serialize_field("width", &self.width)?;
        state.serialize_field("height", &self.height)?;
        state.end()
    }
}

pub struct Post
{
    pub id: i64,
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
            id: 0,
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
        let mut state = serializer.serialize_struct("Post", 5)?;
        state.serialize_field("id", &self.id)?;
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

// ========== Unit tests ============================================>

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn thumnailPath() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut image = Image
        {
            path: PathBuf::from("a").join("bc.jpg"),
            width: 0,
            height: 0,
        };

        assert_eq!(image.thumbnail()?.to_str().unwrap(), "a/bc_t.jpg");

        image.path = PathBuf::from("aaa.jpg");
        assert_eq!(image.thumbnail()?.to_str().unwrap(), "aaa_t.jpg");
        image.path = PathBuf::from("aaa");
        assert_eq!(image.thumbnail()?.to_str().unwrap(), "aaa_t");
        Ok(())
    }
}
