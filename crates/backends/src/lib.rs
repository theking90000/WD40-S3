use std::{
    io::{self, IoSlice},
    pin::Pin,
    task::{Context, Poll},
};

use futures_core::Stream;
use futures_io::{AsyncRead, AsyncWrite};
use futures_util::{StreamExt, TryStreamExt, stream};
use serde::{Deserialize, Serialize};

#[cfg(feature = "discord")]
pub mod discord;

pub mod error;
pub mod rotating_vec;

#[path = "trait.rs"]
pub mod traits;

#[cfg(feature = "discord")]
pub use discord::{
    DiscordAccessPath, DiscordFile, DiscordReadAccessPath, DiscordReadFile, DiscordRegistry,
    DiscordUploadFile, DiscordWriteAccessPath,
};
pub use error::Error;
pub use rotating_vec::RotatingVec;
pub use traits::Expired;

#[derive(Clone)]
pub enum FileBackend {
    #[cfg(feature = "discord")]
    Discord(DiscordFile),
    #[cfg(feature = "telegram")]
    Telegram,
}

pub enum ReadFile {
    #[cfg(feature = "discord")]
    Discord(DiscordReadFile),
    #[cfg(feature = "telegram")]
    Telegram,
}

impl AsyncRead for ReadFile {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        match *self.get_mut() {
            #[cfg(feature = "discord")]
            Self::Discord(ref mut file) => Pin::new(file).poll_read(cx, buf),
            #[cfg(feature = "telegram")]
            Self::Telegram => unimplemented!("Telegram file reading"),
        }
    }
}

pub enum WriteFile {
    #[cfg(feature = "discord")]
    Discord(DiscordUploadFile),
    #[cfg(feature = "telegram")]
    Telegram,
}

impl WriteFile {
    /// Finishes the upload and returns the persistent path accepted by its backend.
    pub async fn finish(self) -> Result<AccessPath, Error> {
        match self {
            #[cfg(feature = "discord")]
            Self::Discord(file) => Ok(AccessPath::Discord(DiscordAccessPath::from(
                file.finish().await?,
            ))),
            #[cfg(feature = "telegram")]
            Self::Telegram => Err(Error::UnsupportedBackend),
        }
    }
}

impl AsyncWrite for WriteFile {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match *self.get_mut() {
            #[cfg(feature = "discord")]
            Self::Discord(ref mut file) => Pin::new(file).poll_write(cx, buf),
            #[cfg(feature = "telegram")]
            Self::Telegram => unimplemented!("Telegram file writing"),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match *self.get_mut() {
            #[cfg(feature = "discord")]
            Self::Discord(ref mut file) => Pin::new(file).poll_flush(cx),
            #[cfg(feature = "telegram")]
            Self::Telegram => unimplemented!("Telegram file flushing"),
        }
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match *self.get_mut() {
            #[cfg(feature = "discord")]
            Self::Discord(ref mut file) => Pin::new(file).poll_close(cx),
            #[cfg(feature = "telegram")]
            Self::Telegram => unimplemented!("Telegram file closing"),
        }
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        match *self.get_mut() {
            #[cfg(feature = "discord")]
            Self::Discord(ref mut file) => Pin::new(file).poll_write_vectored(cx, bufs),
            #[cfg(feature = "telegram")]
            Self::Telegram => unimplemented!("Telegram vectored file writing"),
        }
    }
}

impl traits::FileBackend for FileBackend {
    type ReadAccessPath = ReadAccessPath;
    type WriteAccessPath = WriteAccessPath;
    type ReadFile = ReadFile;
    type WriteFile = WriteFile;

    async fn write(&self, path: &Self::WriteAccessPath) -> Result<Self::WriteFile, Error> {
        #[allow(unreachable_patterns)]
        match (self, path) {
            #[cfg(feature = "discord")]
            (Self::Discord(backend), WriteAccessPath::Discord(path)) => {
                traits::FileBackend::write(backend, path)
                    .await
                    .map(WriteFile::Discord)
            }
            #[cfg(feature = "telegram")]
            (Self::Telegram, WriteAccessPath::Telegram) => unimplemented!("Telegram file backend"),
            _ => Err(Error::BackendMismatch),
        }
    }

    async fn read(
        &self,
        path: &Self::ReadAccessPath,
        start: usize,
        end: Option<usize>,
    ) -> Result<Self::ReadFile, Error> {
        #[allow(unreachable_patterns)]
        match (self, path) {
            #[cfg(feature = "discord")]
            (Self::Discord(backend), ReadAccessPath::Discord(path)) => {
                traits::FileBackend::read(backend, path, start, end)
                    .await
                    .map(ReadFile::Discord)
            }
            #[cfg(feature = "telegram")]
            (Self::Telegram, ReadAccessPath::Telegram) => unimplemented!("Telegram file backend"),
            _ => Err(Error::BackendMismatch),
        }
    }
}

#[derive(Clone)]
pub enum RegistryBackend {
    #[cfg(feature = "discord")]
    Discord(DiscordRegistry),
    #[cfg(feature = "telegram")]
    Telegram,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum AccessPath {
    #[cfg(feature = "discord")]
    Discord(DiscordAccessPath),
    #[cfg(feature = "telegram")]
    Telegram,
}

impl Expired for AccessPath {
    fn is_expired(&self) -> bool {
        match *self {
            #[cfg(feature = "discord")]
            Self::Discord(ref path) => path.is_expired(),
            #[cfg(feature = "telegram")]
            Self::Telegram => unimplemented!("Telegram access path expiration"),
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub enum WriteAccessPath {
    #[cfg(feature = "discord")]
    Discord(DiscordWriteAccessPath),
    #[cfg(feature = "telegram")]
    Telegram,
}

#[derive(Clone, Serialize, Deserialize)]
pub enum ReadAccessPath {
    #[cfg(feature = "discord")]
    Discord(DiscordReadAccessPath),
    #[cfg(feature = "telegram")]
    Telegram,
}

impl Expired for ReadAccessPath {
    fn is_expired(&self) -> bool {
        match *self {
            #[cfg(feature = "discord")]
            Self::Discord(ref path) => !path.is_valid(),
            #[cfg(feature = "telegram")]
            Self::Telegram => true,
        }
    }
}

// ponytail: boxed streams keep dispatch simple across backend stream types.
impl traits::RegistryBackend for RegistryBackend {
    type AccessPath = AccessPath;
    type WriteAccessPath = WriteAccessPath;
    type ReadAccessPath = ReadAccessPath;

    fn write(
        &self,
        count: usize,
        max_file_size: u64,
    ) -> impl Stream<Item = Self::WriteAccessPath> + Send {
        stream::iter((count != 0).then_some(self)).flat_map(
            move |backend| -> Pin<Box<dyn Stream<Item = Self::WriteAccessPath> + Send + '_>> {
                match *backend {
                    #[cfg(feature = "discord")]
                    Self::Discord(ref registry) => Box::pin(
                        traits::RegistryBackend::write(registry, count, max_file_size)
                            .map(WriteAccessPath::Discord),
                    ),
                    #[cfg(feature = "telegram")]
                    Self::Telegram => Box::pin(stream::empty()),
                }
            },
        )
    }

    fn read<'stream>(
        &'stream self,
        access_paths: &'stream [Self::AccessPath],
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + Send + 'stream {
        let paths = stream::iter((!access_paths.is_empty()).then_some(self));
        paths.flat_map(
            move |backend| -> Pin<
                Box<dyn Stream<Item = Result<Self::ReadAccessPath, Error>> + Send + 'stream>,
            > {
                match *backend {
                    #[cfg(feature = "discord")]
                    Self::Discord(ref registry) => Box::pin(
                        stream::iter(access_paths.chunks(discord::DISCORD_RENEW_BATCH_SIZE))
                            .then(move |paths| async move {
                                let paths = paths
                                    .iter()
                                    .map(|path| {
                                        #[allow(unreachable_patterns)]
                                        match path {
                                            AccessPath::Discord(path) => Ok(path.clone()),
                                            _ => Err(Error::BackendMismatch),
                                        }
                                    })
                                    .collect::<Result<Vec<_>, _>>()?;
                                let results = traits::RegistryBackend::read(registry, &paths)
                                    .map_ok(ReadAccessPath::Discord)
                                    .try_collect::<Vec<_>>()
                                    .await?;
                                Ok::<_, Error>(stream::iter(results.into_iter().map(Ok)))
                            })
                            .try_flatten(),
                    ),
                    #[cfg(feature = "telegram")]
                    Self::Telegram => unimplemented!("Telegram registry backend"),
                }
            },
        )
    }
}
