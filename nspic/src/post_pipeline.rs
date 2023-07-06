use std::path::{Path, PathBuf};
use std::io::prelude::*;
use std::io::BufWriter;
use std::fs::File;
use std::ffi::OsStr;
use std::process::Command;
use std::str;

use futures_util::StreamExt;
use bytes::buf::Buf;
use log::debug;
use log::error as log_error;
use warp::http::status::StatusCode;
use sha2::Digest;

use crate::error::Error;
use crate::post::Image;
use crate::config::Configuration;

pub fn imagePath(image: &Image, config: &Configuration) -> PathBuf
{
    Path::new(&config.image_dir).join(&image.path)
}

fn randomTempFilename<P: AsRef<Path>>(dir: P) -> PathBuf
{
    loop
    {
        let filename = format!("temp-{}", rand::random::<u32>());
        let path = dir.as_ref().join(&filename);
        if !path.exists()
        {
            return path;
        }
    }
}

struct ImageMetadata
{
    width: u32,
    height: u32,
}

impl ImageMetadata
{
    pub fn new() -> Self
    {
        Self { width: 0, height: 0 }
    }
}

fn resizeImage(img: &Path, output: &Path, size: u32, quality: i32) ->
    Result<(), Error>
{
    let status = Command::new("magick").args(
        &[img.to_str().ok_or_else(
            || rterr!("Invalid image path: {:?}", img))?,
          "-colorspace", "RGB", "-resize", &format!("{size}x{size}>"),
          "-colorspace", "sRGB", "-quality", &quality.to_string(),
          output.to_str().ok_or_else(
              || rterr!("Invalid image path: {:?}", img))?,
        ])
        .status().map_err(|e| rterr!("Failed to run imagemagick: {}", e))?;
    if status.success()
    {
        Ok(())
    }
    else
    {
        Err(rterr!("Imagemagick failed."))
    }
}

fn probeImage(f: &Path) -> Result<ImageMetadata, Error>
{
    let output = Command::new("magick").arg("identify").arg("-format")
        .arg("%[fx:w]\n%[fx:h]\n")
        .arg(f.to_str().ok_or_else(|| rterr!("Invalid image path: {:?}", f))?)
        .output().map_err(|e| rterr!("Failed to run imagemagick: {}", e))?;
    if !output.status.success()
    {
        if let Some(code) = output.status.code()
        {
            return Err(rterr!("Identify failed with code {}.", code));
        }
        else
        {
            return Err(rterr!("Identify terminated with signal."));
        }
    }
    let output = str::from_utf8(&output.stdout).map_err(
        |_| rterr!("Invalid UTF-8 in imagemagick output"))?;
    let mut lines = output.lines();
    let mut data = ImageMetadata::new();
    data.width = lines.next().ok_or_else(
        || rterr!("Not enough lines in imagemagick output"))?.parse().map_err(
        |_| rterr!("Invalid width"))?;
    data.height = lines.next().ok_or_else(
        || rterr!("Not enough lines in imagemagick output"))?.parse().map_err(
        |_| rterr!("Invalid height"))?;
    Ok(data)
}

pub async fn uploadPart(part: warp::multipart::Part) -> Result<Vec<u8>, Error>
{
    let mut data: Vec<u8> = Vec::new();
    let mut buffers = part.stream();
    while let Some(buffer) = buffers.next().await
    {
        let mut buffer = buffer.map_err(
            |e| rterr!("Failed to acquire buffer from form data: {}", e))?;
        while buffer.has_remaining()
        {
            let bytes = buffer.chunk();
            let buffer_size = bytes.len();
            data.extend(bytes.iter());
            buffer.advance(buffer_size);
        }
    }
    Ok(data)
}

/// Some bytes that are being uploaded
pub struct UploadingImage
{
    pub part: warp::multipart::Part,
}

/// A image file that is just uploaded.
pub struct RawImage
{
    /// Path of the image file, accessible from the CWD.
    pub path: PathBuf,
    pub hash: String,
    pub original_filename: String,
}

impl UploadingImage
{
    /// This will create a temp file under the image directory. This
    /// is important, because later the image will be renamed to the
    /// correct name. We need the rename to happen in the same storage
    /// volumn so that it can succeed.
    pub async fn saveToTemp(self, config: &Configuration) ->
        Result<RawImage, Error>
    {
        let orig_name = self.part.filename().map(|n| n.to_owned()).ok_or_else(
            || Error::HTTPStatus(StatusCode::BAD_REQUEST,
                                 String::from("No filename in upload")))?;
        let temp_file = randomTempFilename(&config.image_dir)
            .with_extension(Path::new(&orig_name).extension()
                            .or(Some(OsStr::new(""))).unwrap());
        let mut f = match File::create(&temp_file)
        {
            Ok(f) => BufWriter::new(f),
            Err(e) => {
                return Err(rterr!("Failed to open temp file: {}", e));
            },
        };
        let mut hasher = sha2::Sha256::new();
        let mut buffers = self.part.stream();
        while let Some(buffer) = buffers.next().await
        {
            if buffer.is_err()
            {
                if std::fs::remove_file(&temp_file).is_err()
                {
                    log_error!("Failed to remove temp file at {:?}.", temp_file);
                }
            }
            let mut buffer = buffer.map_err(
                |e| rterr!("Failed to acquire buffer from form data: {}", e))?;
            while buffer.has_remaining()
            {
                let bytes = buffer.chunk();
                hasher.update(bytes);
                if let Err(e) = f.write_all(bytes)
                {
                    drop(f);
                    if std::fs::remove_file(&temp_file).is_err()
                    {
                        log_error!("Failed to remove temp file at {:?}.", temp_file);
                    }
                    return Err(rterr!("Failed to write temp file: {}", e));
                }
                buffer.advance(bytes.len());
            }
        }

        let hash = hasher.finalize();
        // A full hex-encoded 256 bit hash is 64 characters. That’s
        // pretty long. Here we just take the first half.
        let byte_strs: Vec<_> = hash[..16].iter().map(|b| format!("{:02x}", b))
            .collect();

        Ok(RawImage {
            path: temp_file,
            hash: byte_strs.join(""),
            original_filename: orig_name,
        })
    }
}

/// A uploaded image file with resized version.
pub struct ResizedImage
{
    /// Path of the uploaded temp image file, accessible from the CWD.
    pub uploaded: PathBuf,
    /// Path of the resized image file, accessible from the CWD.
    pub path: PathBuf,
    pub hash: String,
    pub original_filename: String,
}

impl RawImage
{
    pub fn resize(self, config: &Configuration) -> Result<ResizedImage, Error>
    {
        let target_file = self.path.with_file_name(
            format!("{}-processed.{}",
                    self.path.file_stem().unwrap().to_str().unwrap().to_owned(),
                    config.image_encoding.extension()));

        if let Err(e) = resizeImage(
            &self.path, &target_file, config.image_pixel_size,
            config.image_encoding_quality)
        {
            std::fs::remove_file(&self.path).ok();
            std::fs::remove_file(&target_file).ok();
            return Err(e);
        }
        Ok(ResizedImage {
            uploaded: self.path,
            path: target_file,
            hash: self.hash,
            original_filename: self.original_filename,
        })
    }
}

/// A uploaded image file with resized version.
pub struct ImageWithThumbnail
{
    /// Path of the resized image file, accessible from the CWD.
    pub path: PathBuf,
    /// Path of the thumbnail file, accessible from the CWD.
    pub thumbnail: PathBuf,
    pub hash: String,
    pub original_filename: String,
}

impl ResizedImage
{
    pub fn makeThumbnail(self, config: &Configuration) ->
        Result<ImageWithThumbnail, Error>
    {
        let thumb_file = randomTempFilename(&config.image_dir)
            .with_extension(config.image_encoding.extension());
        if let Err(e) = resizeImage(
            &self.uploaded, &thumb_file, config.thumb_pixel_size,
            config.image_encoding_quality)
        {
            std::fs::remove_file(&self.path).ok();
            std::fs::remove_file(&self.uploaded).ok();
            std::fs::remove_file(&thumb_file).ok();
            return Err(e);
        }
        std::fs::remove_file(&self.uploaded).ok();
        Ok(ImageWithThumbnail {
            path: self.path,
            thumbnail: thumb_file,
            hash: self.hash,
            original_filename: self.original_filename
        })
    }
}

impl ImageWithThumbnail
{
    pub fn moveToLibrary(self, config: &Configuration) ->
        Result<Self, Error>
    {
        let subdir = Path::new(&config.image_dir).join(&self.hash[..1]);
        if !subdir.exists()
        {
            std::fs::create_dir(&subdir).map_err(
                |_| rterr!("Failed to create sub dir"))?;
        }
        let ext = config.image_encoding.extension();
        let image_file: PathBuf = subdir.join(&self.hash).with_extension(ext);
        debug!("Moving image {:?} --> {:?}...", self.path, image_file);
        assert!(self.path.exists());
        if let Err(e) = std::fs::rename(&self.path, &image_file)
        {
            std::fs::remove_file(&self.path).ok();
            std::fs::remove_file(&self.thumbnail).ok();
            std::fs::remove_file(&image_file).ok();
            return Err(rterr!("Failed to rename temp file: {}", e));
        }
        let thumb_file: PathBuf = subdir.join(
            format!("{}_t.{}", self.hash, ext));
        assert!(self.thumbnail.exists());
        debug!("Moving thumbnail {:?} --> {:?}...", self.thumbnail, thumb_file);
        if let Err(e) = std::fs::rename(&self.thumbnail, &thumb_file)
        {
            std::fs::remove_file(&self.path).ok();
            std::fs::remove_file(&self.thumbnail).ok();
            std::fs::remove_file(&image_file).ok();
            std::fs::remove_file(&thumb_file).ok();
            return Err(rterr!("Failed to rename temp file: {}", e));
        }
        Ok(Self {
            path: image_file,
            thumbnail: thumb_file,
            hash: self.hash,
            original_filename: self.original_filename
        })
    }

    pub fn makeRelativePath(mut self, config: &Configuration) ->
        Result<Self, Error>
    {
        let full_path = self.path.canonicalize().map_err(
            |e| {
                std::fs::remove_file(&self.path).ok();
                rterr!("Failed to canonicalize path {:?}: {}", self.path, e)
            })?;
        let video_dir = Path::new(&config.image_dir).canonicalize().map_err(
            |e| {
                std::fs::remove_file(&self.path).ok();
                rterr!("Failed to canonicalize path {:?}: {}",
                       config.image_dir, e)
            })?;
        if !full_path.exists()
        {
            std::fs::remove_file(&self.path).ok();
            return Err(rterr!("Image not found: {:?}", full_path));
        }
        let path = full_path.strip_prefix(video_dir).map_err(
            |_| {
                std::fs::remove_file(&full_path).ok();
                rterr!("Image is not in the image directory.")
            })?;
        self.path = path.to_owned();
        Ok(self)
    }

    pub fn probeMetadata(self, config: &Configuration) -> Result<Image, Error>
    {
        let metadata = match probeImage(&PathBuf::from(&config.image_dir)
                                        .join(&self.path))
        {
            Ok(data) => data,
            Err(e) => {
                std::fs::remove_file(&self.path).ok();
                std::fs::remove_file(&self.thumbnail).ok();
                return Err(e);
            },
        };
        Ok(Image {
            path: self.path,
            width: metadata.width,
            height: metadata.height,
        })
    }
}

#[cfg(test)]
mod tests
{
    use super::*;
    use crate::data;

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
                if !f.exists()
                {
                    continue;
                }
                if f.is_dir()
                {
                    std::fs::remove_dir_all(&f).ok();
                }
                else
                {
                    std::fs::remove_file(&f).ok();
                }
            }
        }
    }

    fn uniqueTempDir() -> Result<PathBuf, Box<dyn std::error::Error>>
    {
        let image_dir = std::env::temp_dir().join(
            "nspic-test-".to_owned() + &rand::random::<u64>().to_string());
        std::fs::create_dir_all(&image_dir)?;
        Ok(image_dir)
    }

    #[test]
    fn postPipelineWontShrinkSmallImage() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut clean_up = FileDeleter::new();
        let image_dir = uniqueTempDir()?;
        clean_up.register(&image_dir);
        let mut config = Configuration::default();
        config.image_dir = image_dir.to_str().ok_or(
            rterr!("Invalid image dir"))?.to_owned();
        let temp_file = image_dir.join("test.png");
        std::fs::copy("test-data/test.png", &temp_file)?;
        clean_up.register(&temp_file);
        let v = RawImage {
            path: temp_file,
            hash: "12345".to_owned(),
            original_filename: "test.png".to_owned(),
        };
        let mut data_manager = data::Manager::new(
            crate::sqlite_connection::Source::Memory);
        data_manager.connect()?;
        data_manager.init()?;
        let img = v.resize(&config)?
            .makeThumbnail(&config)?
            .moveToLibrary(&config)?
            .makeRelativePath(&config)?
            .probeMetadata(&config)?;

        assert_eq!(&img.path, &Path::new("1").join("12345.jpg"));
        assert!(PathBuf::from(&config.image_dir).join(&img.thumbnail()?)
                .exists());
        assert_eq!(img.width, 400);
        assert_eq!(img.height, 296);
        Ok(())
    }

    #[test]
    fn postPipelineShrinksLargeImage() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut clean_up = FileDeleter::new();
        let image_dir = uniqueTempDir()?;
        clean_up.register(&image_dir);
        let mut config = Configuration::default();
        config.image_pixel_size = 256;
        config.image_dir = image_dir.to_str().ok_or(
            rterr!("Invalid image dir"))?.to_owned();
        let temp_file = image_dir.join("test.png");
        std::fs::copy("test-data/test.png", &temp_file)?;
        clean_up.register(&temp_file);
        let v = RawImage {
            path: temp_file,
            hash: "12345".to_owned(),
            original_filename: "test.png".to_owned(),
        };
        let mut data_manager = data::Manager::new(
            crate::sqlite_connection::Source::Memory);
        data_manager.connect()?;
        data_manager.init()?;
        let img = v.resize(&config)?
            .makeThumbnail(&config)?
            .moveToLibrary(&config)?
            .makeRelativePath(&config)?
            .probeMetadata(&config)?;

        assert_eq!(&img.path, &Path::new("1").join("12345.jpg"));
        assert!(PathBuf::from(&config.image_dir).join(&img.thumbnail()?)
                .exists());
        assert_eq!(img.width, 256);
        assert_eq!(img.height, 189);
        Ok(())
    }
}
