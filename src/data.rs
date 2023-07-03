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

pub enum PostOrder { NewFirst, }

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
             id INTEGER PRIMARY KEY ASC,
             desc TEXT,
             upload_time INTEGER,
             album INTEGER,
             FOREIGN KEY(album) REFERENCES albums(id)
             );", []).map_err(
            |e| error!(DataError, "Failed to create table: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS images (
             id INTEGER PRIMARY KEY ASC,
             path TEXT,
             width INTEGER,
             height INTEGER,
             post id,
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

    pub fn addPost(&self, post: &Post, album_id: Option<i64>) -> Result<i64, Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "INSERT INTO posts (desc, upload_time, album)
             VALUES (?, ?, ?);", sql::params![
                 &post.desc,
                 post.upload_time.unix_timestamp(),
                 album_id,
             ]).map_err(|e| error!(DataError, "Failed to add image: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid insert happened"));
        }
        let id = conn.last_insert_rowid();
        for img in &post.images
        {
            self.addImage(&img, id)?;
        }
        Ok(id)
    }

    fn addImage(&self, img: &Image, post_id: i64) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute(
            "INSERT INTO images (path, width, height, post)
             VALUES (?, ?, ?, ?);", sql::params![
                 &img.path.to_str().ok_or_else(
                     || rterr!("Invalid image path: {:?}", img.path))?,
                 img.width,
                 img.height,
                 post_id,
             ]).map_err(|e| error!(DataError, "Failed to add image: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid insert happened"));
        }
        Ok(())
    }

    pub fn deletePost(&self, post_id: i64) -> Result<(), Error>
    {
        let conn = self.confirmConnection()?;
        let row_count = conn.execute("DELETE FROM images WHERE post = ?;",
                                     sql::params![post_id,]).map_err(
            |e| error!(DataError, "Failed to delete images: {}", e))?;
        if row_count == 0
        {
            return Err(error!(DataError, "Post not found"));
        }
        let row_count = conn.execute("DELETE FROM posts WHERE id = ?;",
                                     sql::params![post_id,]).map_err(
            |e| error!(DataError, "Failed to delete post: {}", e))?;
        if row_count != 1
        {
            return Err(error!(DataError, "Invalid deletion happened."));
        }
        Ok(())
    }

    fn row2Post(row: &sql::Row, images: Vec<Image>) -> sql::Result<Post>
    {
        let time_value = row.get(2)?;
        Ok(Post {
            id: row.get(0)?,
            images,
            desc: row.get(1)?,
            upload_time: time::OffsetDateTime::from_unix_timestamp(
                time_value).map_err(
                |_| sql::Error::IntegralValueOutOfRange(
                    2, time_value))?,
            album_id: row.get(3)?,
        })
    }

    fn row2Image(row: &sql::Row) -> sql::Result<Image>
    {
        let path: String = row.get(0)?;
        Ok(Image {
            path: PathBuf::from_str(&path).unwrap(),
            width: row.get(1)?,
            height: row.get(2)?,
        })
    }

    pub fn findPostByID(&self, post_id: i64) -> Result<Option<Post>, Error>
    {
        let conn = self.confirmConnection()?;
        let mut cmd = conn.prepare(
            "SELECT path, width, height FROM images WHERE post = ?;")
            .map_err(|e| error!(
                DataError,
                "Failed to compare statement to get images: {}", e))?;
        let images: Result<Vec<Image>, Error> =
            cmd.query_map([post_id,], Self::row2Image)
            .map_err(|e| error!(DataError, "Failed to retrieve image: {}", e))?
            .map(|row| row.map_err(|e| error!(DataError, "{}", e)))
            .collect();
        let images = images?;
        conn.query_row(
            "SELECT id, desc, upload_time, album FROM posts WHERE id=?;",
            sql::params![post_id], |row| Self::row2Post(row, images))
            .optional().map_err(
                |e| error!(DataError, "Failed to look up post {}: {}", post_id, e))
    }

    /// Retrieve “count” number of posts, starting from the entry at
    /// index “start_index”. Index is 0-based. Returned entries are
    /// sorted from new to old.
    pub fn getPosts(&self, start_index: u64, count: u64, order: PostOrder) ->
        Result<Vec<Post>, Error>
    {
        let conn = self.confirmConnection()?;

        let order_expr = match order
        {
            PostOrder::NewFirst => "ORDER BY upload_time DESC",
        };

        let mut cmd = conn.prepare(
            &format!("SELECT id FROM posts {} LIMIT ? OFFSET ?;", order_expr))
            .map_err(|e| error!(
                DataError,
                "Failed to compare statement to get posts: {}", e))?;
        let ids = cmd.query_map([count, start_index], |row| row.get(0))
            .map_err(|e| error!(DataError, "Failed to retrieve videos: {}", e))?
            .map(|row| row.map_err(|e| error!(DataError, "{}", e)));
        let mut result: Vec<Post> = Vec::new();
        for id in ids
        {
            let id = id?;
            if let Some(p) = self.findPostByID(id)?
            {
                result.push(p);
            }
            else
            {
                return Err(error!(
                    DataError, "Failed to retrieve post with id {}.", id));
            }
        }
        Ok(result)
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

// ========== Unit tests ============================================>

#[cfg(test)]
mod tests
{
    use super::*;

    fn tempFile() -> PathBuf
    {
        std::env::temp_dir().join(OffsetDateTime::now_utc().unix_timestamp_nanos().to_string())
    }

    struct FileDeleter
    {
        files: Vec<PathBuf>,
    }

    impl FileDeleter
    {
        fn new() -> Self
        {
            Self { files: Vec::new() }
        }

        fn register<P: AsRef<Path>>(&mut self, f: P)
        {
            let p: &Path = f.as_ref();
            self.files.push(p.to_owned());
        }
    }

    impl Drop for FileDeleter
    {
        fn drop(&mut self)
        {
            for f in &self.files
            {
                std::fs::remove_file(&f).ok();
            }
        }
    }

    #[test]
    fn addEmptyPostAndQuery() -> Result<(), Error>
    {
        let mut manager = Manager::new(sqlite_connection::Source::Memory);
        manager.connect()?;
        manager.init()?;

        let p = Post::new();
        let id = manager.addPost(&p, None)?;
        let post_maybe = manager.findPostByID(id)?;
        assert!(post_maybe.is_some());
        Ok(())
    }

    #[test]
    fn addPostWithImageAndQueryAndDelete() -> Result<(), Error>
    {
        let mut deleter = FileDeleter::new();
        let db = tempFile();
        deleter.register(&db);

        let mut manager = Manager::new(sqlite_connection::Source::File(db));
        manager.connect()?;
        manager.init()?;

        let image1 = Image {
            path: PathBuf::from("aaa"),
            width: 1,
            height: 2,
        };
        let image2 = Image {
            path: PathBuf::from("bbb"),
            width: 3,
            height: 4,
        };
        let mut p = Post::new();
        p.images = vec![image1, image2];

        let id = manager.addPost(&p, None)?;
        let post_maybe = manager.findPostByID(id)?;
        assert!(post_maybe.is_some());
        let post = post_maybe.unwrap();
        assert_eq!(post.id, id);
        assert_eq!(post.images.len(), 2);

        manager.deletePost(id)?;
        assert!(manager.findPostByID(id)?.is_none());
        Ok(())
    }
}
