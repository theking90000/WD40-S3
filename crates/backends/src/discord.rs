use discord_webhook_storage::{BotCredentials, DiscordFile, renew_urls};
use futures_core::Stream;
use futures_util::{StreamExt, stream};
use serde::{Deserialize, Serialize};
use tokio_tcp_pool::Pool;

use crate::{Error, Expired, RotatingVec, traits::RegistryBackend};

pub use discord_webhook_storage::DiscordFileUrl as DiscordReadAccessPath;

#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordAccessPath(DiscordFile);

impl Expired for DiscordAccessPath {
    fn is_expired(&self) -> bool {
        !self.0.is_valid()
    }
}

// ponytail: store the webhook URL; upload settings are always WriteConfig::default().
#[derive(Clone, Serialize, Deserialize)]
pub struct DiscordWriteAccessPath(String);

/// Discord backend with a TLS connection pool targeting `discord.com:443`.
///
/// Owns the credential lists, including their URL and token strings.
#[derive(Clone)]
pub struct DiscordRegistry {
    pool: Pool,
    webhook_credentials: RotatingVec<String>,
    bot_credentials: RotatingVec<String>,
}

impl DiscordRegistry {
    /// Creates a backend from an existing pool and both credential lists.
    ///
    /// The caller must configure the pool for TLS connections to `discord.com:443`.
    pub fn new(pool: Pool, webhook_credentials: Vec<String>, bot_credentials: Vec<String>) -> Self {
        Self {
            pool,
            webhook_credentials: RotatingVec::new(webhook_credentials),
            bot_credentials: RotatingVec::new(bot_credentials),
        }
    }
}

impl RegistryBackend for DiscordRegistry {
    type AccessPath = DiscordAccessPath;
    type WriteAccessPath = DiscordWriteAccessPath;
    type ReadAccessPath = DiscordReadAccessPath;

    fn write(&self, count: usize) -> impl Stream<Item = Self::WriteAccessPath> {
        stream::iter((0..count).map_while(|_| {
            self.webhook_credentials
                .get()
                .cloned()
                .map(DiscordWriteAccessPath)
        }))
    }

    fn read<'stream>(
        &'stream self,
        access_paths: &'stream [Self::AccessPath],
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + 'stream {
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
            vec!["first".into(), "second".into()],
            Vec::new(),
        ));
        let waker = futures_util::task::noop_waker();
        let mut context = Context::from_waker(&waker);

        for (backend, expected) in [
            (backend.clone(), Vec::new()),
            (backend.clone(), vec!["first", "second", "first"]),
            (backend, vec!["second", "first"]),
        ] {
            let paths = RegistryBackend::write(&backend, expected.len());
            let mut paths = std::pin::pin!(paths);
            for expected in expected {
                match paths.as_mut().poll_next(&mut context) {
                    Poll::Ready(Some(crate::WriteAccessPath::Discord(path)))
                        if path.0 == expected => {}
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
