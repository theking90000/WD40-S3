use std::{
    error::Error as StdError,
    sync::{Arc, Barrier, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_executor::block_on;
use policy_registry::{
    Action, Actions, Effect, Error, LocalRegistry, Policy, PolicyRegistry, PolicySnapshot,
    PrincipalSelector, ResourcePattern, Revision, Statement, traits::PolicyRegistry as _,
};

type TestResult<T = ()> = Result<T, Box<dyn StdError + Send + Sync>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn database() -> TestResult<sled::Db> {
    Ok(sled::Config::new()
        .temporary(true)
        .flush_every_ms(None)
        .open()?)
}

fn policy(resource: &str) -> Policy {
    Policy {
        statements: vec![
            Statement {
                effect: Effect::Allow,
                actions: Actions::ALL,
                resource: ResourcePattern::Prefix(resource.to_owned()),
            },
            Statement {
                effect: Effect::Deny,
                actions: Actions::WRITE,
                resource: ResourcePattern::Exact("任意\0key".to_owned()),
            },
        ],
    }
}

fn require_send<T: Send>(_: &T) {}

#[test]
fn creation_allocates_distinct_ids_without_replacing_explicit_policies() -> TestResult {
    block_on(async {
        let db = database()?;
        let occupied = u128::from(db.generate_id()?.checked_add(1).ok_or("ID overflow")?);
        let registry = PolicyRegistry::Local(LocalRegistry::new(db)?);
        let original = policy("explicit/");
        let value = policy("created/");
        registry.put_policy(occupied, original.clone()).await?;
        registry
            .attach(PrincipalSelector::Everyone, occupied)
            .await?;
        let create = registry.create_policy(value.clone());
        require_send(&create);
        let first = create.await?;
        check(first > occupied, "creation did not skip occupied ID")?;
        check(
            registry.get_policy(first).await? == Some(value.clone()),
            "created policy content changed",
        )?;
        check(
            registry.get_policy(occupied).await? == Some(original.clone()),
            "creation replaced explicit policy",
        )?;
        check(
            registry
                .load_policies(&[PrincipalSelector::Everyone])
                .await?
                == PolicySnapshot {
                    revision: Revision(3),
                    policies: vec![(occupied, original)],
                },
            "creation changed existing bindings or incremented revision more than once",
        )?;
        let second = registry.create_policy(value.clone()).await?;
        check(first != second, "identical creations reused an ID")?;
        check(
            registry.get_policy(second).await? == Some(value)
                && registry.load_policies(&[]).await?.revision == Revision(4),
            "second creation lost content or revision",
        )
    })
}

#[test]
fn concurrent_creations_across_clones_and_independent_handles_allocate_distinct_ids() -> TestResult
{
    let db = database()?;
    let registry = LocalRegistry::new(db.clone())?;
    let barrier = Arc::new(Barrier::new(9));
    let (sender, receiver) = mpsc::channel();
    let mut workers = Vec::new();
    for number in 0..8 {
        let handle = if number % 2 == 0 {
            registry.clone()
        } else {
            LocalRegistry::new(db.clone())?
        };
        let barrier = barrier.clone();
        let sender = sender.clone();
        workers.push(std::thread::Builder::new().spawn(move || {
            barrier.wait();
            let result: TestResult<u128> =
                block_on(async { Ok(handle.create_policy(policy("concurrent/")).await?) });
            let _ = sender.send(result);
        })?);
    }
    barrier.wait();
    let mut ids = std::collections::BTreeSet::new();
    for _ in 0..8 {
        check(
            ids.insert(receiver.recv_timeout(Duration::from_secs(20))??),
            "concurrent creations returned duplicate IDs",
        )?;
    }
    for worker in workers {
        worker.join().map_err(|_| "creation worker panicked")?;
    }
    block_on(async {
        for id in ids {
            check(
                registry.get_policy(id).await? == Some(policy("concurrent/")),
                "concurrent creation lost policy",
            )?;
        }
        check(
            registry.load_policies(&[]).await?.revision == Revision(8),
            "concurrent creations lost revision updates",
        )
    })
}

#[test]
fn action_masks_preserve_legacy_list_serialization() -> TestResult {
    let legacy = vec![Action::Write, Action::Read, Action::Write];
    let bytes = postcard::to_allocvec(&legacy)?;
    let mut actions = postcard::from_bytes::<Actions>(&bytes)?;
    check(
        actions == Actions::READ | Actions::WRITE,
        "legacy action list was not normalized",
    )?;
    actions |= Actions::LIST;
    check(
        actions.contains(Action::List) && !actions.contains(Action::Delete),
        "action mask union changed unrelated permissions",
    )?;
    let bytes = postcard::to_allocvec(&actions)?;
    check(
        postcard::from_bytes::<Vec<Action>>(&bytes)?
            == vec![Action::Read, Action::Write, Action::List],
        "serialized mask is not a compatible canonical action list",
    )?;
    let bytes = postcard::to_allocvec(&Vec::<Action>::new())?;
    check(
        postcard::from_bytes::<Actions>(&bytes)?.is_empty(),
        "legacy empty list is not an empty mask",
    )
}

#[test]
fn lifecycle_revisions_idempotence_and_owned_snapshot() -> TestResult {
    block_on(async {
        let registry = PolicyRegistry::Local(LocalRegistry::new(database()?)?);
        let id = u128::MAX;
        let selector = PrincipalSelector::Specific(u128::MAX);
        let original = policy("photos/");
        check(
            registry.get_policy(id).await?.is_none(),
            "missing policy is not None",
        )?;
        check(
            registry.load_policies(&[]).await?.revision == Revision(0),
            "initial revision is not zero",
        )?;
        let put = registry.put_policy(id, original.clone());
        require_send(&put);
        check(put.await? == Revision(1), "put did not increment revision")?;
        check(
            registry.put_policy(id, original.clone()).await? == Revision(1),
            "identical put changed revision",
        )?;
        let get = registry.get_policy(id);
        require_send(&get);
        check(
            get.await? == Some(original.clone()),
            "policy content changed",
        )?;
        let attach = registry.attach(selector, id);
        require_send(&attach);
        check(
            attach.await? == Revision(2),
            "attach did not increment revision",
        )?;
        check(
            registry.attach(selector, id).await? == Revision(2),
            "repeated attach changed revision",
        )?;
        let selectors = [selector];
        let load = registry.load_policies(&selectors);
        require_send(&load);
        let snapshot = load.await?;
        check(
            snapshot
                == PolicySnapshot {
                    revision: Revision(2),
                    policies: vec![(id, original)],
                },
            "invalid snapshot",
        )?;
        let changed = policy("other/");
        check(
            registry.put_policy(id, changed.clone()).await? == Revision(3),
            "replacement did not increment revision",
        )?;
        check(
            registry.load_policies(&[selector]).await?.policies == vec![(id, changed)],
            "replacement lost binding",
        )?;
        let detach = registry.detach(selector, id);
        require_send(&detach);
        check(
            detach.await? == Revision(4),
            "detach did not increment revision",
        )?;
        check(
            registry.detach(selector, id).await? == Revision(4),
            "repeated detach changed revision",
        )?;
        let delete = registry.delete_policy(id);
        require_send(&delete);
        check(
            delete.await? == Revision(5),
            "delete did not increment revision",
        )?;
        check(
            registry.delete_policy(id).await? == Revision(5),
            "repeated delete changed revision",
        )?;
        drop(registry);
        check(
            snapshot.policies.len() == 1,
            "snapshot did not own its policies",
        )
    })
}

#[test]
fn selectors_union_is_explicit_deduplicated_and_sorted() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(database()?)?;
        let everyone = PrincipalSelector::Everyone;
        let authenticated = PrincipalSelector::Authenticated;
        let alice = PrincipalSelector::Specific(0);
        let bob = PrincipalSelector::Specific(1);
        let value = policy("");
        for id in [0, 1, 2, u128::MAX] {
            registry.put_policy(id, value.clone()).await?;
        }
        registry.attach(everyone, 2).await?;
        registry.attach(authenticated, 1).await?;
        registry.attach(alice, u128::MAX).await?;
        registry.attach(alice, 2).await?;
        registry.attach(bob, 0).await?;
        check(
            registry.load_policies(&[alice]).await?.policies
                == vec![(2, value.clone()), (u128::MAX, value.clone())],
            "specific selector implicitly included other bindings",
        )?;
        let snapshot = registry
            .load_policies(&[alice, everyone, authenticated, alice])
            .await?;
        check(
            snapshot.policies == vec![(1, value.clone()), (2, value.clone()), (u128::MAX, value)],
            "union order, deduplication or isolation failed",
        )?;
        check(
            snapshot.revision == Revision(9),
            "global revision missed mutations",
        )?;
        let empty = registry.load_policies(&[]).await?;
        check(
            empty.policies.is_empty() && empty.revision == snapshot.revision,
            "empty selector snapshot changed revision",
        )?;
        let bytes = postcard::to_allocvec(&snapshot)?;
        check(
            postcard::from_bytes::<PolicySnapshot>(&bytes)? == snapshot,
            "snapshot serialization lost data",
        )
    })
}

#[test]
fn deletion_removes_all_bindings_without_restoring_them_on_recreation() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(database()?)?;
        let selectors = [
            PrincipalSelector::Everyone,
            PrincipalSelector::Authenticated,
            PrincipalSelector::Specific(42),
        ];
        let value = policy("photos/");
        registry.put_policy(1, value.clone()).await?;
        registry.put_policy(2, value.clone()).await?;
        for selector in selectors {
            registry.attach(selector, 1).await?;
            registry.attach(selector, 2).await?;
        }
        check(
            registry.delete_policy(1).await? == Revision(9),
            "delete did not increment exactly once",
        )?;
        check(
            registry.get_policy(1).await?.is_none(),
            "deleted policy is readable",
        )?;
        for selector in selectors {
            check(
                registry.load_policies(&[selector]).await?.policies == vec![(2, value.clone())],
                "delete damaged unrelated bindings or retained deleted policy",
            )?;
        }
        registry.put_policy(1, value.clone()).await?;
        check(
            registry.load_policies(&selectors).await?.policies == vec![(2, value)],
            "recreation restored old bindings",
        )?;
        check(
            matches!(
                registry.attach(PrincipalSelector::Everyone, 3).await,
                Err(Error::PolicyNotFound(3))
            ),
            "missing policy attachment succeeded",
        )?;
        check(
            registry.detach(PrincipalSelector::Everyone, 3).await? == Revision(10),
            "missing detach changed revision",
        )
    })
}

#[test]
fn empty_policies_and_statements_are_retained() -> TestResult {
    block_on(async {
        let registry = LocalRegistry::new(database()?)?;
        for (id, value) in [
            (0, Policy { statements: vec![] }),
            (
                1,
                Policy {
                    statements: vec![Statement {
                        effect: Effect::Deny,
                        actions: Actions::EMPTY,
                        resource: ResourcePattern::Exact(String::new()),
                    }],
                },
            ),
        ] {
            registry.put_policy(id, value.clone()).await?;
            check(
                registry.get_policy(id).await? == Some(value),
                "empty policy data changed",
            )?;
        }
        Ok(())
    })
}

#[test]
fn revision_overflow_rolls_back_each_mutation() -> TestResult {
    block_on(async {
        let db = database()?;
        let registry = LocalRegistry::new(db.clone())?;
        let selector = PrincipalSelector::Everyone;
        let value = policy("photos/");
        registry.put_policy(1, value.clone()).await?;
        registry.put_policy(2, value.clone()).await?;
        registry.attach(selector, 1).await?;
        db.open_tree("policy-state")?
            .insert(b"revision", postcard::to_allocvec(&Revision(u64::MAX))?)?;
        let before = registry.load_policies(&[selector]).await?;
        check(
            matches!(
                registry.create_policy(value.clone()).await,
                Err(Error::RevisionOverflow)
            ),
            "allocated creation accepted revision overflow",
        )?;
        check(
            db.open_tree("policy-records")?.len() == 2,
            "failed creation inserted a policy",
        )?;
        check(
            matches!(
                registry.put_policy(1, policy("changed/")).await,
                Err(Error::RevisionOverflow)
            ),
            "replacement accepted revision overflow",
        )?;
        check(
            matches!(
                registry.put_policy(3, value.clone()).await,
                Err(Error::RevisionOverflow)
            ),
            "creation accepted revision overflow",
        )?;
        check(
            matches!(
                registry.attach(selector, 2).await,
                Err(Error::RevisionOverflow)
            ),
            "attach accepted revision overflow",
        )?;
        check(
            matches!(
                registry.detach(selector, 1).await,
                Err(Error::RevisionOverflow)
            ),
            "detach accepted revision overflow",
        )?;
        check(
            matches!(
                registry.delete_policy(1).await,
                Err(Error::RevisionOverflow)
            ),
            "delete accepted revision overflow",
        )?;
        check(
            registry.load_policies(&[selector]).await? == before,
            "overflow partially changed snapshot",
        )?;
        check(
            registry.get_policy(1).await? == Some(value.clone())
                && registry.get_policy(3).await?.is_none(),
            "overflow partially changed policies",
        )?;
        check(
            registry.put_policy(1, value).await? == Revision(u64::MAX),
            "identical put failed at max revision",
        )?;
        check(
            registry.attach(selector, 1).await? == Revision(u64::MAX),
            "repeated attach failed at max revision",
        )?;
        check(
            registry.detach(selector, 2).await? == Revision(u64::MAX),
            "missing detach failed at max revision",
        )?;
        check(
            registry.delete_policy(3).await? == Revision(u64::MAX),
            "missing delete failed at max revision",
        )
    })
}

#[test]
fn corrupt_state_and_indexes_abort_without_partial_changes() -> TestResult {
    block_on(async {
        let db = database()?;
        let registry = LocalRegistry::new(db.clone())?;
        let value = policy("photos/");
        registry.put_policy(7, value.clone()).await?;
        registry.attach(PrincipalSelector::Everyone, 7).await?;
        let bindings = db.open_tree("policy-bindings")?;
        let old = bindings
            .insert(
                [0, 0],
                postcard::to_allocvec(&std::collections::BTreeSet::<u128>::new())?,
            )?
            .ok_or("missing binding")?;
        check(
            matches!(
                registry.detach(PrincipalSelector::Everyone, 7).await,
                Err(Error::CorruptState(_))
            ),
            "detach accepted inconsistent indexes",
        )?;
        check(
            matches!(registry.delete_policy(7).await, Err(Error::CorruptState(_))),
            "delete accepted inconsistent indexes",
        )?;
        check(
            registry.get_policy(7).await? == Some(value.clone()),
            "failed delete removed policy",
        )?;
        check(
            registry.load_policies(&[]).await?.revision == Revision(2),
            "failed mutation changed revision",
        )?;
        bindings.insert([0, 0], old)?;
        let policies = db.open_tree("policy-records")?;
        let mut trailing = postcard::to_allocvec(&value)?;
        trailing.push(0);
        policies.insert(7_u128.to_be_bytes().as_slice(), trailing)?;
        check(
            matches!(registry.get_policy(7).await, Err(Error::CorruptState(_))),
            "trailing bytes accepted",
        )?;
        policies.insert(7_u128.to_be_bytes().as_slice(), vec![0xff])?;
        let error = registry
            .get_policy(7)
            .await
            .err()
            .ok_or("corrupt policy accepted")?;
        check(
            matches!(&error, Error::Serialization(_)) && error.source().is_some(),
            "serialization cause lost",
        )?;
        check(
            matches!(
                registry.load_policies(&[PrincipalSelector::Everyone]).await,
                Err(Error::Serialization(_))
            ),
            "snapshot accepted corrupt policy",
        )?;
        Ok(())
    })
}

#[test]
fn persistent_reopen_preserves_revision_policy_and_bindings() -> TestResult {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path =
        std::env::temp_dir().join(format!("wd40-policy-test-{}-{unique}", std::process::id()));
    let value = policy("photos/");
    let selector = PrincipalSelector::Specific(u128::MAX);
    let result: TestResult = block_on(async {
        let id = {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            let id = registry.create_policy(value.clone()).await?;
            registry.attach(selector, id).await?;
            id
        };
        let registry = LocalRegistry::new(sled::open(&path)?)?;
        check(
            registry.load_policies(&[selector]).await?
                == PolicySnapshot {
                    revision: Revision(2),
                    policies: vec![(id, value.clone())],
                },
            "reopen lost persisted state",
        )?;
        check(
            registry.detach(selector, id).await? == Revision(3),
            "reopen reset revision",
        )?;
        let next = registry.create_policy(value.clone()).await?;
        check(next != id, "reopen reused an allocated ID")?;
        check(
            registry.get_policy(id).await? == Some(value.clone())
                && registry.get_policy(next).await? == Some(value)
                && registry.load_policies(&[]).await?.revision == Revision(4),
            "creation after reopen lost policy or revision",
        )
    });
    std::fs::remove_dir_all(path)?;
    result
}

#[test]
fn concurrent_independent_handles_keep_snapshots_and_revisions_consistent() -> TestResult {
    let db = database()?;
    let registry = LocalRegistry::new(db.clone())?;
    block_on(async {
        registry.put_policy(1, policy("0")).await?;
        registry.attach(PrincipalSelector::Everyone, 1).await?;
        Ok::<_, Box<dyn StdError + Send + Sync>>(())
    })?;
    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    let (sender, receiver) = mpsc::channel();
    for number in 0..8 {
        let handle = LocalRegistry::new(db.clone())?;
        let barrier = barrier.clone();
        let sender = sender.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let result: TestResult = block_on(async {
                if number == 0 {
                    for step in 1..=40 {
                        check(
                            handle.put_policy(1, policy(&step.to_string())).await?
                                == Revision(step + 2),
                            "concurrent writer revision changed",
                        )?;
                    }
                } else {
                    for _ in 0..80 {
                        let snapshot = handle.load_policies(&[PrincipalSelector::Everyone]).await?;
                        let step = snapshot
                            .revision
                            .0
                            .checked_sub(2)
                            .ok_or("snapshot revision regressed")?;
                        check(
                            snapshot.policies == vec![(1, policy(&step.to_string()))],
                            "snapshot mixed revision and policy from different commits",
                        )?;
                    }
                }
                Ok(())
            });
            let _ = sender.send(result);
        }));
    }
    barrier.wait();
    for _ in 0..8 {
        receiver.recv_timeout(Duration::from_secs(20))??;
    }
    for worker in workers {
        worker.join().map_err(|_| "snapshot worker panicked")?;
    }

    let barrier = Arc::new(Barrier::new(17));
    let mut workers = Vec::new();
    for id in 2..18 {
        let handle = LocalRegistry::new(db.clone())?;
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || -> TestResult<Revision> {
            barrier.wait();
            block_on(async {
                handle.put_policy(id, policy("concurrent/")).await?;
                Ok(handle.attach(PrincipalSelector::Everyone, id).await?)
            })
        }));
    }
    barrier.wait();
    for worker in workers {
        worker.join().map_err(|_| "binding worker panicked")??;
    }
    let snapshot = block_on(registry.load_policies(&[PrincipalSelector::Everyone]))?;
    check(
        snapshot.revision == Revision(74) && snapshot.policies.len() == 17,
        "concurrent bindings lost updates",
    )
}

#[test]
fn sixteen_independent_databases_flush_without_starvation() -> TestResult {
    let (sender, receiver) = mpsc::channel();
    let barrier = Arc::new(Barrier::new(17));
    let mut workers = Vec::new();
    for id in 0..16 {
        let registry = LocalRegistry::new(database()?)?;
        let sender = sender.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            let result: TestResult = block_on(async {
                registry.put_policy(id, policy("photos/")).await?;
                registry.attach(PrincipalSelector::Everyone, id).await?;
                registry.delete_policy(id).await?;
                Ok(())
            });
            let _ = sender.send(result);
        }));
    }
    barrier.wait();
    for _ in 0..16 {
        receiver.recv_timeout(Duration::from_secs(20))??;
    }
    for worker in workers {
        worker.join().map_err(|_| "flush worker panicked")?;
    }
    Ok(())
}
