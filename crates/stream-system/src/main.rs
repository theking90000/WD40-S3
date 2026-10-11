use std::{
    error::Error,
    io::{self, Write},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use backends::{DiscordFile, DiscordRegistry, RegistryBackend};
use clap::{Parser, ValueEnum};
use futures_util::io::AsyncWriteExt;
use stream_system::{
    BytesFrameAllocator, Compression, Encryption, FileBackend, FileRegistry, FrameBudget,
    SchedulerConfig, StreamRegistry, StreamWriterConfig, Window, WriteStream,
};
use tokio::io::AsyncReadExt;
use tokio_tcp_pool::{Endpoint, Pool, PoolConfig, Route, rustls};

#[derive(Clone, Copy, ValueEnum)]
enum EncryptionMode {
    None,
    Aes256,
}

#[derive(Clone, Copy, ValueEnum)]
enum CompressionMode {
    None,
    Lz4,
}

#[derive(Parser)]
#[command(version, about = "Upload a stream to Discord and persist its manifest")]
struct Args {
    /// Input file, or - for standard input.
    #[arg(short, long, env = "WD40_INPUT", default_value = "-")]
    input: PathBuf,

    /// Persistent file registry database directory.
    #[arg(
        long,
        env = "WD40_FILE_REGISTRY_PATH",
        default_value = "./data/file-registry"
    )]
    file_registry_path: PathBuf,

    /// Persistent stream registry database directory.
    #[arg(
        long,
        env = "WD40_STREAM_REGISTRY_PATH",
        default_value = "./data/stream-registry"
    )]
    stream_registry_path: PathBuf,

    /// Discord webhook URL. Repeat the option or use comma-separated environment values.
    #[arg(
        long = "webhook",
        env = "WD40_DISCORD_WEBHOOK_URLS",
        required = true,
        value_delimiter = ',',
        hide_env_values = true
    )]
    webhooks: Vec<String>,

    /// Optional bot token for URL renewal. Repeat the option or use comma-separated values.
    #[arg(
        long = "bot-token",
        env = "WD40_DISCORD_BOT_TOKENS",
        value_delimiter = ',',
        hide_env_values = true
    )]
    bot_tokens: Vec<String>,

    /// AES-256 uses a fresh system-generated key for each file attempt.
    #[arg(long, env = "WD40_ENCRYPTION", value_enum, default_value = "none")]
    encryption: EncryptionMode,

    /// Compression applied independently to each frame.
    #[arg(long, env = "WD40_COMPRESSION", value_enum, default_value = "none")]
    compression: CompressionMode,

    /// Raw frame size in bytes.
    #[arg(long, env = "WD40_FRAME_SIZE", default_value = "65536")]
    frame_size: NonZeroUsize,

    /// Maximum encoded file size in bytes, including authentication tags.
    #[arg(long, env = "WD40_MAX_FILE_SIZE", default_value = "20000000",
        value_parser = clap::value_parser!(u64).range(1..=20_000_000))]
    max_file_size: u64,

    /// Desired number of frames retained by CARBON.
    #[arg(long, env = "WD40_TARGET_FRAMES", default_value = "256")]
    target_frames: NonZeroUsize,

    /// CARBON discovery horizon, used by reading rather than writing.
    #[arg(long, env = "WD40_OPEN_AHEAD_FRAMES", default_value = "1024")]
    open_ahead_frames: usize,

    /// Maximum number of active file destinations.
    #[arg(long, env = "WD40_MAX_ACTIVE_FILES", default_value = "16")]
    max_active_files: NonZeroUsize,

    /// Retries per file. A nonzero value requires a budget that fits a whole file.
    #[arg(long, env = "WD40_MAX_RETRIES", default_value = "0")]
    max_retries: u32,

    /// Total shared frame budget for this run.
    #[arg(long, env = "WD40_FRAME_BUDGET", default_value = "256")]
    frame_budget: NonZeroUsize,

    /// Size of the reusable input buffer in bytes.
    #[arg(long, env = "WD40_INPUT_BUFFER_SIZE", default_value = "65536")]
    input_buffer_size: NonZeroUsize,

    /// Maximum number of connections in each Discord pool.
    #[arg(long, env = "WD40_POOL_MAX_OPEN", default_value = "16")]
    pool_max_open: NonZeroUsize,

    /// DNS, TCP and TLS establishment timeout in milliseconds.
    #[arg(long, env = "WD40_POOL_CONNECT_TIMEOUT_MS", default_value = "10000")]
    pool_connect_timeout_ms: u64,

    /// Idle connection expiration in milliseconds. Zero disables reuse.
    #[arg(long, env = "WD40_POOL_IDLE_TIMEOUT_MS", default_value = "60000")]
    pool_idle_timeout_ms: u64,

    /// Optional maximum connection age in milliseconds.
    #[arg(long, env = "WD40_POOL_MAX_LIFETIME_MS")]
    pool_max_lifetime_ms: Option<u64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    // ponytail: one runtime polls the writer directly, including its non-Send futures.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run(args))
}

async fn run(args: Args) -> Result<(), Box<dyn Error>> {
    let mut input: Box<dyn tokio::io::AsyncRead + Unpin> = if args.input == Path::new("-") {
        Box::new(tokio::io::stdin())
    } else {
        Box::new(tokio::fs::File::open(&args.input).await?)
    };

    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()?
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    let tls = Arc::new(tls);
    let pool_config = PoolConfig {
        max_open: args.pool_max_open.get(),
        connect_timeout: Duration::from_millis(args.pool_connect_timeout_ms),
        idle_timeout: Duration::from_millis(args.pool_idle_timeout_ms),
        max_lifetime: args.pool_max_lifetime_ms.map(Duration::from_millis),
    };
    let pool = |host| -> Result<Pool, Box<dyn Error>> {
        Ok(Pool::builder(Route::Direct {
            target: Endpoint::new(host, 443)?,
        })
        .config(pool_config.clone())
        .tls(Arc::clone(&tls))
        .build()?)
    };
    let write_pool = pool("discord.com")?;
    let read_pool = pool("cdn.discordapp.com")?;
    let backend = FileBackend::Discord(DiscordFile::new(read_pool, write_pool.clone()));
    let registry_backend = RegistryBackend::Discord(DiscordRegistry::new(
        write_pool,
        args.webhooks,
        args.bot_tokens,
    ));
    let file_registry = FileRegistry::Local(file_registry::LocalRegistry::new(
        sled::open(&args.file_registry_path)?,
        registry_backend,
    )?);
    let stream_registry = StreamRegistry::Local(stream_registry::LocalRegistry::new(sled::open(
        &args.stream_registry_path,
    )?)?);

    let config = StreamWriterConfig {
        encryption: match args.encryption {
            EncryptionMode::None => Encryption::None,
            EncryptionMode::Aes256 => Encryption::Aes256,
        },
        compression: match args.compression {
            CompressionMode::None => Compression::None,
            CompressionMode::Lz4 => Compression::Lz4,
        },
        frame_size: u64::try_from(args.frame_size.get())?,
        max_file_size: args.max_file_size,
    };
    let mut allocator = BytesFrameAllocator::new(args.frame_size.get())?;
    let budget = FrameBudget::new(args.frame_budget.get());
    let mut carbon_config = SchedulerConfig::new(
        Window::new(
            args.target_frames.get(),
            args.open_ahead_frames,
            args.max_active_files.get(),
        )?,
        args.max_retries,
    );
    let mut stream = WriteStream::new(
        file_registry,
        stream_registry,
        backend,
        config,
        &mut allocator,
        &budget,
        &mut carbon_config,
    )?;
    let mut buffer = vec![0; args.input_buffer_size.get()];
    loop {
        let count = tokio::select! {
            biased;
            result = stream.progress() => {
                result?;
                continue;
            }
            result = input.read(&mut buffer) => result?,
        };
        if count == 0 {
            break;
        }
        let bytes = buffer.get(..count).ok_or_else(|| {
            io::Error::other("input reported more bytes than the buffer can hold")
        })?;
        stream.write_all(bytes).await?;
    }
    stream.close().await?;
    let id = stream
        .stream_id()
        .ok_or_else(|| io::Error::other("closed stream has no persisted manifest ID"))?;
    writeln!(io::stdout(), "{id}")?;
    Ok(())
}
