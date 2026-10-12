use std::{
    error::Error as StdError,
    sync::{Arc, Barrier, mpsc},
    time::{Duration, Instant},
};

use backends::{
    AccessPath, DiscordAccessPath, DiscordReadAccessPath, DiscordRegistry, RegistryBackend,
};
use file_registry::{FileAccess, LocalRegistry, traits::FileRegistry as _};
use futures_executor::block_on;
use futures_util::StreamExt;
use tokio_tcp_pool::{Endpoint, Pool, Route};

type TestResult<T = ()> = Result<T, Box<dyn StdError + Send + Sync>>;

#[test]
fn parallel_create_store_and_retrieve_across_databases_complete() -> TestResult {
    // Allocating paths and resolving a nonexpired reference perform no network I/O.
    let pool = Pool::builder(Route::Direct {
        target: Endpoint::new("127.0.0.1", 1)?,
    })
    .build()?;
    let backend = RegistryBackend::Discord(DiscordRegistry::new(
        pool,
        vec!["https://discord.com/api/webhooks/test/token".to_owned()],
        Vec::new(),
    ));
    let reference = AccessPath::Discord(DiscordAccessPath::from(
        discord_webhook_storage::DiscordFile {
            id: "test-message".to_owned(),
            url: DiscordReadAccessPath {
                channel_id: 1,
                attachment_id: 2,
                attachment_name: "test.bin".to_owned(),
                ex: u64::MAX,
                is: 0,
                hm: "test-signature".to_owned(),
            },
        },
    ));
    let mut registries = Vec::new();
    for _ in 0..16 {
        let database = sled::Config::new()
            .temporary(true)
            .flush_every_ms(None)
            .open()?;
        registries.push(LocalRegistry::new(database, backend.clone())?);
    }
    let barrier = Arc::new(Barrier::new(registries.len()));
    let (sender, receiver) = mpsc::channel();
    for registry in registries {
        let reference = reference.clone();
        let barrier = barrier.clone();
        let sender = sender.clone();
        std::thread::Builder::new().spawn(move || {
            barrier.wait();
            let result: TestResult = block_on(async {
                for _ in 0..8 {
                    let mut paths = registry.create(1, 1024).await?;
                    let path = paths.next().await.ok_or("create returned no path")?;
                    registry
                        .store(&[FileAccess {
                            id: path.id,
                            access_path: reference.clone(),
                        }])
                        .await?;
                    let mut reads = registry.retrieve(&[path.id]).await?;
                    let read = reads.next().await.ok_or("retrieve returned no path")??;
                    if read.id != path.id {
                        return Err("concurrent store/retrieve changed the file ID".into());
                    }
                }
                Ok(())
            });
            let _ = sender.send(result);
        })?;
    }
    drop(sender);
    let deadline = Instant::now() + Duration::from_secs(10);
    for _ in 0..16 {
        receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))??;
    }
    Ok(())
}
