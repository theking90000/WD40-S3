use std::{
    collections::BTreeSet,
    error::Error as StdError,
    sync::{Arc, Barrier, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_executor::block_on;
use object_registry::{
    BlobId, Cursor, Error, LocalRegistry, Metadata, ObjectRegistry, RecordMetadata, VersionId,
    traits::ObjectRegistry as _,
};

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

fn version_key(key: &str, id: VersionId) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(key.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&(!id).to_be_bytes());
    bytes
}

fn head_key(key: &str) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend_from_slice(key.as_bytes());
    bytes
}

async fn all_pages(
    registry: &LocalRegistry,
    prefix: &str,
    include_versions: bool,
    limit: usize,
) -> TestResult<Vec<RecordMetadata>> {
    let mut records = Vec::new();
    let mut cursor = None;
    loop {
        let page = registry
            .list(prefix, cursor.as_ref(), Some(limit), include_versions)
            .await?;
        check(page.records.len() <= limit, "page exceeds its limit")?;
        records.extend(page.records);
        match page.next_cursor {
            Some(next) => {
                check(cursor.as_ref() != Some(&next), "pagination did not advance")?;
                let bytes = postcard::to_allocvec(&next)?;
                cursor = Some(postcard::from_bytes::<Cursor>(&bytes)?);
            }
            None => return Ok(records),
        }
    }
}

#[test]
fn enum_returns_owned_complete_records_and_opaque_metadata() -> TestResult {
    block_on(async {
        let registry = ObjectRegistry::Local(LocalRegistry::new(temporary_database()?)?);
        let metadata = Metadata::from([
            ("size".to_owned(), "not a number".to_owned()),
            ("etag".to_owned(), "caller-defined".to_owned()),
            ("任意\0field".to_owned(), "value\0with NUL".to_owned()),
        ]);
        let store = registry.store("documents/é.txt", BlobId::MAX, metadata.clone());
        require_send(&store);
        let first = store.await?;
        let second = registry
            .store("documents/é.txt", 7, Metadata::new())
            .await?;
        check(second > first, "versions are not monotonic")?;
        let head = registry.head("documents/é.txt", Some(first));
        require_send(&head);
        let original = head.await?;
        check(
            original
                == RecordMetadata {
                    key: "documents/é.txt".to_owned(),
                    version_id: first,
                    blob_id: Some(BlobId::MAX),
                    metadata,
                    is_delete_marker: false,
                },
            "head changed opaque metadata or checked the blob",
        )?;
        let current = registry.head("documents/é.txt", None).await?;
        check(
            current.version_id == second && current.metadata.is_empty(),
            "current head is stale",
        )?;
        let list = registry.list("documents/", None, None, false);
        require_send(&list);
        check(
            list.await?.records == vec![current],
            "list does not return the same record as head",
        )?;
        check(
            matches!(
                registry.head("other", Some(first)).await,
                Err(Error::VersionNotFound { .. })
            ),
            "version lookup crossed object keys",
        )?;
        let delete = registry.delete("documents/é.txt", None);
        require_send(&delete);
        let marker = delete.await?.version_id;
        check(
            marker > second,
            "delete marker did not get a new version ID",
        )?;
        drop(registry);
        check(
            original.blob_id == Some(BlobId::MAX),
            "head borrowed the registry",
        )
    })
}

#[test]
fn markers_and_deleted_versions_restore_the_correct_predecessor() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let first = registry.store("key", 11, Metadata::new()).await?;
        let second = registry.store("key", 22, Metadata::new()).await?;
        let marker = registry.delete("key", None).await?.version_id;
        check(
            matches!(
                registry.head("key", None).await,
                Err(Error::ObjectNotFound(_))
            ),
            "marker did not hide the current object",
        )?;
        check(
            registry
                .list("", None, None, false)
                .await?
                .records
                .is_empty(),
            "active listing included a deleted object",
        )?;
        let marker_record = registry.head("key", Some(marker)).await?;
        check(
            marker_record.is_delete_marker
                && marker_record.blob_id.is_none()
                && marker_record.metadata.is_empty(),
            "invalid delete marker record",
        )?;
        check(
            registry.head("key", Some(first)).await?.blob_id == Some(11),
            "marker hid an explicit older version",
        )?;

        registry.delete("key", Some(second)).await?;
        check(
            matches!(
                registry.head("key", None).await,
                Err(Error::ObjectNotFound(_))
            ),
            "deleting an older version resurrected an object behind a marker",
        )?;
        check(
            matches!(
                registry.head("key", Some(second)).await,
                Err(Error::VersionNotFound { .. })
            ),
            "deleted version is still readable",
        )?;
        let visible: Vec<_> = registry
            .list("key", None, None, true)
            .await?
            .records
            .into_iter()
            .map(|record| record.version_id)
            .collect();
        check(
            visible == vec![marker, first],
            "history included a flagged version",
        )?;

        registry.delete("key", Some(marker)).await?;
        check(
            registry.head("key", None).await?.version_id == first,
            "deleting the marker did not skip a deleted predecessor",
        )?;
        let remaining_marker = registry.delete("key", None).await?.version_id;
        let object = registry.store("key", 33, Metadata::new()).await?;
        registry.delete("key", Some(object)).await?;
        check(
            matches!(
                registry.head("key", None).await,
                Err(Error::ObjectNotFound(_))
            ),
            "deleting the current object skipped a surviving marker",
        )?;
        let newest_marker = registry.delete("key", None).await?.version_id;
        registry.delete("key", Some(newest_marker)).await?;
        check(
            matches!(
                registry.head("key", None).await,
                Err(Error::ObjectNotFound(_))
            ),
            "deleting one of two markers resurrected the object",
        )?;
        registry.delete("key", Some(remaining_marker)).await?;
        check(
            registry.head("key", None).await?.version_id == first,
            "original was not restored",
        )?;
        registry.delete("key", Some(first)).await?;
        check(
            registry
                .list("", None, None, true)
                .await?
                .records
                .is_empty(),
            "fully deleted history is visible",
        )?;
        check(
            matches!(
                registry.head("key", None).await,
                Err(Error::ObjectNotFound(_))
            ),
            "deleting the last version left an active entry",
        )?;
        let versions = database.open_tree("object-versions")?;
        check(
            versions.get(version_key("key", second))?.is_some(),
            "deleted record was removed",
        )?;
        check(
            versions.get(head_key("key"))?.is_none(),
            "empty history retained a current pointer",
        )?;
        let fresh = registry.store("key", 44, Metadata::new()).await?;
        check(
            fresh > newest_marker,
            "fully deleted key reused a version ID",
        )?;
        check(
            registry.head("key", None).await?.blob_id == Some(44),
            "key could not be stored again",
        )
    })
}

#[test]
fn explicit_deletion_is_idempotent_and_leaves_blob_trees_unchanged() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let blob_tree = database.open_tree("blob-manifests")?;
        blob_tree.insert(
            77_u128.to_be_bytes().as_slice(),
            b"opaque blob record".as_slice(),
        )?;
        let id = registry.store("key", 77, Metadata::new()).await?;
        registry.delete("key", Some(id)).await?;
        let versions = database.open_tree("object-versions")?;
        let bytes = versions
            .get(version_key("key", id))?
            .ok_or("deleted version was removed")?;
        let count = versions.len();
        check(
            registry.delete("key", Some(id)).await?.version_id == id,
            "repeat changed the result",
        )?;
        check(
            registry.delete("key", Some(u64::MAX)).await?.version_id == u64::MAX,
            "missing version deletion failed",
        )?;
        registry.delete("missing", Some(id)).await?;
        check(
            versions.len() == count,
            "idempotent deletion created a version",
        )?;
        check(
            versions.get(version_key("key", id))? == Some(bytes),
            "repeat rewrote the record",
        )?;
        check(
            blob_tree.get(77_u128.to_be_bytes())?.as_deref()
                == Some(b"opaque blob record".as_slice()),
            "object deletion changed blob data",
        )?;
        let first_marker = registry.delete("missing", None).await?.version_id;
        let second_marker = registry.delete("missing", None).await?.version_id;
        check(
            second_marker > first_marker,
            "repeated simple deletion reused a marker",
        )?;
        check(
            registry
                .list("missing", None, None, true)
                .await?
                .records
                .len()
                == 2,
            "deleting an absent key did not retain both markers",
        )
    })
}

#[test]
fn pages_preserve_key_order_version_order_and_complete_metadata() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(temporary_database()?)?;
        for key in ["pré", "pré/a", "pré/b", "préfixe", "other"] {
            let metadata = Metadata::from([("key".to_owned(), key.to_owned())]);
            registry.store(key, 10, metadata.clone()).await?;
            registry.store(key, 20, metadata).await?;
        }
        let older = registry.store("pré/a", 30, Metadata::new()).await?;
        registry.delete("pré/a", Some(older)).await?;
        let marker = registry.delete("pré/b", None).await?.version_id;
        registry.delete("pré/c", None).await?;
        for include_versions in [false, true] {
            let expected = registry.list("pré", None, None, include_versions).await?;
            check(
                expected.next_cursor.is_none(),
                "small listing unexpectedly needs a cursor",
            )?;
            for limit in [1, 2, 3] {
                let actual = all_pages(&registry, "pré", include_versions, limit).await?;
                check(
                    actual == expected.records,
                    "pagination omitted or repeated records",
                )?;
                for record in actual {
                    check(
                        registry.head(&record.key, Some(record.version_id)).await? == record,
                        "listing metadata differs from head",
                    )?;
                }
            }
        }
        let current = registry.list("pré", None, None, false).await?.records;
        let keys: Vec<_> = current.iter().map(|record| record.key.as_str()).collect();
        check(
            keys == vec!["pré", "pré/a", "préfixe"],
            "current listing has wrong prefix or ordering",
        )?;
        let history = registry.list("pré", None, None, true).await?.records;
        check(
            history.windows(2).all(|pair| match pair {
                [left, right] => {
                    left.key < right.key
                        || (left.key == right.key && left.version_id > right.version_id)
                }
                _ => false,
            }),
            "history is not sorted by key and descending version",
        )?;
        check(
            history
                .iter()
                .any(|record| record.version_id == marker && record.is_delete_marker),
            "history omitted a marker",
        )?;
        check(
            !history.iter().any(|record| record.version_id == older),
            "history exposed a flagged version",
        )?;
        check(
            registry
                .list("absent", None, None, true)
                .await?
                .records
                .is_empty(),
            "empty prefix matched",
        )?;

        let first = registry.list("pré", None, Some(1), true).await?;
        let cursor = first.next_cursor.ok_or("expected a cursor")?;
        let returned = first.records.first().ok_or("first page is empty")?;
        registry
            .delete(&returned.key, Some(returned.version_id))
            .await?;
        check(
            !registry
                .list("pré", Some(&cursor), Some(1), true)
                .await?
                .records
                .is_empty(),
            "cursor required its last returned version to remain visible",
        )?;
        let beyond = registry
            .list("pré", None, Some(1), false)
            .await?
            .next_cursor
            .ok_or("expected an active cursor")?;
        check(
            !registry
                .list("pré", Some(&beyond), Some(1), false)
                .await?
                .records
                .is_empty(),
            "active cursor did not resume",
        )
    })
}

#[test]
fn invalid_inputs_and_incompatible_or_malformed_cursors_are_rejected() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        for key in ["", "a\0b"] {
            check(
                matches!(
                    registry.store(key, 1, Metadata::new()).await,
                    Err(Error::InvalidKey)
                ),
                "store accepted an invalid key",
            )?;
            check(
                matches!(registry.head(key, None).await, Err(Error::InvalidKey)),
                "head accepted an invalid key",
            )?;
            check(
                matches!(registry.delete(key, None).await, Err(Error::InvalidKey)),
                "delete accepted an invalid key",
            )?;
        }
        check(
            database.open_tree("object-versions")?.is_empty(),
            "invalid input changed versions",
        )?;
        check(
            matches!(
                registry.list("a\0", None, None, false).await,
                Err(Error::InvalidPrefix)
            ),
            "list accepted NUL in a prefix",
        )?;
        check(
            matches!(
                registry.list("", None, Some(0), false).await,
                Err(Error::InvalidLimit)
            ),
            "list accepted zero limit",
        )?;
        for key in ["a", "ab", "b"] {
            registry.store(key, 1, Metadata::new()).await?;
        }
        let cursor = registry
            .list("a", None, Some(1), false)
            .await?
            .next_cursor
            .ok_or("expected an active cursor")?;
        check(
            matches!(
                registry.list("b", Some(&cursor), None, false).await,
                Err(Error::InvalidCursor)
            ),
            "cursor crossed prefixes",
        )?;
        check(
            matches!(
                registry.list("a", Some(&cursor), None, true).await,
                Err(Error::InvalidCursor)
            ),
            "cursor crossed listing modes",
        )?;
        for (mode, bytes) in [
            (false, vec![0xff]),
            (true, vec![1, b'a', 0, 1]),
            (false, b"b".to_vec()),
        ] {
            let data = postcard::to_allocvec(&("a", mode, bytes))?;
            let malformed: Cursor = postcard::from_bytes(&data)?;
            check(
                matches!(
                    registry.list("a", Some(&malformed), None, mode).await,
                    Err(Error::InvalidCursor)
                ),
                "malformed cursor was accepted",
            )?;
        }
        Ok(())
    })
}

#[test]
fn default_limit_is_one_thousand_without_an_empty_final_page() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(temporary_database()?)?;
        for index in 0..1_001 {
            registry
                .store(&format!("key/{index:04}"), 1, Metadata::new())
                .await?;
        }
        let first = registry.list("key/", None, None, false).await?;
        check(first.records.len() == 1_000, "incorrect default limit")?;
        let cursor = first.next_cursor.ok_or("missing continuation cursor")?;
        let last = registry.list("key/", Some(&cursor), None, false).await?;
        check(
            last.records.len() == 1 && last.next_cursor.is_none(),
            "incorrect final page",
        )?;
        let exact = registry.list("key/", None, Some(1_001), false).await?;
        check(
            exact.records.len() == 1_001 && exact.next_cursor.is_none(),
            "exactly full final page advertised another page",
        )
    })
}

#[test]
fn independent_handles_serialize_same_key_stores_and_preserve_predecessors() -> TestResult {
    let database = temporary_database()?;
    let registry = LocalRegistry::new(database.clone())?;
    let barrier = Arc::new(Barrier::new(8));
    let mut handles = Vec::new();
    for blob_id in 1_u128..=8 {
        handles.push((blob_id, LocalRegistry::new(database.clone())?));
    }
    let ids: TestResult<Vec<VersionId>> = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for (blob_id, independent) in handles {
            let barrier = barrier.clone();
            workers.push(scope.spawn(move || {
                barrier.wait();
                block_on(independent.store("shared", blob_id, Metadata::new()))
            }));
        }
        let mut ids = Vec::new();
        for worker in workers {
            ids.push(worker.join().map_err(|_| "store worker panicked")??);
        }
        Ok(ids)
    });
    let ids = ids?;
    check(
        ids.iter().copied().collect::<BTreeSet<_>>().len() == 8,
        "concurrent stores reused IDs",
    )?;
    let history = block_on(registry.list("shared", None, None, true))?.records;
    let mut expected = ids;
    expected.sort_unstable_by(|left, right| right.cmp(left));
    check(
        history
            .iter()
            .map(|record| record.version_id)
            .collect::<Vec<_>>()
            == expected,
        "concurrent history lost or reordered versions",
    )?;
    for (position, record) in history.iter().enumerate() {
        check(
            block_on(registry.head("shared", None))?.version_id == record.version_id,
            "current pointer does not follow descending history",
        )?;
        block_on(registry.delete("shared", Some(record.version_id)))?;
        match history.get(position.saturating_add(1)) {
            Some(next) => check(
                block_on(registry.head("shared", None))?.version_id == next.version_id,
                "concurrent store omitted a predecessor link",
            )?,
            None => check(
                matches!(
                    block_on(registry.head("shared", None)),
                    Err(Error::ObjectNotFound(_))
                ),
                "last deleted version left the object active",
            )?,
        }
    }
    Ok(())
}

#[test]
fn racing_deletions_and_store_delete_preserve_the_current_object() -> TestResult {
    let database = temporary_database()?;
    let registry = LocalRegistry::new(database.clone())?;
    let older = block_on(registry.store("key", 1, Metadata::new()))?;
    let current = block_on(registry.store("key", 2, Metadata::new()))?;
    let left = LocalRegistry::new(database.clone())?;
    let right = LocalRegistry::new(database.clone())?;
    let barrier = Arc::new(Barrier::new(2));
    let (a, b) = std::thread::scope(|scope| {
        let left_barrier = barrier.clone();
        let a = scope.spawn(move || {
            left_barrier.wait();
            block_on(left.delete("key", Some(older)))
        });
        let b = scope.spawn(move || {
            barrier.wait();
            block_on(right.delete("key", Some(current)))
        });
        (a.join(), b.join())
    });
    a.map_err(|_| "delete worker panicked")??;
    b.map_err(|_| "delete worker panicked")??;
    check(
        matches!(
            block_on(registry.head("key", None)),
            Err(Error::ObjectNotFound(_))
        ),
        "racing deletions resurrected a deleted predecessor",
    )?;

    let old = block_on(registry.store("key", 3, Metadata::new()))?;
    let left = LocalRegistry::new(database.clone())?;
    let right = LocalRegistry::new(database)?;
    let barrier = Arc::new(Barrier::new(2));
    let (stored, deleted) = std::thread::scope(|scope| {
        let left_barrier = barrier.clone();
        let stored = scope.spawn(move || {
            left_barrier.wait();
            block_on(left.store("key", 4, Metadata::new()))
        });
        let deleted = scope.spawn(move || {
            barrier.wait();
            block_on(right.delete("key", Some(old)))
        });
        (stored.join(), deleted.join())
    });
    let fresh = stored.map_err(|_| "store worker panicked")??;
    deleted.map_err(|_| "delete worker panicked")??;
    check(
        block_on(registry.head("key", None))?.version_id == fresh,
        "deletion overwrote a concurrent store",
    )?;
    check(
        matches!(
            block_on(registry.head("key", Some(old))),
            Err(Error::VersionNotFound { .. })
        ),
        "concurrent deletion did not persist",
    )
}

#[test]
fn parallel_flushes_across_independent_databases_complete() -> TestResult {
    let mut registries = Vec::new();
    for _ in 0..16 {
        registries.push(LocalRegistry::new(temporary_database()?)?);
    }
    let barrier = Arc::new(Barrier::new(registries.len()));
    let (sender, receiver) = mpsc::channel();
    for registry in registries {
        let barrier = barrier.clone();
        let sender = sender.clone();
        std::thread::Builder::new().spawn(move || {
            barrier.wait();
            let _ = sender.send(block_on(registry.store("key", 1, Metadata::new())));
        })?;
    }
    drop(sender);
    for _ in 0..16 {
        receiver.recv_timeout(Duration::from_secs(10))??;
    }
    Ok(())
}

#[test]
fn versions_flags_markers_and_unique_ids_survive_reopening() -> TestResult {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path = std::env::temp_dir().join(format!(
        "wd40-object-registry-{}-{timestamp}",
        std::process::id()
    ));
    let result: TestResult = block_on(async {
        let (first, second, marker) = {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            let first = registry.store("key", 1, Metadata::new()).await?;
            let second = registry
                .store(
                    "key",
                    2,
                    Metadata::from([("custom".into(), "retained".into())]),
                )
                .await?;
            registry.delete("key", Some(first)).await?;
            let marker = registry.delete("key", None).await?.version_id;
            (first, second, marker)
        };
        let registry = LocalRegistry::new(sled::open(&path)?)?;
        check(
            matches!(
                registry.head("key", None).await,
                Err(Error::ObjectNotFound(_))
            ),
            "marker did not survive reopening",
        )?;
        check(
            matches!(
                registry.head("key", Some(first)).await,
                Err(Error::VersionNotFound { .. })
            ),
            "deleted flag did not survive reopening",
        )?;
        registry.delete("key", Some(marker)).await?;
        let restored = registry.head("key", None).await?;
        check(
            restored.version_id == second
                && restored.metadata.get("custom").map(String::as_str) == Some("retained"),
            "predecessor or metadata did not survive reopening",
        )?;
        let next = registry.store("other", 3, Metadata::new()).await?;
        check(next > marker, "reopening reused a generated ID")
    });
    let cleanup = std::fs::remove_dir_all(&path);
    result?;
    cleanup?;
    Ok(())
}

#[test]
fn corrupt_metadata_links_and_pointers_abort_mutations_without_partial_changes() -> TestResult {
    block_on(async {
        let database = temporary_database()?;
        let registry = LocalRegistry::new(database.clone())?;
        let previous = registry.store("key", 1, Metadata::new()).await?;
        let current = registry.store("key", 2, Metadata::new()).await?;
        let active = database.open_tree("object-active")?;
        let versions = database.open_tree("object-versions")?;
        let active_before = active.get("key")?.ok_or("active record missing")?;
        let head_before = versions
            .get(head_key("key"))?
            .ok_or("head pointer missing")?;
        let current_before = versions
            .get(version_key("key", current))?
            .ok_or("current version missing")?;
        let previous_before = versions
            .insert(version_key("key", previous), vec![0xff])?
            .ok_or("previous version missing")?;
        check(
            matches!(
                registry.delete("key", Some(current)).await,
                Err(Error::Serialization(_))
            ),
            "delete accepted corrupt predecessor metadata",
        )?;
        check(
            active.get("key")? == Some(active_before.clone()),
            "failed deletion changed active",
        )?;
        check(
            versions.get(head_key("key"))? == Some(head_before.clone()),
            "failed deletion advanced head",
        )?;
        check(
            versions.get(version_key("key", current))? == Some(current_before.clone()),
            "failed deletion flagged the current version",
        )?;
        versions.remove(version_key("key", previous))?;
        check(
            matches!(
                registry.delete("key", Some(current)).await,
                Err(Error::CorruptState(_))
            ),
            "delete accepted a missing predecessor",
        )?;
        versions.insert(version_key("key", previous), previous_before)?;

        let (_, rest) = postcard::take_from_bytes::<Option<VersionId>>(&current_before)?;
        let mut cycle = postcard::to_allocvec(&Some(current))?;
        cycle.extend_from_slice(rest);
        versions.insert(version_key("key", current), cycle.clone())?;
        check(
            matches!(
                registry.head("key", Some(current)).await,
                Err(Error::CorruptState(_))
            ),
            "head accepted a cyclic predecessor link",
        )?;
        check(
            matches!(
                registry.delete("key", Some(current)).await,
                Err(Error::CorruptState(_))
            ),
            "delete accepted a cyclic predecessor link",
        )?;
        check(
            matches!(
                registry.store("key", 3, Metadata::new()).await,
                Err(Error::CorruptState(_))
            ),
            "store appended to a corrupt current version",
        )?;
        check(
            active.get("key")? == Some(active_before.clone()),
            "corrupt-link operation changed active",
        )?;
        check(
            versions.get(version_key("key", current))?.as_deref() == Some(cycle.as_slice()),
            "corrupt-link operation rewrote a version",
        )?;
        versions.insert(version_key("key", current), current_before)?;

        versions.insert(head_key("key"), vec![0xff])?;
        let count = versions.len();
        check(
            matches!(
                registry.store("key", 4, Metadata::new()).await,
                Err(Error::Serialization(_))
            ),
            "store accepted corrupt current pointer",
        )?;
        check(
            versions.len() == count && active.get("key")? == Some(active_before.clone()),
            "failed store partially changed the registry",
        )?;
        versions.insert(head_key("key"), head_before)?;
        active.insert("key", vec![0xff])?;
        let error = registry
            .head("key", None)
            .await
            .err()
            .ok_or("head accepted corrupt active metadata")?;
        check(
            matches!(&error, Error::Serialization(_)) && error.source().is_some(),
            "active decoding lost its typed error or source",
        )?;
        check(
            matches!(
                registry.list("", None, None, false).await,
                Err(Error::Serialization(_))
            ),
            "list accepted corrupt active metadata",
        )?;
        active.insert("key", active_before)?;
        versions.insert(vec![1, b'x'], vec![0])?;
        check(
            matches!(
                registry.list("", None, None, true).await,
                Err(Error::CorruptState(_))
            ),
            "history accepted a malformed version key",
        )?;
        versions.remove(vec![1, b'x'])?;
        registry.delete("key", Some(current)).await?;
        check(
            registry.head("key", None).await?.version_id == previous,
            "recovering metadata did not restore normal deletion",
        )
    })
}
