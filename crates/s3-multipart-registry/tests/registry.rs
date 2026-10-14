use std::{
    collections::BTreeMap,
    sync::{Arc, Barrier},
    time::{SystemTime, UNIX_EPOCH},
};

use futures_executor::block_on;
use s3_multipart_registry::{
    Error, LocalRegistry, MultipartUpload, MultipartUploadId, S3MultipartRegistry,
    traits::S3MultipartRegistry as _,
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn database() -> Result<sled::Db, sled::Error> {
    sled::Config::new()
        .temporary(true)
        .flush_every_ms(None)
        .open()
}

fn upload(blob_id: u128) -> MultipartUpload {
    MultipartUpload {
        blob_id,
        bucket: "bucket".to_owned(),
        key: "directory/密.txt".to_owned(),
        metadata: BTreeMap::from([
            ("content-type".to_owned(), "text/plain".to_owned()),
            ("custom".to_owned(), "valeur 密".to_owned()),
        ]),
    }
}

#[test]
fn ids_and_metadata_round_trip() -> TestResult {
    let value = "0123456789abcdef0123456789abcdef".to_owned();
    let id = MultipartUploadId::new(value.clone())?;
    check(
        MultipartUploadId::try_from(value.clone())? == id,
        "TryFrom changed ID",
    )?;
    check(
        value.parse::<MultipartUploadId>()? == id,
        "FromStr changed ID",
    )?;
    let decoded: MultipartUploadId = postcard::from_bytes(&postcard::to_allocvec(&id)?)?;
    check(decoded == id, "ID serialization changed value")?;
    for invalid in [
        String::new(),
        "a".repeat(31),
        "a".repeat(33),
        "A".repeat(32),
        "g".repeat(32),
        "密".repeat(32),
    ] {
        check(
            matches!(
                MultipartUploadId::new(invalid.clone()),
                Err(Error::InvalidUploadId)
            ),
            "invalid ID accepted",
        )?;
        check(
            postcard::from_bytes::<MultipartUploadId>(&postcard::to_allocvec(&invalid)?).is_err(),
            "deserialization accepted invalid ID",
        )?;
    }
    let original = upload(u128::MAX);
    let decoded: MultipartUpload = postcard::from_bytes(&postcard::to_allocvec(&original)?)?;
    check(decoded == original, "metadata serialization changed upload")
}

#[test]
fn create_get_remove_and_blob_reuse() -> TestResult {
    block_on(async {
        let registry = S3MultipartRegistry::Local(LocalRegistry::new(database()?)?);
        let absent = MultipartUploadId::new("0".repeat(32))?;
        check(
            registry.get(&absent).await?.is_none(),
            "absent upload returned",
        )?;
        registry.remove(&absent).await?;
        let original = upload(u128::MAX);
        let id = registry.create(original.clone()).await?;
        check(
            registry.get(&id).await? == Some(original.clone()),
            "upload changed",
        )?;
        for duplicate in [
            original.clone(),
            MultipartUpload {
                key: "other".to_owned(),
                ..original.clone()
            },
        ] {
            check(
                matches!(registry.create(duplicate).await, Err(Error::BlobAlreadyExists(value)) if value == u128::MAX),
                "duplicate blob accepted",
            )?;
        }
        check(
            registry.get(&id).await? == Some(original.clone()),
            "duplicate changed upload",
        )?;
        registry.remove(&id).await?;
        registry.remove(&id).await?;
        check(
            registry.get(&id).await?.is_none(),
            "removed upload returned",
        )?;
        let next = registry.create(original.clone()).await?;
        check(
            registry.get(&next).await? == Some(original),
            "blob reuse failed",
        )
    })
}

#[test]
fn concurrent_creates_preserve_bijection() -> TestResult {
    let database = database()?;
    let barrier = Arc::new(Barrier::new(2));
    let mut workers = Vec::new();
    for _ in 0..2 {
        let registry = LocalRegistry::new(database.clone())?;
        let barrier = barrier.clone();
        workers.push(std::thread::Builder::new().spawn(move || {
            barrier.wait();
            block_on(registry.create(upload(42)))
        })?);
    }
    let mut winner = None;
    let mut duplicate = false;
    for worker in workers {
        match worker.join().map_err(|_| "creation worker panicked")? {
            Ok(id) => {
                check(winner.is_none(), "two creates succeeded for the same blob")?;
                winner = Some(id);
            }
            Err(Error::BlobAlreadyExists(42)) => {
                check(!duplicate, "both creates failed as duplicates")?;
                duplicate = true;
            }
            Err(error) => return Err(error.into()),
        }
    }
    check(duplicate, "missing typed duplicate")?;
    let id = winner.ok_or("no successful create")?;
    check(
        database.open_tree("s3-multipart-uploads")?.len() == 1,
        "extra upload persisted",
    )?;
    let blobs = database.open_tree("s3-multipart-blobs")?;
    check(blobs.len() == 1, "extra reverse mapping persisted")?;
    check(
        blobs.get(42_u128.to_be_bytes())?.as_deref() == Some(id.as_str().as_bytes()),
        "reverse mapping changed",
    )
}

#[test]
fn reopening_preserves_creation_and_removal() -> TestResult {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!("wd40-multipart-{}-{unique}", std::process::id()));
    let result = block_on(async {
        let original = upload(7);
        let id = {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            registry.create(original.clone()).await?
        };
        {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            check(
                registry.get(&id).await? == Some(original.clone()),
                "reopen lost upload",
            )?;
            registry.remove(&id).await?;
        }
        let registry = LocalRegistry::new(sled::open(&path)?)?;
        check(
            registry.get(&id).await?.is_none(),
            "reopen restored removed upload",
        )?;
        registry.create(original).await?;
        Ok(())
    });
    std::fs::remove_dir_all(path)?;
    result
}

#[test]
fn corrupted_metadata_and_reverse_mapping_are_reported() -> TestResult {
    block_on(async {
        let database = database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let id = registry.create(upload(8)).await?;
        let uploads = database.open_tree("s3-multipart-uploads")?;
        let original = uploads
            .get(id.as_str().as_bytes())?
            .ok_or("missing upload")?;
        let mut corrupt = original.to_vec();
        corrupt.push(0);
        uploads.insert(id.as_str().as_bytes(), corrupt)?;
        check(
            matches!(registry.get(&id).await, Err(Error::CorruptState(_))),
            "trailing bytes accepted",
        )?;
        uploads.insert(id.as_str().as_bytes(), original)?;
        database
            .open_tree("s3-multipart-blobs")?
            .remove(8_u128.to_be_bytes())?;
        check(
            matches!(registry.remove(&id).await, Err(Error::CorruptState(_))),
            "missing reverse mapping ignored",
        )?;
        check(
            registry.get(&id).await? == Some(upload(8)),
            "failed removal changed session",
        )
    })
}
