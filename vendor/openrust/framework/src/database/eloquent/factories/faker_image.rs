use std::{
    fmt::Write as _,
    io,
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

use tokio::io::AsyncWriteExt;

use crate::database::{DatabaseError, DatabaseResult};

use super::Faker;

const BASE_URL: &str = "https://via.placeholder.com";

/// Options for the FakerPHP placeholder image provider.
pub struct ImageOptions<'a> {
    pub width: u32,
    pub height: u32,
    pub category: Option<&'a str>,
    pub randomize: bool,
    pub word: Option<&'a str>,
    pub gray: bool,
    pub format: &'a str,
}

impl Default for ImageOptions<'_> {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            category: None,
            randomize: true,
            word: None,
            gray: false,
            format: "png",
        }
    }
}

#[allow(non_snake_case, reason = "Matches Faker formatter names")]
impl Faker {
    pub fn imageUrl(&self, options: &ImageOptions<'_>) -> DatabaseResult<String> {
        if !Self::getFormats()
            .iter()
            .any(|format| format.eq_ignore_ascii_case(options.format))
        {
            return Err(DatabaseError::InvalidQuery(
                "unsupported Faker image format",
            ));
        }

        let color = if options.gray {
            "CCCCCC".to_owned()
        } else {
            let value = fastrand::u8(..);
            let high = value >> 4;
            let low = value & 15;
            format!("00{high:x}{high:x}{low:x}{low:x}")
        };
        let mut url = format!(
            "{BASE_URL}/{}x{}.{}/{color}",
            options.width, options.height, options.format
        );
        let word = options.randomize.then(|| self.word());
        let parts: Vec<_> = [options.category, options.word, word.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        if !parts.is_empty() {
            url.push_str("?text=");
            for byte in parts.join(" ").bytes() {
                match byte {
                    b' ' => url.push('+'),
                    b'-' | b'_' | b'.' => url.push(char::from(byte)),
                    byte if byte.is_ascii_alphanumeric() => url.push(char::from(byte)),
                    byte => write!(url, "%{byte:02X}").expect("writing to String cannot fail"),
                }
            }
        }
        Ok(url)
    }

    pub fn getFormats() -> [&'static str; 3] {
        Self::getFormatConstants().map(|(format, _)| format)
    }

    /// Returns the image formats and their PHP image type identifiers.
    pub const fn getFormatConstants() -> [(&'static str, u8); 3] {
        [("jpg", 2), ("jpeg", 2), ("png", 3)]
    }

    /// Downloads a placeholder image. A remote failure returns no path.
    pub async fn image(
        &self,
        directory: Option<&Path>,
        fullPath: bool,
        options: &ImageOptions<'_>,
    ) -> io::Result<Option<PathBuf>> {
        let url = self
            .imageUrl(options)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let default_directory;
        let directory = match directory {
            Some(directory) => directory,
            None => {
                default_directory = std::env::temp_dir();
                &default_directory
            }
        };
        download_image(&url, directory, options.format, fullPath).await
    }
}

async fn download_image(
    url: &str,
    directory: &Path,
    format: &str,
    full_path: bool,
) -> io::Result<Option<PathBuf>> {
    static CLIENT: LazyLock<Result<reqwest::Client, reqwest::Error>> = LazyLock::new(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
    });
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    let client = CLIENT
        .as_ref()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut response = match client.get(url).send().await {
        Ok(response) if response.status() == reqwest::StatusCode::OK => response,
        _ => return Ok(None),
    };
    let mut file = tokio::fs::File::from_std(temporary.reopen()?);
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => file.write_all(&chunk).await?,
            Ok(None) => break,
            Err(_) => return Ok(None),
        }
    }
    file.flush().await?;
    drop(file);
    let name = format!("{:032x}.{format}", fastrand::u128(..));
    let destination = directory.join(&name);
    temporary.persist_noclobber(&destination)?;
    Ok(Some(if full_path {
        destination
    } else {
        PathBuf::from(name)
    }))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    #[test]
    fn faker_image_urls_match_php_format_and_encoding() {
        let faker = Faker::new("en").unwrap();
        assert_eq!(
            Faker::getFormatConstants(),
            [("jpg", 2), ("jpeg", 2), ("png", 3)]
        );
        let mut options = ImageOptions {
            category: Some("a b"),
            word: Some("你好~*"),
            randomize: false,
            gray: true,
            format: "JPEG",
            ..ImageOptions::default()
        };
        let expected =
            "https://via.placeholder.com/640x480.JPEG/CCCCCC?text=a+b+%E4%BD%A0%E5%A5%BD%7E%2A";
        assert_eq!(faker.imageUrl(&options).unwrap(), expected);
        assert_eq!(faker.unique(false).imageUrl(&options).unwrap(), expected);
        assert!(faker.unique(false).imageUrl(&options).is_err());
        options.format = "gif";
        assert!(faker.imageUrl(&options).is_err());
        options.format = "png";
        options.category = None;
        options.word = None;
        options.gray = false;
        let url = faker.imageUrl(&options).unwrap();
        let color = url.rsplit('/').next().unwrap().as_bytes();
        assert_eq!(&color[..2], b"00");
        assert_eq!(color[2], color[3]);
        assert_eq!(color[4], color[5]);
    }

    #[tokio::test]
    async fn faker_image_download_publishes_only_complete_successes() {
        let directory = tempfile::tempdir().unwrap();
        for (response, expected) in [
            (
                &b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nimage"[..],
                true,
            ),
            (
                &b"HTTP/1.1 404 Not Found\r\nContent-Length: 5\r\nConnection: close\r\n\r\nerror"[..],
                false,
            ),
            (
                &b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nshort"[..],
                false,
            ),
        ] {
            let server = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", server.local_addr().unwrap());
            let worker = tokio::spawn(async move {
                let (mut client, _) = server.accept().await.unwrap();
                let mut buffer = [0; 1024];
                let count = client.read(&mut buffer).await.unwrap();
                assert!(buffer[..count].starts_with(b"GET / "));
                client.write_all(response).await.unwrap();
            });
            let result = tokio::time::timeout(
                Duration::from_secs(5),
                download_image(&url, directory.path(), "png", false),
            )
            .await
            .unwrap()
            .unwrap();
            worker.await.unwrap();
            assert_eq!(result.is_some(), expected);
            if let Some(name) = result {
                assert_eq!(name.extension().unwrap(), "png");
                assert_eq!(name.file_stem().unwrap().len(), 32);
                let path = directory.path().join(name);
                assert_eq!(std::fs::read(&path).unwrap(), b"image");
                std::fs::remove_file(path).unwrap();
            }
            assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
        }
    }
}
