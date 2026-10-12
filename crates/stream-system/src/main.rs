use std::{
    error::Error,
    io::{self, Write},
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use backends::cli::DiscordBackendConfig;
use clap::{Parser, Subcommand};
use file_registry::cli::FileRegistryConfig;
use futures_util::io::AsyncWriteExt;
use stream_registry::cli::StreamRegistryConfig;
use stream_system::{
    BytesFrameAllocator, FileBackend, FileRegistry, FrameBudget, ReadFrame, ReadStream,
    SchedulerConfig, StreamId, StreamRegistry, WriteStream,
    cli::{StreamSchedulerConfig, StreamWriteConfig},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt as _};
use tokio_tcp_pool::{Endpoint, Pool, PoolConfig, Route, rustls};

#[derive(Parser)]
#[command(
    version,
    about = "Upload or download streams using Discord and persistent registries"
)]
struct Args {
    #[command(subcommand)]
    command: Command,

    #[command(flatten)]
    common: CommonArgs,
}

#[derive(Subcommand)]
enum Command {
    /// Upload input and print the persisted stream ID.
    Upload(UploadArgs),
    /// Download a stream or a half-open plaintext byte range.
    Download(DownloadArgs),
}

#[derive(clap::Args)]
struct UploadArgs {
    /// Input file, or - for standard input.
    #[arg(short, long, env = "WD40_INPUT", default_value = "-")]
    input: PathBuf,

    #[command(flatten)]
    writer: StreamWriteConfig,

    /// Size of the reusable input buffer in bytes.
    #[arg(long, env = "WD40_INPUT_BUFFER_SIZE", default_value = "65536")]
    input_buffer_size: NonZeroUsize,
}

#[derive(clap::Args)]
struct DownloadArgs {
    /// Decimal stream ID printed by upload.
    #[arg(env = "WD40_STREAM_ID")]
    stream_id: StreamId,

    /// Output file, or - for standard output. An existing file is truncated.
    #[arg(short, long, env = "WD40_OUTPUT", default_value = "-")]
    output: PathBuf,

    /// First plaintext byte to read, inclusive.
    #[arg(long, env = "WD40_START", default_value = "0")]
    start: u64,

    /// Last plaintext byte boundary, exclusive. Omit to read through EOF.
    #[arg(long, env = "WD40_END")]
    end: Option<u64>,
}

#[derive(clap::Args)]
struct CommonArgs {
    #[command(flatten)]
    file_registry: FileRegistryConfig,

    #[command(flatten)]
    stream_registry: StreamRegistryConfig,

    #[command(flatten)]
    backend: DiscordBackendConfig,

    #[command(flatten)]
    scheduler: StreamSchedulerConfig,

    /// Maximum number of connections in each Discord pool.
    #[arg(long, global = true, env = "WD40_POOL_MAX_OPEN", default_value = "16")]
    pool_max_open: NonZeroUsize,

    /// DNS, TCP and TLS establishment timeout in milliseconds.
    #[arg(
        long,
        global = true,
        env = "WD40_POOL_CONNECT_TIMEOUT_MS",
        default_value = "10000"
    )]
    pool_connect_timeout_ms: u64,

    /// Idle connection expiration in milliseconds. Zero disables reuse.
    #[arg(
        long,
        global = true,
        env = "WD40_POOL_IDLE_TIMEOUT_MS",
        default_value = "60000"
    )]
    pool_idle_timeout_ms: u64,

    /// Optional maximum connection age in milliseconds.
    #[arg(long, global = true, env = "WD40_POOL_MAX_LIFETIME_MS")]
    pool_max_lifetime_ms: Option<u64>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    // ponytail: one runtime polls the stream directly, including its non-Send futures.
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run(args))
}

async fn run(args: Args) -> Result<(), Box<dyn Error>> {
    let Args {
        command,
        common: args,
    } = args;
    // The selected registry supplies upload paths. Remote paths already carry
    // the server's credentials; Local reports missing paths during allocation.
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
    let (backend, registry_backend) = args.backend.build(read_pool, write_pool);
    let file_registry = args.file_registry.open(registry_backend).await?;
    let stream_registry = args.stream_registry.open().await?;

    let budget = args.scheduler.frame_budget();
    let mut carbon_config = args.scheduler.scheduler_config()?;
    match command {
        Command::Upload(args) => {
            upload(
                args,
                file_registry,
                stream_registry,
                backend,
                &budget,
                &mut carbon_config,
            )
            .await
        }
        Command::Download(args) => {
            download(
                args,
                file_registry,
                stream_registry,
                backend,
                &budget,
                &mut carbon_config,
            )
            .await
        }
    }
}

async fn upload(
    args: UploadArgs,
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    backend: FileBackend,
    budget: &FrameBudget,
    carbon_config: &mut SchedulerConfig,
) -> Result<(), Box<dyn Error>> {
    let mut input: Box<dyn tokio::io::AsyncRead + Unpin> = if args.input == Path::new("-") {
        Box::new(tokio::io::stdin())
    } else {
        Box::new(tokio::fs::File::open(&args.input).await?)
    };
    let config = args.writer.writer_config()?;
    let mut allocator = BytesFrameAllocator::new(args.writer.frame_size.get())?;
    let mut stream = WriteStream::new(
        file_registry,
        stream_registry,
        backend,
        config,
        &mut allocator,
        budget,
        carbon_config,
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

async fn download(
    args: DownloadArgs,
    file_registry: FileRegistry,
    stream_registry: StreamRegistry,
    backend: FileBackend,
    budget: &FrameBudget,
    carbon_config: &mut SchedulerConfig,
) -> Result<(), Box<dyn Error>> {
    let mut stream = ReadStream::new(
        file_registry,
        stream_registry,
        backend,
        budget,
        carbon_config,
    )?;
    stream.push_stream(args.stream_id, args.start, args.end)?;
    stream.close_stream();
    let mut output: Box<dyn tokio::io::AsyncWrite + Unpin> = if args.output == Path::new("-") {
        Box::new(tokio::io::stdout())
    } else {
        Box::new(tokio::fs::File::create(&args.output).await?)
    };
    let mut frame = ReadFrame::default();
    while stream.read_frame(&mut frame).await?.is_some() {
        let bytes = frame.buffer.get(frame.selected.clone()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid decoded frame selection",
            )
        })?;
        output.write_all(bytes).await?;
    }
    output.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upload_and_download_range() -> Result<(), Box<dyn Error>> {
        let upload = Args::try_parse_from([
            "stream-system",
            "upload",
            "--input",
            "input.bin",
            "--frame-size",
            "4096",
            "--encryption",
            "aes256",
            "--compression",
            "lz4",
            "--max-file-size",
            "1000000",
            "--webhook",
            "https://example.invalid/webhook",
            "--file-registry-path",
            "files.db",
            "--stream-registry-path",
            "streams.db",
            "--pool-max-open",
            "4",
        ])?;
        assert_eq!(
            upload.common.backend.webhooks,
            ["https://example.invalid/webhook"]
        );
        assert_eq!(upload.common.pool_max_open.get(), 4);
        assert_eq!(
            upload.common.file_registry.file_registry_path,
            Path::new("files.db")
        );
        assert_eq!(
            upload.common.stream_registry.stream_registry_path,
            Path::new("streams.db")
        );
        assert!(matches!(upload.command,
            Command::Upload(args) if args.input == Path::new("input.bin")
                && matches!(args.writer.encryption, stream_system::cli::EncryptionMode::Aes256)
                && matches!(args.writer.compression, stream_system::cli::CompressionMode::Lz4)
                && args.writer.frame_size.get() == 4096
                && args.writer.max_file_size == 1_000_000));
        let download = Args::try_parse_from([
            "stream-system",
            "download",
            "123",
            "--start",
            "100",
            "--end",
            "200",
            "--output",
            "range.bin",
            "--frame-budget",
            "8",
        ])?;
        assert_eq!(download.common.scheduler.frame_budget.get(), 8);
        assert!(matches!(download.command, Command::Download(args)
            if args.stream_id == 123 && args.start == 100 && args.end == Some(200)
                && args.output == Path::new("range.bin")));
        assert!(Args::try_parse_from(["stream-system", "download", "invalid-id"]).is_err());
        Ok(())
    }
}
