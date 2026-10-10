use std::sync::Arc;

use discord_webhook_storage::{
    BotCredentials, DiscordFile as StoredDiscordFile, WebhookCredentials, WriteConfig, renew_urls,
};
use futures_core::Stream;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use tokio_tcp_pool::{Connection, Pool};

use crate::{
    Error, Expired, RotatingVec,
    traits::{FileBackend, RegistryBackend},
};

pub use discord_webhook_storage::DiscordFileUrl as DiscordReadAccessPath;

// ponytail: shared URLs avoid copies; the dependency requires parsing at upload opening.
#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordWriteAccessPath(Arc<str>);

// Mirrors the documented WriteConfig::default limit in discord-webhook-storage
// 0.1.0. This limit excludes multipart framing.
const DISCORD_MAX_FILE_SIZE: u64 = 20_000_000;

pub type DiscordReadFile = discord_webhook_storage::ReadFile<Connection>;
pub type DiscordUploadFile = discord_webhook_storage::WriteFile<Connection>;

#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordAccessPath(StoredDiscordFile);

impl From<StoredDiscordFile> for DiscordAccessPath {
    fn from(file: StoredDiscordFile) -> Self {
        Self(file)
    }
}

impl Expired for DiscordAccessPath {
    fn is_expired(&self) -> bool {
        !self.0.is_valid()
    }
}

/// Opens Discord files using existing TLS connection pools.
#[derive(Clone)]
pub struct DiscordFile {
    read_pool: Pool,
    write_pool: Pool,
}

impl DiscordFile {
    /// Uses pools targeting `cdn.discordapp.com:443` for reads and
    /// `discord.com:443` for uploads, both with TLS and HTTP/1.1.
    pub fn new(read_pool: Pool, write_pool: Pool) -> Self {
        Self {
            read_pool,
            write_pool,
        }
    }
}

impl FileBackend for DiscordFile {
    type ReadAccessPath = DiscordReadAccessPath;
    type WriteAccessPath = DiscordWriteAccessPath;
    type ReadFile = DiscordReadFile;
    type WriteFile = DiscordUploadFile;

    async fn write(&self, path: &Self::WriteAccessPath) -> Result<Self::WriteFile, Error> {
        let credentials = WebhookCredentials::parse(&path.0)?;
        let connection = self.write_pool.acquire().await?;
        Ok(DiscordUploadFile::open(connection, &credentials, &WriteConfig::default()).await?)
    }

    async fn read(
        &self,
        path: &Self::ReadAccessPath,
        start: usize,
        end: Option<usize>,
    ) -> Result<Self::ReadFile, Error> {
        let connection = self.read_pool.acquire().await?;
        Ok(DiscordReadFile::open_with_range(connection, path, start, end).await?)
    }
}

/// Discord backend with a TLS connection pool targeting `discord.com:443`.
///
/// Owns the credential lists, including webhook URLs and bot tokens.
#[derive(Clone)]
pub struct DiscordRegistry {
    pool: Pool,
    webhook_credentials: RotatingVec<DiscordWriteAccessPath>,
    bot_credentials: RotatingVec<String>,
}

impl DiscordRegistry {
    /// Creates a backend from an existing pool and both credential lists.
    ///
    /// The caller must configure the pool for TLS connections to `discord.com:443`.
    pub fn new(
        pool: Pool,
        webhook_credentials: Vec<String>,
        bot_credentials: Vec<String>,
    ) -> Self {
        Self {
            pool,
            webhook_credentials: RotatingVec::new(
                webhook_credentials
                    .into_iter()
                    .map(|url| DiscordWriteAccessPath(Arc::from(url)))
                    .collect(),
            ),
            bot_credentials: RotatingVec::new(bot_credentials),
        }
    }
}

impl RegistryBackend for DiscordRegistry {
    type AccessPath = DiscordAccessPath;
    type WriteAccessPath = DiscordWriteAccessPath;
    type ReadAccessPath = DiscordReadAccessPath;

    fn write(
        &self,
        count: usize,
        max_file_size: u64,
    ) -> impl Stream<Item = Self::WriteAccessPath> + Send {
        let count = if max_file_size <= DISCORD_MAX_FILE_SIZE { count } else { 0 };
        stream::iter((0..count).map_while(|_| self.webhook_credentials.get().cloned()))
    }

    fn read<'stream>(
        &'stream self,
        access_paths: &'stream [Self::AccessPath],
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + Send + 'stream {
        stream::iter(access_paths).then(move |access_path| async move {
            let mut url = access_path.0.url.clone();
            if !access_path.is_expired() {
                return Ok(url);
            }

            let credentials = self
                .bot_credentials
                .get()
                .ok_or(Error::MissingBotCredentials)?;
            let credentials = BotCredentials { token: credentials };
            let mut connection = self.pool.acquire().await?;
            renew_urls(&mut connection, &credentials, std::iter::once(&mut url)).await?;
            connection.release();

            Ok(url)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_core::Stream;
    use std::task::{Context, Poll};
    use tokio_tcp_pool::{Endpoint, Route};

    #[test]
    fn enum_dispatch_preserves_count_and_shared_rotation() -> Result<(), Box<dyn std::error::Error>>
    {
        let pool = Pool::builder(Route::Direct {
            target: Endpoint::new("discord.com", 443)?,
        })
        .build()?;
        let backend = crate::RegistryBackend::Discord(DiscordRegistry::new(
            pool,
            vec![
                "https://discord.com/api/webhooks/first/token".into(),
                "https://discord.com/api/webhooks/second/token".into(),
            ],
            Vec::new(),
        ));
        let waker = futures_util::task::noop_waker();
        let mut context = Context::from_waker(&waker);

        {
            let oversized = RegistryBackend::write(&backend, 1, 20_000_001);
            let mut oversized = std::pin::pin!(oversized);
            if !matches!(oversized.as_mut().poll_next(&mut context), Poll::Ready(None)) {
                return Err("write path accepted an unsupported file size".into());
            }
        }

        for (backend, expected) in [
            (backend.clone(), Vec::new()),
            (backend.clone(), vec!["first", "second", "first"]),
            (backend, vec!["second", "first"]),
        ] {
            let paths = RegistryBackend::write(&backend, expected.len(), 20_000_000);
            let mut paths = std::pin::pin!(paths);
            for expected in expected {
                let expected_url = format!("https://discord.com/api/webhooks/{expected}/token");
                match paths.as_mut().poll_next(&mut context) {
                    Poll::Ready(Some(crate::WriteAccessPath::Discord(path)))
                        if path.0.as_ref() == expected_url => {}
                    _ => return Err("unexpected webhook rotation".into()),
                }
            }
            if !matches!(paths.as_mut().poll_next(&mut context), Poll::Ready(None)) {
                return Err("write stream exceeded requested count".into());
            }
        }
        Ok(())
    }
}
