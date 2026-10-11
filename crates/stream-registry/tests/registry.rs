use std::{
    error::Error as StdError,
    time::{SystemTime, UNIX_EPOCH},
};

use craft_codec::{Compression, Config, Encryption, FrameSizes, Framing, Metadata, MetadataParts};
use futures_executor::block_on;
use futures_util::{StreamExt, TryStreamExt};
use stream_registry::{
    Error, LocalRegistry, StreamId, StreamManifest, StreamPart, StreamRegistry,
    traits::StreamRegistry as _,
};

type TestResult<T = ()> = Result<T, Box<dyn StdError>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn metadata(length: u64) -> TestResult<Metadata> {
    let config = Config::new(Framing::Fixed(16), Compression::None, Encryption::None)?;
    let mut metadata = Metadata::new(config);
    metadata.push(FrameSizes::new(length, length)?)?;
    Ok(metadata)
}

fn temporary_database() -> TestResult<sled::Db> {
    Ok(sled::Config::new().temporary(true).open()?)
}

fn require_send<T: Send>(_: &T) {}

fn stored_manifest(database: &sled::Db, id: StreamId) -> TestResult<StreamManifest> {
    let data = database
        .open_tree("stream-manifests")?
        .get(id.to_be_bytes())?
        .ok_or("stored manifest was not found")?;
    Ok(postcard::from_bytes(&data)?)
}

#[test]
fn enum_preserves_manifest_order_repetitions_and_empty_streams() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = StreamRegistry::Local(LocalRegistry::new(database.clone())?);
        let insert = registry.insert(41, metadata(3)?);
        require_send(&insert);
        let first = insert.await?;
        let second = registry.insert(42, metadata(5)?).await?;
        let store = registry.store(vec![first, second, first]);
        require_send(&store);
        let a = store.await?;
        let b = registry.store(vec![second, first]).await?;
        let empty = registry.store(Vec::new()).await?;

        check(
            stored_manifest(&database, a)?
                == StreamManifest {
                    parts: vec![first, second, first],
                    length: 11,
                },
            "manifest order or repeated part length changed",
        )?;
        check(
            stored_manifest(&database, empty)?
                == StreamManifest {
                    parts: Vec::new(),
                    length: 0,
                },
            "empty stream did not retain an empty manifest",
        )?;

        let expected_first = StreamPart {
            file_id: 41,
            metadata: metadata(3)?,
        };
        let expected_second = StreamPart {
            file_id: 42,
            metadata: metadata(5)?,
        };
        let requests = vec![a, empty, b, a];
        let retrieve = registry.retrieve_parts(&requests);
        require_send(&retrieve);
        let stream = retrieve.await?;
        require_send(&stream);
        drop(requests);
        let parts = stream.try_collect::<Vec<_>>().await?;
        check(
            parts
                == vec![
                    expected_first.clone(),
                    expected_second.clone(),
                    expected_first.clone(),
                    expected_second.clone(),
                    expected_first.clone(),
                    expected_first.clone(),
                    expected_second,
                    expected_first,
                ],
            "part retrieval changed file IDs, metadata, order or repetitions",
        )?;
        check(
            registry.retrieve_parts(&[]).await?.next().await.is_none(),
            "empty requests produced parts",
        )
    })
}

#[test]
fn missing_part_and_length_overflow_do_not_publish_manifests() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let small = registry.insert(41, metadata(3)?).await?;
        check(
            matches!(
                registry.store(vec![small, u128::MAX]).await,
                Err(Error::StreamPartNotFound(u128::MAX))
            ),
            "store accepted a missing part",
        )?;

        let large = Metadata::from_parts(MetadataParts {
            schema_version: craft_codec::METADATA_SCHEMA_VERSION,
            version: craft_codec::FORMAT_VERSION,
            config: Config::new(Framing::Fixed(1), Compression::None, Encryption::None)?,
            frame_count: u64::MAX,
            last_frame_len: Some(1),
            raw_lengths: None,
            payload_lengths: None,
        })?;
        let large = registry.insert(42, large).await?;
        check(
            matches!(
                registry.store(vec![large, small]).await,
                Err(Error::LengthOverflow)
            ),
            "store accepted an overflowing logical length",
        )?;
        check(
            database
                .open_tree("stream-manifests")?
                .iter()
                .next()
                .is_none(),
            "failed store published a partial manifest",
        )?;
        let id = registry.store(vec![small]).await?;
        check(
            stored_manifest(&database, id)?.length == 3,
            "failed store damaged an existing part",
        )
    })
}

#[test]
fn missing_reads_are_lazy_and_terminate_after_the_error() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let part = registry.insert(41, metadata(3)?).await?;
        let id = registry.store(vec![part]).await?;
        let mut parts = registry.retrieve_parts(&[id, u128::MAX, id]).await?;
        check(
            matches!(parts.next().await, Some(Ok(value)) if value.file_id == 41),
            "retrieve_parts failed before yielding the preceding stream's part",
        )?;
        check(
            matches!(
                parts.next().await,
                Some(Err(Error::StreamNotFound(u128::MAX)))
            ),
            "retrieve_parts did not identify the missing stream",
        )?;
        check(
            parts.next().await.is_none(),
            "retrieve_parts continued after a missing stream",
        )?;

        let missing = registry.insert(42, metadata(5)?).await?;
        let broken = registry.store(vec![part, missing, part]).await?;
        database
            .open_tree("stream-parts")?
            .remove(missing.to_be_bytes())?;
        let mut parts = registry.retrieve_parts(&[broken, id]).await?;
        check(
            matches!(parts.next().await, Some(Ok(value)) if value.file_id == 41),
            "retrieve_parts failed before yielding the preceding part",
        )?;
        check(
            matches!(parts.next().await, Some(Err(Error::StreamPartNotFound(value))) if value == missing),
            "retrieve_parts did not identify the missing part",
        )?;
        check(
            parts.next().await.is_none(),
            "retrieve_parts continued after an error",
        )
    })
}

#[test]
fn corrupted_parts_abort_store_and_preserve_the_serialization_cause() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let id = registry.insert(41, metadata(3)?).await?;
        let stream = registry.store(vec![id]).await?;
        database
            .open_tree("stream-parts")?
            .insert(id.to_be_bytes(), vec![0xff])?;

        check(
            matches!(registry.store(vec![id]).await, Err(Error::Serialization(_))),
            "store accepted corrupt part metadata",
        )?;
        check(
            database.open_tree("stream-manifests")?.iter().count() == 1,
            "failed store changed the number of manifests",
        )?;
        let mut parts = registry.retrieve_parts(&[stream, stream]).await?;
        match parts.next().await {
            Some(Err(error @ Error::Serialization(_))) => {
                check(
                    error.source().is_some(),
                    "serialization error lost its cause",
                )?;
            }
            _ => return Err("corrupt metadata did not produce a serialization error".into()),
        }
        check(
            parts.next().await.is_none(),
            "corrupt part read continued after its error",
        )
    })
}

#[test]
fn manifests_and_metadata_survive_database_reopening() -> TestResult {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "wd40-stream-registry-{}-{timestamp}",
        std::process::id(),
    ));
    let result: TestResult = block_on(async {
        let (part, stream) = {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            let part = registry.insert(41, metadata(3)?).await?;
            let stream = registry.store(vec![part, part]).await?;
            (part, stream)
        };

        let database = sled::open(&path)?;
        let registry = LocalRegistry::new(database.clone())?;
        check(
            stored_manifest(&database, stream)?
                == StreamManifest {
                    parts: vec![part, part],
                    length: 6,
                },
            "manifest did not survive database reopening",
        )?;
        check(
            registry
                .retrieve_parts(&[stream])
                .await?
                .try_collect::<Vec<_>>()
                .await?
                == vec![
                    StreamPart {
                        file_id: 41,
                        metadata: metadata(3)?,
                    },
                    StreamPart {
                        file_id: 41,
                        metadata: metadata(3)?,
                    },
                ],
            "CRAFT metadata did not survive database reopening",
        )?;
        let next = registry.insert(42, metadata(5)?).await?;
        check(next != part, "reopening reused an existing part identifier")
    });
    let cleanup = std::fs::remove_dir_all(&path);
    result?;
    cleanup?;
    Ok(())
}
