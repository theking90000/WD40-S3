use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use backends::traits::FileBackend as _;
use craft_codec::{Compression, Config, Encoder, Encryption as CodecEncryption, Framing, Metadata};
use file_registry::{FileId, WriteAccessPath, traits::FileRegistry as _};
use futures_util::StreamExt;

use crate::{Error, FileBackend, FileRegistry};

type OpenFuture = Pin<Box<dyn Future<Output = Result<OpenedFile, Error>>>>;

enum State {
    Closed,
    Opening(OpenFuture),
    Opened(OpenedFile),
}

// These resources will be used by the frame-writing increment.
#[expect(dead_code, reason = "this increment implements only file opening")]
struct OpenedFile {
    file_id: FileId,
    metadata: Metadata,
    encoder: Encoder,
    file: backends::WriteFile,
}

/// Encryption mode used by each file of a stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encryption {
    /// Store frames without encryption.
    None,
    /// Encrypt with AES-256-GCM and a fresh system-generated key per open attempt.
    Aes256,
}

/// Object-wide settings used by every destination of a stream writer.
#[derive(Clone, Copy, Debug)]
pub struct StreamWriterConfig {
    /// Encryption mode; keys are generated internally for each open attempt.
    pub encryption: Encryption,
    /// Compression mode applied independently to each frame.
    pub compression: Compression,
    /// Fixed raw frame size in bytes, except for the final short frame.
    pub frame_size: u64,
    /// Maximum encoded file payload in bytes, including authentication tags.
    pub max_file_size: u64,
}

/// File destination under construction for CARBON's write scheduler.
///
/// This increment exposes opening only. Frame writing and CARBON's
/// `WriteFile<Vec<u8>>` implementation follow in the next increment.
pub struct WriteFile {
    file_registry: FileRegistry,
    file_backend: FileBackend,
    config: StreamWriterConfig,
    frame_capacity: u32,
    state: State,
}

impl WriteFile {
    /// Validates settings and fixes capacity without allocating an ID or path.
    pub fn new(
        file_registry: FileRegistry,
        file_backend: FileBackend,
        config: StreamWriterConfig,
    ) -> Result<Self, Error> {
        // The placeholder key only validates the profile; no encoder is built here.
        let profile = codec_config(config, [0; 32])?;
        let frame_capacity = capacity_for_limit(&profile, config.max_file_size)?;
        if frame_capacity == 0 {
            return Err(Error::FileTooSmall { limit: config.max_file_size });
        }
        Ok(Self {
            file_registry,
            file_backend,
            config,
            frame_capacity,
            state: State::Closed,
        })
    }

    /// Fixed frame capacity, available before opening the backend writer.
    pub fn frame_capacity(&self) -> u32 {
        self.frame_capacity
    }

    /// Allocates one ID/path supporting the configured size, then opens the backend.
    ///
    /// Every attempt uses `create(1, max_file_size)` and, when encryption is
    /// enabled, a fresh system key. Pending polls retain the same future and
    /// supplied waker. An error resets the destination for a fresh attempt.
    /// Polling an opened destination succeeds without allocating another file.
    pub fn poll_open(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
        let this = self.get_mut();
        loop {
            match &mut this.state {
                State::Closed => {
                    let file_registry = this.file_registry.clone();
                    let file_backend = this.file_backend.clone();
                    let config = this.config;
                    // ponytail: one owned future is polled here; no task or mailbox.
                    this.state = State::Opening(Box::pin(async move {
                        let path = allocate_path(&file_registry, config.max_file_size).await?;
                        let (metadata, encoder) = new_codec(config)?;
                        let file = file_backend
                            .write(&path.access_path)
                            .await
                            .map_err(Error::Backend)?;
                        Ok(OpenedFile {
                            file_id: path.id,
                            metadata,
                            encoder,
                            file,
                        })
                    }));
                }
                State::Opening(future) => {
                    let result = std::task::ready!(future.as_mut().poll(cx));
                    return match result {
                        Ok(file) => {
                            this.state = State::Opened(file);
                            Poll::Ready(Ok(()))
                        }
                        Err(error) => {
                            this.state = State::Closed;
                            Poll::Ready(Err(error))
                        }
                    };
                }
                State::Opened(_file) => return Poll::Ready(Ok(())),
            }
        }
    }
}

async fn allocate_path(
    file_registry: &FileRegistry,
    max_file_size: u64,
) -> Result<WriteAccessPath, Error> {
    let paths = file_registry
        .create(1, max_file_size)
        .await
        .map_err(Error::FileRegistry)?;
    let mut paths = std::pin::pin!(paths);
    paths
        .next()
        .await
        .ok_or(file_registry::Error::MissingWriteAccessPath)
        .map_err(Error::FileRegistry)
}

fn capacity_for_limit(config: &Config, limit: u64) -> Result<u32, Error> {
    let frame_bytes = config
        .max_frame_len()
        .checked_add(config.encryption().tag_len() as u64)
        .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
    let frames = limit
        .checked_div(frame_bytes)
        .ok_or(Error::Codec(craft_codec::Error::InvalidFrameSize))?
        .min(config.max_frames())
        .min(u64::from(u32::MAX));
    u32::try_from(frames).map_err(|_| Error::Codec(craft_codec::Error::Overflow))
}

fn codec_config(settings: StreamWriterConfig, key: [u8; 32]) -> Result<Config, Error> {
    Config::new(
        Framing::Fixed(settings.frame_size),
        settings.compression,
        match settings.encryption {
            Encryption::None => CodecEncryption::None,
            Encryption::Aes256 => CodecEncryption::Aes256Gcm { key },
        },
    )
    .map_err(Error::Codec)
}

fn new_codec(settings: StreamWriterConfig) -> Result<(Metadata, Encoder), Error> {
    let mut key = [0; 32];
    match settings.encryption {
        Encryption::None => {}
        Encryption::Aes256 => getrandom::fill(&mut key).map_err(Error::Random)?,
    }
    let config = codec_config(settings, key)?;
    let metadata = Metadata::new(config);
    let encoder = Encoder::try_from(&metadata).map_err(Error::Codec)?;
    Ok((metadata, encoder))
}

#[cfg(test)]
mod tests {
    use super::*;
    use craft_codec::Decoder;

    #[test]
    fn capacity_respects_codec_overhead() -> Result<(), Box<dyn std::error::Error>> {
        for encryption in [Encryption::None, Encryption::Aes256] {
            for compression in [Compression::None, Compression::Lz4] {
                let config = codec_config(StreamWriterConfig {
                    encryption,
                    compression,
                    frame_size: 4096,
                    max_file_size: 8224,
                }, [0; 32])?;
                let limits = match encryption {
                    Encryption::None => [(4095, 0), (4096, 1), (8191, 1), (8192, 2)],
                    Encryption::Aes256 => [(4111, 0), (4112, 1), (8223, 1), (8224, 2)],
                };
                for (limit, expected) in limits {
                    if capacity_for_limit(&config, limit)? != expected {
                        return Err("capacity ignored codec overhead or partial frame allowance".into());
                    }
                }
            }
        }
        Ok(())
    }

    #[test]
    fn codec_options_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
        for encryption in [Encryption::None, Encryption::Aes256] {
            for compression in [Compression::None, Compression::Lz4] {
                let settings = StreamWriterConfig {
                    encryption,
                    compression,
                    frame_size: 4096,
                    max_file_size: 8224,
                };
                let (mut metadata, mut encoder) = new_codec(settings)?;
                let expected_encryption = match encryption {
                    Encryption::None => matches!(metadata.config().encryption(), CodecEncryption::None),
                    Encryption::Aes256 => matches!(metadata.config().encryption(), CodecEncryption::Aes256Gcm { .. }),
                };
                if metadata.config().compression() != compression
                    || !expected_encryption
                {
                    return Err("metadata does not match requested codec options".into());
                }
                let original = vec![42; 4096];
                let mut encoded = original.clone();
                metadata.push(encoder.encode_frame(0, &mut encoded)?)?;
                if u64::try_from(encoded.len())? > settings.max_file_size {
                    return Err("encoded frame exceeded the configured file size".into());
                }
                let spec = metadata.frame(0).ok_or("encoded frame missing from metadata")?;
                Decoder::try_from(&metadata)?.decode_frame(spec, &mut encoded)?;
                if encoded != original {
                    return Err("configured codec failed its roundtrip".into());
                }
            }
        }
        Ok(())
    }

    #[test]
    fn attempts_use_fresh_keys_and_matching_encoders() -> Result<(), Box<dyn std::error::Error>> {
        let settings = StreamWriterConfig {
            encryption: Encryption::Aes256,
            compression: Compression::None,
            frame_size: 4096,
            max_file_size: 8224,
        };
        let (mut first, mut encoder) = new_codec(settings)?;
        let (second, _) = new_codec(settings)?;
        if first.config().encryption() == second.config().encryption() {
            return Err("attempts reused an AES key".into());
        }
        if first.config().framing() != Framing::Fixed(4096)
            || first.config().compression() != Compression::None
            || first.frame_count() != 0
            || second.frame_count() != 0
        {
            return Err("unexpected initial codec profile".into());
        }
        if !matches!(first.config().encryption(), CodecEncryption::Aes256Gcm { .. }) {
            return Err("AES encryption was not initialized".into());
        }

        let original = vec![42; 4096];
        let mut encoded = original.clone();
        first.push(encoder.encode_frame(0, &mut encoded)?)?;
        let spec = first.frame(0).ok_or("encoded frame missing from metadata")?;
        Decoder::try_from(&first)?.decode_frame(spec, &mut encoded)?;
        if encoded != original {
            return Err("encoder does not match the generated metadata".into());
        }
        if codec_config(StreamWriterConfig { frame_size: 0, ..settings }, [0; 32]).is_ok() {
            return Err("zero frame size was accepted".into());
        }
        Ok(())
    }
}
