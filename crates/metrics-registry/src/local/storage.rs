//! Column-family access, snapshot reads and atomic batch preparation.

use rocksdb::{
    ColumnFamilyRef, DB, Direction, IteratorMode, ReadOptions, Snapshot, WriteBatch, WriteOptions,
};

use crate::Error;

use super::codec::{Key, Store, Value, prefix_end};
use super::merge::Operand;

#[derive(Debug)]
pub(super) struct Storage {
    database: DB,
}

pub(super) struct View<'a> {
    storage: &'a Storage,
    snapshot: Snapshot<'a>,
}

impl Storage {
    pub(super) fn new(database: DB) -> Result<Self, Error> {
        let storage = Self { database };
        for store in Store::ALL {
            storage.family(store)?;
        }
        Ok(storage)
    }

    pub(super) fn snapshot(&self) -> View<'_> {
        View {
            storage: self,
            snapshot: self.database.snapshot(),
        }
    }

    pub(super) fn put(
        &self,
        batch: &mut WriteBatch,
        key: Key<'_>,
        value: Value,
    ) -> Result<(), Error> {
        let family = self.family(key.store())?;
        let bytes = key.encode()?;
        let value = key.encode_value(value)?;
        batch.put_cf(&family, bytes, value);
        Ok(())
    }

    pub(super) fn merge(
        &self,
        batch: &mut WriteBatch,
        key: Key<'_>,
        operand: Operand,
    ) -> Result<(), Error> {
        let family = self.family(key.store())?;
        batch.merge_cf(&family, key.encode()?, operand.encode());
        Ok(())
    }

    /// Range bounds must stay within the selected logical store. Reference rows
    /// use a different tag from history windows and survive window range deletion.
    pub(super) fn delete_range(
        &self,
        batch: &mut WriteBatch,
        store: Store,
        start: &[u8],
        end: &[u8],
    ) -> Result<(), Error> {
        if start >= end {
            return Err(Error::CorruptData("invalid storage deletion range"));
        }
        let family = self.family(store)?;
        batch.delete_range_cf(&family, start, end);
        Ok(())
    }

    /// Commits all prepared column-family changes together and syncs the WAL.
    pub(super) fn write(&self, batch: WriteBatch) -> Result<(), Error> {
        let mut options = WriteOptions::default();
        options.set_sync(true);
        self.database.write_opt(batch, &options).map_err(backend)
    }

    fn family(&self, store: Store) -> Result<ColumnFamilyRef<'_>, Error> {
        self.database
            .cf_handle(store.name())
            .ok_or(Error::MissingColumnFamily(store.name()))
    }
}

impl View<'_> {
    pub(super) fn get(&self, key: Key<'_>) -> Result<Option<Value>, Error> {
        let family = self.storage.family(key.store())?;
        self.snapshot
            .get_cf(&family, key.encode()?)
            .map_err(backend)?
            .map(|bytes| key.decode_value(&bytes))
            .transpose()
    }

    /// Streams one bounded prefix range from this view without collecting rows.
    /// Total-order seeks also work with caller-configured prefix extractors.
    pub(super) fn scan_prefix(
        &self,
        store: Store,
        prefix: &[u8],
        mut visit: impl FnMut(Key<'_>, Value) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let family = self.storage.family(store)?;
        let mut options = ReadOptions::default();
        options.set_total_order_seek(true);
        options.set_iterate_lower_bound(prefix.to_vec());
        if let Some(end) = prefix_end(prefix) {
            options.set_iterate_upper_bound(end);
        }
        let mode = IteratorMode::From(prefix, Direction::Forward);
        for row in self.snapshot.iterator_cf_opt(&family, options, mode) {
            let (key, value) = row.map_err(backend)?;
            if !key.starts_with(prefix) {
                break;
            }
            let key = Key::decode(store, &key)?;
            visit(key, key.decode_value(&value)?)?;
        }
        Ok(())
    }
}

fn backend(error: rocksdb::Error) -> Error {
    Error::Backend(Box::new(error))
}
