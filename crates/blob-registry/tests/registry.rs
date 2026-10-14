use std::{
    collections::BTreeMap,
    error::Error as StdError,
    sync::{Arc, Barrier, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use blob_registry::{
    BlobId, BlobRegistry, Error, LocalRegistry, StreamSlice, traits::BlobRegistry as _,
};
use futures_executor::block_on;
use futures_util::{StreamExt, TryStreamExt};

type TestResult<T = ()> = Result<T, Box<dyn StdError>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn temporary_database() -> TestResult<sled::Db> {
    Ok(sled::Config::new().temporary(true).open()?)
}

fn require_send<T: Send>(_: &T) {}

#[test]
fn parallel_mutations_across_independent_databases_complete() -> TestResult {
    let mut registries = Vec::new();
    for _ in 0..16 {
        let database = sled::Config::new()
            .temporary(true)
            .flush_every_ms(None)
            .open()?;
        registries.push(LocalRegistry::new(database)?);
    }
    let barrier = Arc::new(Barrier::new(registries.len()));
    let (sender, receiver) = mpsc::channel();
    for registry in registries {
        let barrier = barrier.clone();
        let sender = sender.clone();
        std::thread::Builder::new().spawn(move || {
            barrier.wait();
            let result: Result<Vec<u64>, Error> = block_on(async {
                let mut lengths = Vec::new();
                for _ in 0..8 {
                    let blob = registry.create().await?;
                    registry.save_part(blob, 41, 3, 1).await?;
                    lengths.push(registry.finalize(blob, vec![1]).await?);
                }
                Ok(lengths)
            });
            let _ = sender.send(result);
        })?;
    }
    drop(sender);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    for _ in 0..16 {
        let lengths = receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))??;
        check(
            lengths == vec![3; 8],
            "parallel mutations changed blob lengths",
        )?;
    }
    Ok(())
}

#[test]
fn can_save_checks_open_state_without_reserving() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = BlobRegistry::Local(LocalRegistry::new(database.clone())?);
        check(
            matches!(
                registry.can_save(u128::MAX).await,
                Err(Error::BlobNotFound(u128::MAX))
            ),
            "can_save accepted a missing blob",
        )?;
        let blob = registry.create().await?;
        let other = registry.create().await?;
        let can_save = registry.can_save(blob);
        require_send(&can_save);
        can_save.await?;
        registry.can_save(blob).await?;
        check(
            database.open_tree("blob-parts")?.is_empty(),
            "can_save reserved a part",
        )?;
        registry.save_part(blob, 41, 3, 1).await?;
        registry.can_save(blob).await?;
        registry.can_save(other).await?;
        registry.finalize(blob, vec![1]).await?;
        check(
            matches!(
                registry.can_save(blob).await,
                Err(Error::BlobFinalized(id)) if id == blob
            ),
            "can_save accepted a finalized blob",
        )?;
        check(
            matches!(
                registry.save_part(blob, 42, 5, 2).await,
                Err(Error::BlobFinalized(id)) if id == blob
            ),
            "a successful precheck bypassed finalization",
        )?;
        Ok(())
    })
}

#[test]
fn enum_preserves_order_repetitions_and_owns_read_plans() -> TestResult {
    block_on(async {
        let registry = BlobRegistry::Local(LocalRegistry::new(temporary_database()?)?);
        let create = registry.create();
        require_send(&create);
        let blob = create.await?;
        let other = registry.create().await?;
        check(blob != other, "create reused a blob ID")?;
        let save = registry.save_part(blob, 41, 10, 1);
        require_send(&save);
        save.await?;
        registry.save_part(blob, 42, 20, 2).await?;
        registry.save_part(blob, 43, 100, 3).await?;
        let finalize = registry.finalize(blob, vec![2, 1, 2]);
        require_send(&finalize);
        check(
            finalize.await? == 50,
            "final length includes unselected parts",
        )?;
        let read = registry.read(blob, 15..35);
        require_send(&read);
        let slices = read.await?;
        require_send(&slices);
        drop(registry);
        check(
            slices.try_collect::<Vec<_>>().await?
                == vec![
                    StreamSlice {
                        stream_id: 42,
                        skip: 15,
                        length: 5,
                    },
                    StreamSlice {
                        stream_id: 41,
                        skip: 0,
                        length: 10,
                    },
                    StreamSlice {
                        stream_id: 42,
                        skip: 0,
                        length: 5,
                    },
                ],
            "read changed the ordering, repetitions or local ranges",
        )
    })
}

#[test]
fn every_valid_range_reconstructs_exact_plaintext() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(temporary_database()?)?;
        let blob = registry.create().await?;
        registry.save_part(blob, 41, 3, 1).await?;
        registry.save_part(blob, 42, 0, 2).await?;
        registry.save_part(blob, 43, 5, 3).await?;
        registry.finalize(blob, vec![2, 3, 1, 2, 3, 2]).await?;
        let streams: BTreeMap<_, &[u8]> = BTreeMap::from([
            (41_u128, b"abc".as_slice()),
            (42, b"".as_slice()),
            (43, b"12345".as_slice()),
        ]);
        let plaintext = b"12345abc12345";
        let length = u64::try_from(plaintext.len())?;
        for start in 0..=length {
            for end in start..=length {
                let mut actual = Vec::new();
                let mut slices = registry.read(blob, start..end).await?;
                while let Some(slice) = slices.next().await {
                    let slice = slice?;
                    check(slice.length > 0, "read emitted an empty slice")?;
                    let bytes = streams.get(&slice.stream_id).ok_or("unknown stream")?;
                    let slice_end = slice
                        .skip
                        .checked_add(slice.length)
                        .ok_or("slice overflow")?;
                    actual.extend_from_slice(
                        bytes
                            .get(usize::try_from(slice.skip)?..usize::try_from(slice_end)?)
                            .ok_or("slice exceeds stream length")?,
                    );
                }
                let expected = plaintext
                    .get(usize::try_from(start)?..usize::try_from(end)?)
                    .ok_or("invalid expected range")?;
                check(
                    actual == expected,
                    "read plan reconstructed incorrect plaintext",
                )?;
            }
        }
        Ok(())
    })
}

#[test]
fn part_ids_are_scoped_and_finalized_blobs_are_immutable() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(temporary_database()?)?;
        let blob = registry.create().await?;
        let other = registry.create().await?;
        registry.save_part(blob, 41, 3, 1).await?;
        registry.save_part(other, 42, 5, 1).await?;
        registry.save_part(blob, 43, 8, 1).await?;
        check(
            matches!(registry.read(blob, 0..0).await,
                Err(Error::BlobNotFinalized(id)) if id == blob),
            "read accepted an open blob",
        )?;
        check(
            registry.finalize(blob, vec![1]).await? == 8,
            "replacement did not update length",
        )?;
        check(
            registry.finalize(other, vec![1]).await? == 5,
            "part IDs crossed blobs",
        )?;
        check(
            registry.finalize(blob, vec![1]).await? == 8,
            "identical finalize retry failed",
        )?;
        check(
            matches!(registry.finalize(blob, vec![]).await,
                Err(Error::BlobFinalized(id)) if id == blob),
            "finalize changed a published manifest",
        )?;
        check(
            matches!(registry.save_part(blob, 44, 8, 2).await,
                Err(Error::BlobFinalized(id)) if id == blob),
            "save_part accepted a finalized blob",
        )?;
        check(
            matches!(registry.save_part(blob, 45, 1, 1).await,
                Err(Error::BlobFinalized(id)) if id == blob),
            "save_part replaced a finalized part",
        )?;
        check(
            registry
                .read(blob, 0..8)
                .await?
                .try_collect::<Vec<_>>()
                .await?
                == vec![StreamSlice {
                    stream_id: 43,
                    skip: 0,
                    length: 8,
                }],
            "rejected writes changed a finalized read",
        )
    })
}

#[test]
fn missing_parts_and_overflow_leave_blobs_open() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(temporary_database()?)?;
        let blob = registry.create().await?;
        let other = registry.create().await?;
        registry.save_part(blob, 41, 1, 1).await?;
        registry.save_part(other, 42, 4, 2).await?;
        check(
            matches!(registry.finalize(blob, vec![1, 2]).await,
                Err(Error::PartNotFound { blob_id, part_id: 2 }) if blob_id == blob),
            "finalize accepted a part saved in another blob",
        )?;
        registry.save_part(blob, 43, u64::MAX, 2).await?;
        check(
            matches!(
                registry.finalize(blob, vec![1, 2]).await,
                Err(Error::LengthOverflow)
            ),
            "finalize accepted an overflowing length",
        )?;
        registry.save_part(blob, 44, 2, 3).await?;
        check(
            registry.finalize(blob, vec![3, 1]).await? == 3,
            "failed finalization closed blob",
        )?;
        let maximum = registry.create().await?;
        registry.save_part(maximum, 43, u64::MAX, 1).await?;
        check(
            registry.finalize(maximum, vec![1]).await? == u64::MAX,
            "maximum length rejected",
        )?;
        let start = u64::MAX.checked_sub(1).ok_or("maximum length underflow")?;
        check(
            registry
                .read(maximum, start..u64::MAX)
                .await?
                .try_collect::<Vec<_>>()
                .await?
                == vec![StreamSlice {
                    stream_id: 43,
                    skip: start,
                    length: 1,
                }],
            "maximum-length range was truncated",
        )
    })
}

#[test]
fn missing_blobs_invalid_ranges_and_empty_blobs() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(temporary_database()?)?;
        check(
            matches!(
                registry.save_part(u128::MAX, 41, 3, 1).await,
                Err(Error::BlobNotFound(u128::MAX))
            ),
            "save_part accepted a missing blob",
        )?;
        check(
            matches!(
                registry.finalize(u128::MAX, vec![]).await,
                Err(Error::BlobNotFound(u128::MAX))
            ),
            "finalize accepted a missing blob",
        )?;
        check(
            matches!(
                registry.read(u128::MAX, 0..0).await,
                Err(Error::BlobNotFound(u128::MAX))
            ),
            "read accepted a missing blob",
        )?;
        let empty = registry.create().await?;
        registry.save_part(empty, 41, 10, 1).await?;
        check(
            registry.finalize(empty, vec![]).await? == 0,
            "empty selection has content",
        )?;
        check(
            registry.read(empty, 0..0).await?.next().await.is_none(),
            "empty blob emitted slices",
        )?;
        check(
            matches!(
                registry.read(empty, 0..1).await,
                Err(Error::InvalidRange { .. })
            ),
            "read exceeded empty blob length",
        )?;
        let blob = registry.create().await?;
        registry.save_part(blob, 41, 10, 1).await?;
        registry.finalize(blob, vec![1]).await?;
        check(
            matches!(
                registry
                    .read(blob, std::ops::Range { start: 8, end: 2 })
                    .await,
                Err(Error::InvalidRange { .. })
            ),
            "read accepted reversed range",
        )?;
        check(
            matches!(
                registry.read(blob, 0..11).await,
                Err(Error::InvalidRange { .. })
            ),
            "read exceeded blob length",
        )?;
        check(
            matches!(
                registry.read(blob, 11..11).await,
                Err(Error::InvalidRange { .. })
            ),
            "read accepted an empty range beyond EOF",
        )?;
        check(
            registry.read(blob, 10..10).await?.next().await.is_none(),
            "EOF emitted slices",
        )
    })
}

#[test]
fn corrupted_parts_abort_finalization_and_terminate_lazy_reads() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let blob = registry.create().await?;
        registry.save_part(blob, 41, 3, 1).await?;
        registry.save_part(blob, 42, 5, 2).await?;
        let mut key = Vec::from(blob.to_be_bytes());
        key.extend_from_slice(&2_u128.to_be_bytes());
        let tree = database.open_tree("blob-parts")?;
        let original = tree
            .insert(key.as_slice(), vec![0xff])?
            .ok_or("part not stored")?;
        check(
            matches!(
                registry.finalize(blob, vec![1, 2]).await,
                Err(Error::Serialization(_))
            ),
            "finalize accepted corrupt metadata",
        )?;
        registry.save_part(blob, 43, 1, 3).await?;
        tree.insert(key.as_slice(), original)?;
        registry.finalize(blob, vec![1, 2, 1]).await?;
        let mut slices = registry.read(blob, 0..11).await?;
        tree.insert(key.as_slice(), vec![0xff])?;
        check(
            matches!(slices.next().await, Some(Ok(slice)) if slice.stream_id == 41),
            "read failed before preceding slice",
        )?;
        match slices.next().await {
            Some(Err(error @ Error::Serialization(_))) => {
                check(
                    error.source().is_some(),
                    "serialization error lost its cause",
                )?;
            }
            _ => return Err("read did not report corrupt metadata".into()),
        }
        check(
            slices.next().await.is_none(),
            "read continued after corruption",
        )?;
        tree.remove(key)?;
        let mut slices = registry.read(blob, 3..8).await?;
        check(
            matches!(slices.next().await,
            Some(Err(Error::PartNotFound { blob_id, part_id: 2 })) if blob_id == blob),
            "read did not report a missing part",
        )?;
        check(
            slices.next().await.is_none(),
            "read continued after missing part",
        )
    })
}

#[test]
fn concurrent_saves_and_finalization_preserve_blob_state() -> TestResult {
    let database = temporary_database()?;
    let registry = LocalRegistry::new(database.clone())?;
    for _ in 0..16 {
        let blob = block_on(registry.create())?;
        let barrier = Arc::new(Barrier::new(2));
        let (first, second) = std::thread::scope(|scope| {
            let left = registry.clone();
            let right = registry.clone();
            let left_barrier = barrier.clone();
            let a = scope.spawn(move || {
                left_barrier.wait();
                block_on(left.save_part(blob, 41, 3, 1))
            });
            let b = scope.spawn(move || {
                barrier.wait();
                block_on(right.save_part(blob, 42, 5, 1))
            });
            (a.join(), b.join())
        });
        first.map_err(|_| "save thread panicked")??;
        second.map_err(|_| "save thread panicked")??;
        let key: Vec<_> = blob
            .to_be_bytes()
            .into_iter()
            .chain(1_u128.to_be_bytes())
            .collect();
        let data = database
            .open_tree("blob-parts")?
            .get(key)?
            .ok_or("missing saved part")?;
        let part: blob_registry::BlobPart = postcard::from_bytes(&data)?;
        check(
            matches!((part.stream_id, part.length), (41, 3) | (42, 5)),
            "concurrent saves mixed stream reference and length",
        )?;
        let expected_length = part.length.checked_add(7).ok_or("test length overflow")?;
        let barrier = Arc::new(Barrier::new(2));
        let (saved, finalized) = std::thread::scope(|scope| {
            let left = registry.clone();
            let right = registry.clone();
            let left_barrier = barrier.clone();
            let a = scope.spawn(move || {
                left_barrier.wait();
                block_on(left.save_part(blob, 43, 7, 2))
            });
            let b = scope.spawn(move || {
                barrier.wait();
                block_on(right.finalize(blob, vec![1, 2]))
            });
            (a.join(), b.join())
        });
        saved.map_err(|_| "save thread panicked")??;
        match finalized.map_err(|_| "finalize thread panicked")? {
            Ok(total) => check(
                total == expected_length,
                "finalization lost concurrent part",
            )?,
            Err(Error::PartNotFound {
                blob_id,
                part_id: 2,
            }) if blob_id == blob => {}
            _ => return Err("unexpected concurrent finalization result".into()),
        }
        check(
            block_on(registry.finalize(blob, vec![1, 2]))? == expected_length,
            "finalize retry changed length",
        )?;
        check(
            matches!(block_on(registry.save_part(blob, 44, 1, 3)),
            Err(Error::BlobFinalized(id)) if id == blob),
            "save succeeded after finalization",
        )?;
    }
    Ok(())
}

#[test]
fn states_parts_and_unique_ids_survive_database_reopening() -> TestResult {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "wd40-blob-registry-{}-{timestamp}",
        std::process::id(),
    ));
    let result: TestResult = block_on(async {
        let (open, finalized): (BlobId, BlobId) = {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            let open = registry.create().await?;
            let finalized = registry.create().await?;
            registry.save_part(open, 41, 3, 1).await?;
            registry.save_part(finalized, 42, 5, 1).await?;
            registry.finalize(finalized, vec![1, 1]).await?;
            (open, finalized)
        };
        let registry = LocalRegistry::new(sled::open(&path)?)?;
        check(
            matches!(registry.read(open, 0..0).await,
            Err(Error::BlobNotFinalized(id)) if id == open),
            "open state did not persist",
        )?;
        registry.save_part(open, 43, 2, 2).await?;
        check(
            registry.finalize(open, vec![2, 1]).await? == 5,
            "open parts did not persist",
        )?;
        check(
            registry.finalize(finalized, vec![1, 1]).await? == 10,
            "finalized manifest did not persist",
        )?;
        check(
            registry
                .read(finalized, 4..7)
                .await?
                .try_collect::<Vec<_>>()
                .await?
                == vec![
                    StreamSlice {
                        stream_id: 42,
                        skip: 4,
                        length: 1,
                    },
                    StreamSlice {
                        stream_id: 42,
                        skip: 0,
                        length: 2,
                    },
                ],
            "persisted range changed",
        )?;
        let next = registry.create().await?;
        check(
            next != open && next != finalized,
            "reopening reused a blob ID",
        )
    });
    let cleanup = std::fs::remove_dir_all(&path);
    result?;
    cleanup?;
    Ok(())
}
