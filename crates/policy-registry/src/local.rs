use std::collections::BTreeSet;

use serde::{Serialize, de::DeserializeOwned};
use sled::{
    Db, Tree,
    transaction::{ConflictableTransactionResult, Transactional, TransactionalTree},
};

use crate::{
    Error, Policy, PolicyId, PolicySnapshot, PrincipalSelector, Revision, traits::PolicyRegistry,
};

const REVISION_KEY: &[u8] = b"revision";

/// Clones and independent handles over the same database share transactional state.
#[derive(Clone)]
pub struct LocalRegistry {
    database: Db,
    policies: Tree,
    bindings: Tree,
    state: Tree,
}

impl LocalRegistry {
    pub fn new(database: Db) -> Result<Self, Error> {
        Ok(Self {
            policies: database.open_tree("policy-records")?,
            bindings: database.open_tree("policy-bindings")?,
            state: database.open_tree("policy-state")?,
            database,
        })
    }

    async fn flush(&self) -> Result<(), Error> {
        let database = self.database.clone();
        let (sender, receiver) = futures_channel::oneshot::channel();
        // Keep flush off sled's shared I/O pool, as in the other registries.
        std::thread::Builder::new()
            .name("policy-registry-flush".to_owned())
            .spawn(move || {
                let _ = sender.send(database.flush());
            })
            .map_err(Error::FlushWorker)?;
        receiver.await??;
        Ok(())
    }

    async fn change_binding(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
        attach: bool,
    ) -> Result<Revision, Error> {
        let selector_key = selector_key(selector);
        let reverse_key = reverse_key(policy);
        let revision = (&self.policies, &self.bindings, &self.state).transaction(
            |(policies, bindings, state)| {
                if attach && policies.get(policy.to_be_bytes())?.is_none() {
                    return Err(Error::PolicyNotFound(policy).into());
                }
                let mut ids: BTreeSet<PolicyId> = read_set(bindings, &selector_key)?;
                let mut selectors: BTreeSet<PrincipalSelector> = read_set(bindings, &reverse_key)?;
                if ids.contains(&policy) != selectors.contains(&selector) {
                    return Err(Error::CorruptState("binding indexes disagree").into());
                }
                let changed = if attach {
                    selectors.insert(selector);
                    ids.insert(policy)
                } else {
                    selectors.remove(&selector);
                    ids.remove(&policy)
                };
                if !changed {
                    return read_revision(state);
                }
                write_set(bindings, &selector_key, &ids)?;
                write_set(bindings, &reverse_key, &selectors)?;
                bump_revision(state)
            },
        )?;
        self.flush().await?;
        Ok(revision)
    }
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, Error> {
    Ok(postcard::to_allocvec(value)?)
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    let (value, tail) = postcard::take_from_bytes(bytes)?;
    if !tail.is_empty() {
        return Err(Error::CorruptState("trailing serialized bytes"));
    }
    Ok(value)
}

fn selector_key(selector: PrincipalSelector) -> Vec<u8> {
    match selector {
        PrincipalSelector::Everyone => vec![0, 0],
        PrincipalSelector::Authenticated => vec![0, 1],
        PrincipalSelector::Specific(id) => {
            let mut key = vec![0, 2];
            key.extend_from_slice(&id.to_be_bytes());
            key
        }
    }
}

fn reverse_key(id: PolicyId) -> Vec<u8> {
    let mut key = vec![1];
    key.extend_from_slice(&id.to_be_bytes());
    key
}

fn read_set<T: Ord + DeserializeOwned>(
    tree: &TransactionalTree,
    key: &[u8],
) -> ConflictableTransactionResult<BTreeSet<T>, Error> {
    match tree.get(key)? {
        Some(bytes) => Ok(decode(&bytes)?),
        None => Ok(BTreeSet::new()),
    }
}

fn write_set<T: Ord + Serialize>(
    tree: &TransactionalTree,
    key: &[u8],
    values: &BTreeSet<T>,
) -> ConflictableTransactionResult<(), Error> {
    if values.is_empty() {
        tree.remove(key)?;
    } else {
        tree.insert(key, encode(values)?)?;
    }
    Ok(())
}

fn read_revision(state: &TransactionalTree) -> ConflictableTransactionResult<Revision, Error> {
    match state.get(REVISION_KEY)? {
        Some(bytes) => Ok(decode(&bytes)?),
        None => Ok(Revision::default()),
    }
}

fn bump_revision(state: &TransactionalTree) -> ConflictableTransactionResult<Revision, Error> {
    let revision = Revision(
        read_revision(state)?
            .0
            .checked_add(1)
            .ok_or(Error::RevisionOverflow)?,
    );
    state.insert(REVISION_KEY, encode(&revision)?)?;
    Ok(revision)
}

impl PolicyRegistry for LocalRegistry {
    async fn get_policy(&self, id: PolicyId) -> Result<Option<Policy>, Error> {
        self.policies
            .get(id.to_be_bytes())?
            .as_deref()
            .map(decode)
            .transpose()
    }

    async fn put_policy(&self, id: PolicyId, policy: Policy) -> Result<Revision, Error> {
        let data = encode(&policy)?;
        let key = id.to_be_bytes();
        let revision = (&self.policies, &self.state).transaction(|(policies, state)| {
            if policies.get(key)?.as_deref() == Some(data.as_slice()) {
                return read_revision(state);
            }
            policies.insert(key.as_slice(), data.as_slice())?;
            bump_revision(state)
        })?;
        self.flush().await?;
        Ok(revision)
    }

    async fn delete_policy(&self, id: PolicyId) -> Result<Revision, Error> {
        let reverse_key = reverse_key(id);
        let revision = (&self.policies, &self.bindings, &self.state).transaction(
            |(policies, bindings, state)| {
                if policies.remove(id.to_be_bytes().as_slice())?.is_none() {
                    return read_revision(state);
                }
                let selectors: BTreeSet<PrincipalSelector> = read_set(bindings, &reverse_key)?;
                for selector in selectors {
                    let key = selector_key(selector);
                    let mut ids: BTreeSet<PolicyId> = read_set(bindings, &key)?;
                    if !ids.remove(&id) {
                        return Err(Error::CorruptState("binding indexes disagree").into());
                    }
                    write_set(bindings, &key, &ids)?;
                }
                bindings.remove(reverse_key.as_slice())?;
                bump_revision(state)
            },
        )?;
        self.flush().await?;
        Ok(revision)
    }

    async fn attach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> Result<Revision, Error> {
        self.change_binding(selector, policy, true).await
    }

    async fn detach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> Result<Revision, Error> {
        self.change_binding(selector, policy, false).await
    }

    async fn load_policies(
        &self,
        selectors: &[PrincipalSelector],
    ) -> Result<PolicySnapshot, Error> {
        Ok((&self.policies, &self.bindings, &self.state).transaction(
            |(policies, bindings, state)| {
                let revision = read_revision(state)?;
                let mut ids = BTreeSet::new();
                for &selector in selectors {
                    ids.extend(read_set::<PolicyId>(bindings, &selector_key(selector))?);
                }
                let mut loaded = Vec::new();
                for id in ids {
                    let bytes = policies
                        .get(id.to_be_bytes())?
                        .ok_or(Error::CorruptState("bound policy is missing"))?;
                    loaded.push((id, decode(&bytes)?));
                }
                Ok(PolicySnapshot {
                    revision,
                    policies: loaded,
                })
            },
        )?)
    }
}
