//! Stream settings composable with `#[command(flatten)]`.

use std::num::{NonZeroUsize, TryFromIntError};

use crate::{
    Compression, Encryption, Error, FrameBudget, SchedulerConfig, StreamWriterConfig, Window,
};

/// Encryption mode applied with a fresh key per file attempt.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum EncryptionMode {
    None,
    Aes256,
}

/// Compression mode applied independently to each frame.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum CompressionMode {
    None,
    Lz4,
}

/// Codec and file size settings for stream writers.
#[derive(clap::Args)]
pub struct StreamWriteConfig {
    /// AES-256 uses a fresh system-generated key for each file attempt.
    #[arg(long, env = "WD40_ENCRYPTION", value_enum, default_value = "none")]
    pub encryption: EncryptionMode,

    /// Compression applied independently to each frame.
    #[arg(long, env = "WD40_COMPRESSION", value_enum, default_value = "none")]
    pub compression: CompressionMode,

    /// Raw frame size in bytes.
    #[arg(long, env = "WD40_FRAME_SIZE", default_value = "65536")]
    pub frame_size: NonZeroUsize,

    /// Maximum encoded file size in bytes, including authentication tags.
    #[arg(long, env = "WD40_MAX_FILE_SIZE", default_value = "20000000",
        value_parser = clap::value_parser!(u64).range(1..=20_000_000))]
    pub max_file_size: u64,
}

impl StreamWriteConfig {
    /// Converts parsed options to the writer's codec settings.
    pub fn writer_config(&self) -> Result<StreamWriterConfig, TryFromIntError> {
        Ok(StreamWriterConfig {
            encryption: match self.encryption {
                EncryptionMode::None => Encryption::None,
                EncryptionMode::Aes256 => Encryption::Aes256,
            },
            compression: match self.compression {
                CompressionMode::None => Compression::None,
                CompressionMode::Lz4 => Compression::Lz4,
            },
            frame_size: u64::try_from(self.frame_size.get())?,
            max_file_size: self.max_file_size,
        })
    }
}

/// CARBON scheduling and shared frame budget settings.
#[derive(clap::Args)]
pub struct StreamSchedulerConfig {
    /// Desired number of frames retained by CARBON.
    #[arg(long, global = true, env = "WD40_TARGET_FRAMES", default_value = "256")]
    pub target_frames: NonZeroUsize,

    /// CARBON discovery horizon, used by reading rather than writing.
    #[arg(
        long,
        global = true,
        env = "WD40_OPEN_AHEAD_FRAMES",
        default_value = "1024"
    )]
    pub open_ahead_frames: usize,

    /// Maximum number of active backend files.
    #[arg(
        long,
        global = true,
        env = "WD40_MAX_ACTIVE_FILES",
        default_value = "16"
    )]
    pub max_active_files: NonZeroUsize,

    /// Retries per file. A nonzero value requires a budget that fits a whole file.
    #[arg(long, global = true, env = "WD40_MAX_RETRIES", default_value = "0")]
    pub max_retries: u32,

    /// Total shared frame budget for this run.
    #[arg(long, global = true, env = "WD40_FRAME_BUDGET", default_value = "256")]
    pub frame_budget: NonZeroUsize,
}

impl StreamSchedulerConfig {
    /// Constructs a validated CARBON scheduler configuration.
    pub fn scheduler_config(&self) -> Result<SchedulerConfig, Error> {
        Ok(SchedulerConfig::new(
            Window::new(
                self.target_frames.get(),
                self.open_ahead_frames,
                self.max_active_files.get(),
            )
            .map_err(Error::Carbon)?,
            self.max_retries,
        ))
    }

    /// Creates a frame budget owned and shared by the application.
    pub fn frame_budget(&self) -> FrameBudget {
        FrameBudget::new(self.frame_budget.get())
    }
}
