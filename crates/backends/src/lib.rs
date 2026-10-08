use std::pin::Pin;

use futures_core::Stream;
use futures_util::{StreamExt, stream};
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
pub use traits::{Expired, FileBackend};

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

// ponytail: boxed streams keep dispatch simple across backend stream types.
impl traits::RegistryBackend for RegistryBackend {
    type AccessPath = AccessPath;
    type WriteAccessPath = WriteAccessPath;
    type ReadAccessPath = ReadAccessPath;

    fn write(&self, count: usize) -> impl Stream<Item = Self::WriteAccessPath> {
        stream::iter((count != 0).then_some(self)).flat_map(
            move |backend| -> Pin<Box<dyn Stream<Item = Self::WriteAccessPath> + '_>> {
                match *backend {
                    #[cfg(feature = "discord")]
                    Self::Discord(ref registry) => Box::pin(
                        traits::RegistryBackend::write(registry, count)
                            .map(WriteAccessPath::Discord),
                    ),
                    #[cfg(feature = "telegram")]
                    Self::Telegram => unimplemented!("Telegram registry backend"),
                }
            },
        )
    }

    fn read<'stream>(
        &'stream self,
        access_paths: &'stream [Self::AccessPath],
    ) -> impl Stream<Item = Result<Self::ReadAccessPath, Error>> + 'stream {
        stream::iter(access_paths).flat_map(
            move |path| -> Pin<Box<dyn Stream<Item = Result<Self::ReadAccessPath, Error>> + 'stream>> {
                #[allow(unreachable_patterns)]
                match (self, path) {
                    #[cfg(feature = "discord")]
                    (Self::Discord(registry), AccessPath::Discord(path)) => Box::pin(
                        traits::RegistryBackend::read(registry, std::slice::from_ref(path))
                            .map(|result| result.map(ReadAccessPath::Discord)),
                    ),
                    #[cfg(feature = "telegram")]
                    (Self::Telegram, _) => unimplemented!("Telegram registry backend"),
                    _ => Box::pin(stream::iter([Err(Error::BackendMismatch)])),
                }
            },
        )
    }
}
