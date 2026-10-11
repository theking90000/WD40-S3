//! Fixed-framing codec configuration and CARBON upload destinations.

use std::{
    future::Future,
    io,
    pin::Pin,
    task::{Context, Poll},
};

use backends::traits::FileBackend as _;
use craft_codec::{
    Compression, Config, Encoder, Encryption as CodecEncryption, FrameSizes, Framing, Metadata,
};
use file_registry::{FileAccess, FileId, WriteAccessPath, traits::FileRegistry as _};
use futures_io::AsyncWrite;
use futures_util::StreamExt;
use stream_registry::{StreamPartId, traits::StreamRegistry as _};

use crate::{Error, FileBackend, FileRegistry, StreamRegistry};

type OpenFuture = Pin<Box<dyn Future<Output = Result<OpenedFile, Error>>>>;
type CloseFuture = Pin<Box<dyn Future<Output = Result<StreamPartId, Error>>>>;

#[allow(
    clippy::large_enum_variant,
    reason = "Keep the opened file inline without an additional allocation per attempt"
)]
enum State {
    Closed,
    Opening(OpenFuture),
    Opened(OpenedFile),
    Closing(CloseFuture),
}

struct OpenedFile {
    file_id: FileId,
    metadata: Metadata,
    encoder: Encoder,
    file: backends::WriteFile,
    written: usize,
    buffer: Option<(Vec<u8>, FrameSizes)>,
}

impl OpenedFile {
    fn poll_write(&mut self, cx: &mut Context<'_>, frame: &[u8]) -> Poll<Result<(), Error>> {
        if self.buffer.is_none() {
            // ponytail: CRAFT owns allocation and encoding; pending polls retain its result.
            self.buffer = Some(
                self.encoder
                    .encode_frame_alloc(self.metadata.frame_count(), frame)
                    .map_err(Error::Codec)?,
            );
            self.written = 0;
        }

        let (buffer, sizes) = self
            .buffer
            .as_ref()
            .ok_or(Error::Codec(craft_codec::Error::InvalidMetadata))?;
        while self.written < buffer.len() {
            let remaining = buffer
                .get(self.written..)
                .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
            let count = std::task::ready!(Pin::new(&mut self.file).poll_write(cx, remaining))
                .map_err(Error::Io)?;
            if count == 0 {
                return Poll::Ready(Err(Error::Io(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "backend accepted no bytes from a nonempty frame",
                ))));
            }
            if count > remaining.len() {
                return Poll::Ready(Err(Error::Io(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "backend reported more written bytes than supplied",
                ))));
            }
            self.written = self
                .written
                .checked_add(count)
                .ok_or(Error::Codec(craft_codec::Error::Overflow))?;
        }

        self.metadata.push(*sizes).map_err(Error::Codec)?;
        self.buffer = None;
        self.written = 0;
        Poll::Ready(Ok(()))
    }
}

/// Encryption mode used by each file of a stream.
///
/// This enum chooses a mode, not a caller-provided key. Generated keys are
/// retained in the file's CRAFT metadata stored by the stream registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Encryption {
    /// Store frames without encryption.
    None,
    /// Encrypt with AES-256-GCM and a fresh system-generated key per open attempt.
    Aes256,
}

/// Object-wide settings used by every destination of a stream writer.
///
/// Frame size and encoded file size are separate limits. Capacity is fixed
/// before opening, using a full plaintext frame plus codec overhead. Compression
/// savings do not increase that advertised frame capacity. Construct the
/// caller's [`crate::BytesFrameAllocator`] with the same frame size.
#[derive(Clone, Copy, Debug)]
pub struct StreamWriterConfig {
    /// Encryption mode; keys are generated internally for each open attempt.
    /// An encrypted retry uses a new key as well as a new file ID/access path.
    pub encryption: Encryption,
    /// Compression mode applied independently to each frame.
    /// [`Compression::None`] stores uncompressed payloads; [`Compression::Lz4`]
    /// lets CRAFT encode independent blocks with its uncompressed fallback.
    pub compression: Compression,
    /// Fixed raw frame size in bytes, except for the final short frame.
    /// Must be nonzero and supported by CRAFT. This is a plaintext size, not the
    /// encrypted/compressed frame's stored size.
    pub frame_size: u64,
    /// Maximum encoded file payload in bytes, including authentication tags.
    /// Passed to file allocation on each open. Must fit at least one full
    /// configured frame with overhead. Does not include registry metadata or
    /// the backend's transport envelope.
    pub max_file_size: u64,
}

/// File destination for CARBON's write scheduler.
///
/// Implements [`carbon_io::WriteFile`] for `Vec<u8>` plaintext frames. The
/// scheduler owns/replays those frames; this adapter keeps an encoded work copy
/// during partial backend writes. Opening creates one file; closing finishes
/// the upload, stores its access path and inserts its metadata as a stream part.
/// The close output is a part ID, not a complete [`crate::StreamId`].
pub struct WriteFile {
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    file_backend: FileBackend,
    config: StreamWriterConfig,
    frame_capacity: u32,
    state: State,
}

impl WriteFile {
    /// Validates settings and fixes capacity without allocating an ID or path.
    ///
    /// Retains owned registry/backend handles and does not start backend I/O.
    /// Capacity is limited by the file size, CRAFT's profile and CARBON's `u32`
    /// frame contract. A fresh encryption key is generated later on opening.
    ///
    /// # Errors
    /// Returns [`Error::Codec`] for invalid framing/size arithmetic or
    /// [`Error::FileTooSmall`] if the destination cannot fit one full frame.
    pub fn new(
        file_registry: FileRegistry,
        stream_registry: StreamRegistry,
        file_backend: FileBackend,
        config: StreamWriterConfig,
    ) -> Result<Self, Error> {
        // The placeholder key only validates the profile; no encoder is built here.
        let profile = codec_config(config, [0; 32])?;
        let frame_capacity = capacity_for_limit(&profile, config.max_file_size)?;
        if frame_capacity == 0 {
            return Err(Error::FileTooSmall {
                limit: config.max_file_size,
            });
        }
        Ok(Self {
            file_registry,
            stream_registry,
            file_backend,
            config,
            frame_capacity,
            state: State::Closed,
        })
    }
}

impl carbon_io::WriteFile<Vec<u8>> for WriteFile {
    /// Codec, registry, backend, byte-I/O and lifecycle failures.
    type Error = Error;
    /// Persisted stream part ID linking this file and its CRAFT metadata.
    type Output = StreamPartId;

    /// Fixed frame capacity, available before opening the backend writer.
    /// Remains unchanged across fresh open attempts and replay.
    fn frame_capacity(&self) -> u32 {
        self.frame_capacity
    }

    /// Allocates one ID/path supporting the configured size, then opens the backend.
    ///
    /// Every attempt uses `create(1, max_file_size)` and, when encryption is
    /// enabled, a fresh system key. Pending polls retain the same future and
    /// supplied waker. An error resets the destination for a fresh attempt.
    /// Polling an opened destination succeeds without allocating another file.
    /// Registry lookup and backend opening use the supplied context directly.
    /// A destination already closing must not be reopened until that operation
    /// has returned; such polling cancels closing and reports `FileNotOpen`.
    ///
    /// # Errors
    /// Reports file allocation, system key generation, codec initialization or
    /// backend opening failures. A failed opening can be retried by CARBON.
    fn poll_open(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Error>> {
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
                            written: 0,
                            buffer: None,
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
                State::Closing(_) => {
                    this.state = State::Closed;
                    return Poll::Ready(Err(Error::FileNotOpen));
                }
            }
        }
    }

    /// Encodes one borrowed plaintext frame and writes its complete stored bytes.
    ///
    /// Leaves the input unchanged. CRAFT allocates the encoded work buffer once
    /// for this logical frame; pending polls reuse it and its byte offset.
    /// Re-poll with the same logical frame until `Ready`. Metadata advances only
    /// after all stored bytes have been accepted by the backend.
    ///
    /// # Errors
    /// Reports codec or byte-I/O failures, `FrameCapacityExceeded` when full and
    /// `FileNotOpen` outside the opened state. A write error resets the file for
    /// a fresh open; CARBON controls replay according to its retry configuration.
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        frame: &Vec<u8>,
    ) -> Poll<Result<(), Error>> {
        let this = self.get_mut();
        let result = match &mut this.state {
            State::Opened(opened) => {
                if opened.metadata.frame_count() >= u64::from(this.frame_capacity) {
                    Poll::Ready(Err(Error::FrameCapacityExceeded))
                } else {
                    opened.poll_write(cx, frame)
                }
            }
            State::Closed | State::Opening(_) | State::Closing(_) => {
                Poll::Ready(Err(Error::FileNotOpen))
            }
        };
        if matches!(&result, Poll::Ready(Err(_))) {
            this.state = State::Closed;
        }
        result
    }

    /// Finishes the upload, registers the file, then persists and returns its stream part ID.
    /// Pending polls retain the same finalization future. Errors permit a fresh open attempt.
    ///
    /// Finalization calls the backend's consuming finish operation, then
    /// `FileRegistry::store`, then `StreamRegistry::insert`. The metadata moves
    /// into the stream registry. A successful result acknowledges this part;
    /// [`crate::WriteStream`] stores the complete manifest separately.
    ///
    /// # Errors
    /// Returns `FrameWritePending` if an encoded frame remains unfinished,
    /// `FileNotOpen` outside the required lifecycle, or backend/registry failures.
    /// There is no rollback of uploads or registrations that already completed.
    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<Self::Output, Error>> {
        let this = self.get_mut();
        if matches!(&this.state, State::Opened(_)) {
            let State::Opened(opened) = std::mem::replace(&mut this.state, State::Closed) else {
                return Poll::Ready(Err(Error::FileNotOpen));
            };
            if opened.buffer.is_some() {
                return Poll::Ready(Err(Error::FrameWritePending));
            }
            let file_registry = this.file_registry.clone();
            let stream_registry = this.stream_registry.clone();
            // ponytail: finish also closes the backend; keep one owned future across polls.
            this.state = State::Closing(Box::pin(async move {
                let access_path = opened.file.finish().await.map_err(Error::Backend)?;
                let file = FileAccess {
                    id: opened.file_id,
                    access_path,
                };
                file_registry
                    .store(&[file])
                    .await
                    .map_err(Error::FileRegistry)?;
                stream_registry
                    .insert(opened.file_id, opened.metadata)
                    .await
                    .map_err(Error::StreamRegistry)
            }));
        }

        match &mut this.state {
            State::Closing(future) => {
                let result = std::task::ready!(future.as_mut().poll(cx));
                this.state = State::Closed;
                Poll::Ready(result)
            }
            State::Closed | State::Opening(_) | State::Opened(_) => {
                this.state = State::Closed;
                Poll::Ready(Err(Error::FileNotOpen))
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
                let config = codec_config(
                    StreamWriterConfig {
                        encryption,
                        compression,
                        frame_size: 4096,
                        max_file_size: 8224,
                    },
                    [0; 32],
                )?;
                let limits = match encryption {
                    Encryption::None => [(4095, 0), (4096, 1), (8191, 1), (8192, 2)],
                    Encryption::Aes256 => [(4111, 0), (4112, 1), (8223, 1), (8224, 2)],
                };
                for (limit, expected) in limits {
                    if capacity_for_limit(&config, limit)? != expected {
                        return Err(
                            "capacity ignored codec overhead or partial frame allowance".into()
                        );
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
                    Encryption::None => {
                        matches!(metadata.config().encryption(), CodecEncryption::None)
                    }
                    Encryption::Aes256 => matches!(
                        metadata.config().encryption(),
                        CodecEncryption::Aes256Gcm { .. }
                    ),
                };
                if metadata.config().compression() != compression || !expected_encryption {
                    return Err("metadata does not match requested codec options".into());
                }
                let original = vec![42; 4096];
                let (mut encoded, sizes) = encoder.encode_frame_alloc(0, &original)?;
                metadata.push(sizes)?;
                if u64::try_from(encoded.len())? > settings.max_file_size {
                    return Err("encoded frame exceeded the configured file size".into());
                }
                let spec = metadata
                    .frame(0)
                    .ok_or("encoded frame missing from metadata")?;
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
        if !matches!(
            first.config().encryption(),
            CodecEncryption::Aes256Gcm { .. }
        ) {
            return Err("AES encryption was not initialized".into());
        }

        let original = vec![42; 4096];
        let (mut encoded, sizes) = encoder.encode_frame_alloc(0, &original)?;
        first.push(sizes)?;
        let spec = first
            .frame(0)
            .ok_or("encoded frame missing from metadata")?;
        Decoder::try_from(&first)?.decode_frame(spec, &mut encoded)?;
        if encoded != original {
            return Err("encoder does not match the generated metadata".into());
        }
        if codec_config(
            StreamWriterConfig {
                frame_size: 0,
                ..settings
            },
            [0; 32],
        )
        .is_ok()
        {
            return Err("zero frame size was accepted".into());
        }
        Ok(())
    }
}
