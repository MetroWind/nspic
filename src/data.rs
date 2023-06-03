use std::path::{Path, PathBuf};
use std::str::FromStr;

use log::info;
use rusqlite as sql;
use rusqlite::OptionalExtension;
use time::OffsetDateTime;

use crate::error;
use crate::error::Error as Error;
use crate::post::{Image, Post};
use crate::sqlite_connection;

#[derive(Clone)]
pub struct Manager
{
    filename: sqlite_connection::Source,
    connection: Option<r2d2::Pool<sqlite_connection::Manager>>,
}

impl Manager
{
    #[allow(dead_code)]
    pub fn new(f: sqlite_connection::Source) -> Self
    {
        Self { filename: f, connection: None }
    }

    pub fn newWithFilename<P: AsRef<Path>>(f: P) -> Self
    {
        Self {
            filename: sqlite_connection::Source::File(
                std::path::PathBuf::from(f.as_ref())),
            connection: None,
        }
    }

    fn confirmConnection(&self) ->
        Result<r2d2::PooledConnection<sqlite_connection::Manager>, Error>
    {
        if let Some(pool) = &self.connection
        {
            pool.get().map_err(|e| rterr!("Failed to get connection: {}", e))
        }
        else
        {
            Err(error!(DataError, "Sqlite database not connected"))
        }
    }

    /// Connect to the database. Create database file if not exist.
    pub fn connect(&mut self) -> Result<(), Error>
    {
        let manager = match &self.filename
        {
            sqlite_connection::Source::File(path) => {
                info!("Opening database at {:?}...", path);
                sqlite_connection::Manager::file(path)
            },
            sqlite_connection::Source::Memory =>
                sqlite_connection::Manager::memory(),
        };
        self.connection = Some(r2d2::Pool::new(manager).map_err(
            |e| rterr!("Failed to create connection pool: {}", e))?);
        Ok(())
    }

    pub fn init(&self) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS albums (
             id INTEGER PRIMARY KEY ASC,
             title TEXT
             );", []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS posts (
             id TEXT PRIMARY KEY,
             desc TEXT,
             upload_time INTEGER,
             FOREIGN KEY(album) REFERENCES albums(id)
             );", []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS images (
             id INTEGER PRIMARY KEY ASC,
             path TEXT,
             width INTEGER,
             height INTEGER,
             FOREIGN KEY(post) REFERENCES posts(id)
             );", []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
             token TEXT PRIMARY KEY,
             auth_time INTEGER
             );", []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;
        Ok(())
    }

    fn newPost(post: &Post, album_id: Option<i64>) -> Result<i64, Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "INSERT INTO videos (id, desc, upload_time, album)
             VALUES (?, ?, ?, ?);", sql::params![
                 &img.id,
                 &img.path.to_str().ok_or_else(
                     || rterr!("Invalid video path: {:?}", vid.path))?,
                 &img.desc,
                 img.upload_time.unix_timestamp(),
                 album_id,
             ]).map_err(|e| error!(DataError, "Failed to add image: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid insert happened"));
        }
        Ok(())
    }

    fn newImage(img: &Image, post_id: i64) -> Result<(), Error>
    {


    fn row2Video(row: &sql::Row) -> sql::Result<Video>
    {
        let time_value = row.get(6)?;
        let path: String = row.get(1)?;
        let ext: String = row.get(7)?;
        Ok(Video {
            id: row.get(0)?,
            path: PathBuf::from_str(&path).unwrap(),
            title: row.get(2)?,
            desc: row.get(3)?,
            artist: row.get(4)?,
            views: row.get(5)?,
            upload_time: time::OffsetDateTime::from_unix_timestamp(
                time_value).map_err(
                |_| sql::Error::IntegralValueOutOfRange(
                    6, time_value))?,
            container_type: ContainerType::fromExtension(&ext)
                .ok_or_else(|| sql::Error::FromSqlConversionFailure(
                    7, sql::types::Type::Text,
                    Box::new(rterr!("Invalid extension name from database: {}",
                                    ext))))?,
            original_filename: row.get(8)?,
            duration: time::Duration::seconds_f64(row.get(9)?),
            thumbnail_path: row.get::<_, Option<String>>(10)?.map(
                |s| PathBuf::from_str(&s).unwrap()),
        })
    }

    pub fn addVideo(&self, vid: &Video) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "INSERT INTO videos (id, path, title, desc, artist, views,
                                 upload_time, container_type, original_filename,
                                 duration, thumbnail_path)
             VALUES (?, ?, ?, ?, ?, 0, ?, ?, ?, ?, ?);", sql::params![
                 &vid.id,
                 &vid.path.to_str().ok_or_else(
                     || rterr!("Invalid video path: {:?}", vid.path))?,
                 &vid.title,
                 &vid.desc,
                 &vid.artist,
                 vid.upload_time.unix_timestamp(),
                 vid.container_type.toExtension(),
                 &vid.original_filename,
                 vid.duration.as_seconds_f64(),
                 &vid.thumbnail_path.as_ref().map(|p| p.to_str().unwrap()),

             ]).map_err(|e| error!(DataError, "Failed to add video: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid insert happened"));
        }
        Ok(())
    }

    pub fn findVideoByID(&self, id: &str) -> Result<Option<Video>, Error>
    {
        let conn = self.confirmConnection()?;
        conn.query_row("SELECT id, path, title, desc, artist, views,
                        upload_time, container_type, original_filename, duration,
                        thumbnail_path
                        FROM videos WHERE id=?;",
                       sql::params![id], Self::row2Video)

            .optional().map_err(
                |e| error!(DataError, "Failed to look up video {}: {}", id, e))
    }

    pub fn increaseViewCount(&self, id: &str) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "UPDATE videos SET views = views + 1 WHERE id=?;",
            sql::params![id]).map_err(|e| error!(
                DataError, "Failed to increase view count for video {}: {}",
                id, e))?;
        if row_count != 1
        {
            return Err(error!(
                DataError,
                "Failed to increase view count for video {}: \
                 number of affected rows: {} != 1.", id, row_count));
        }
        Ok(())
    }

    /// Retrieve “count” number of videos, starting from the entry at
    /// index “start_index”. Index is 0-based. Returned entries are
    /// sorted from new to old.
    pub fn getVideos(&self, start_index: u64, count: u64, order: VideoOrder) ->
        Result<Vec<Video>, Error>
    {
        let conn = self.confirmConnection()?;

        let order_expr = match order
        {
            VideoOrder::NewFirst => "ORDER BY upload_time DESC",
        };

        let mut cmd = conn.prepare(
            &format!("SELECT id, path, title, desc, artist, views, upload_time,
                      container_type, original_filename, duration,
                      thumbnail_path
                      FROM videos {} LIMIT ? OFFSET ?;", order_expr))
            .map_err(|e| error!(
                DataError,
                "Failed to compare statement to get videos: {}", e))?;
        let rows = cmd.query_map([count, start_index], Self::row2Video).map_err(
            |e| error!(DataError, "Failed to retrieve videos: {}", e))?.map(
            |row| row.map_err(|e| error!(DataError, "{}", e)));
        rows.collect()
    }

    pub fn createSession(&self, token: &str) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "INSERT INTO sessions (token, auth_time)
             VALUES (?, ?);", sql::params![
                 token,
                 OffsetDateTime::now_utc().unix_timestamp(),
             ]).map_err(|e| error!(DataError, "Failed to create session: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid insert happened"));
        }
        Ok(())
    }

    /// Return time of authentication of the token.
    pub fn hasSession(&self, token: &str) -> Result<OffsetDateTime, Error>
    {
        let conn = self.confirmConnection()?;
        let mut cmd = conn.prepare(
            "SELECT auth_time FROM sessions WHERE token=?;")
            .map_err(|e| error!(
                DataError,
                "Failed to prepare statement to lookup session: {}", e))?;
        if let Some(auth_time_sec) = cmd.query_row([token,], |row| row.get(0))
            .optional().map_err(
                |e| error!(DataError, "Failed to look up session: {}", e))?
        {
            OffsetDateTime::from_unix_timestamp(auth_time_sec).map_err(
                |_| rterr!("Invalid auth time"))
        }
        else
        {
            Err(rterr!("Session not found"))
        }
    }

    pub fn expireSessions(&self, life_time_sec: u64) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let row_count = conn.execute(
            "DELETE FROM sessions WHERE auth_time < ?;",
            sql::params![now as u64 - life_time_sec])
            .map_err(|e| error!(DataError, "Failed to expire sessions: {}", e))?;
        if row_count > 0
        {
            info!("Expired {} sessions.", row_count);
        }
        Ok(())
    }

}
