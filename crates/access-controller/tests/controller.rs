use std::{
    error::Error as StdError,
    sync::{Arc, Barrier},
};

use access_controller::{
    AccessController, Action, Actions, CompilationLimits, Decision, Error, PolicyRegistry,
    Principal,
};
use futures_executor::block_on;
use policy_registry::{
    Effect, LocalRegistry, Policy, PrincipalSelector, ResourcePattern, Revision, Statement,
    traits::PolicyRegistry as _,
};

type TestResult<T = ()> = Result<T, Box<dyn StdError + Send + Sync>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn local() -> TestResult<PolicyRegistry> {
    Ok(PolicyRegistry::Local(LocalRegistry::new(
        sled::Config::new()
            .temporary(true)
            .flush_every_ms(None)
            .open()?,
    )?))
}

fn policy(effect: Effect, actions: Actions, key: &str) -> Policy {
    Policy {
        statements: vec![Statement {
            effect,
            actions,
            resource: ResourcePattern::Prefix(key.into()),
        }],
    }
}

#[test]
fn anonymous_authenticated_and_specific_policies_are_combined() -> TestResult {
    block_on(async {
        let registry = local()?;
        for (id, selector, effect, actions, key) in [
            (
                1,
                PrincipalSelector::Everyone,
                Effect::Allow,
                Actions::READ,
                "public/",
            ),
            (
                2,
                PrincipalSelector::Authenticated,
                Effect::Allow,
                Actions::WRITE,
                "shared/",
            ),
            (
                3,
                PrincipalSelector::Specific(10),
                Effect::Allow,
                Actions::READ,
                "alice/",
            ),
            (
                4,
                PrincipalSelector::Specific(10),
                Effect::Deny,
                Actions::READ,
                "public/blocked/",
            ),
        ] {
            registry
                .put_policy(id, policy(effect, actions, key))
                .await?;
            registry.attach(selector, id).await?;
        }
        // The same policy can be shared by public and specific bindings.
        registry.attach(PrincipalSelector::Specific(10), 1).await?;
        let controller = AccessController::new(registry);
        for (principal, action, key, expected) in [
            (
                Principal::Anonymous,
                Action::Read,
                "public/a",
                Decision::Allow,
            ),
            (
                Principal::Anonymous,
                Action::Write,
                "shared/a",
                Decision::Deny,
            ),
            (
                Principal::Anonymous,
                Action::Read,
                "alice/a",
                Decision::Deny,
            ),
            (
                Principal::Authenticated(20),
                Action::Read,
                "public/a",
                Decision::Allow,
            ),
            (
                Principal::Authenticated(20),
                Action::Write,
                "shared/a",
                Decision::Allow,
            ),
            (
                Principal::Authenticated(20),
                Action::Read,
                "alice/a",
                Decision::Deny,
            ),
            (
                Principal::Authenticated(10),
                Action::Read,
                "alice/a",
                Decision::Allow,
            ),
            (
                Principal::Authenticated(10),
                Action::Read,
                "public/blocked/a",
                Decision::Deny,
            ),
            (
                Principal::Authenticated(20),
                Action::Read,
                "public/blocked/a",
                Decision::Allow,
            ),
        ] {
            check(
                controller.authorize(principal, action, key).await? == expected,
                "principal selector union is incorrect",
            )?;
        }
        Ok(())
    })
}

#[test]
fn direct_authorization_refreshes_while_compiled_snapshots_remain_immutable() -> TestResult {
    block_on(async {
        let registry = local()?;
        registry
            .put_policy(1, policy(Effect::Allow, Actions::READ, "photos/"))
            .await?;
        registry.attach(PrincipalSelector::Everyone, 1).await?;
        let controller = AccessController::new(registry.clone());
        let old = controller.compile(Principal::Anonymous).await?;
        check(
            old.revision() == Revision(2),
            "compiled revision was not retained",
        )?;
        let revision = registry
            .put_policy(1, policy(Effect::Deny, Actions::READ, "photos/"))
            .await?;
        check(
            old.authorize(Action::Read, "photos/a") == Decision::Allow,
            "mutation changed an existing snapshot",
        )?;
        check(
            controller
                .authorize(Principal::Anonymous, Action::Read, "photos/a")
                .await?
                == Decision::Deny,
            "direct authorization reused stale policies",
        )?;
        let fresh = controller.compile(Principal::Anonymous).await?;
        check(
            fresh.revision() == revision,
            "fresh compilation lost the new revision",
        )?;
        drop(controller);
        drop(registry);
        check(
            old.authorize(Action::Read, "photos/a") == Decision::Allow,
            "compiled snapshot borrowed its registry",
        )
    })
}

#[test]
fn technical_errors_retain_the_registry_cause() -> TestResult {
    block_on(async {
        let database = sled::Config::new()
            .temporary(true)
            .flush_every_ms(None)
            .open()?;
        let registry = PolicyRegistry::Local(LocalRegistry::new(database.clone())?);
        registry
            .put_policy(1, policy(Effect::Allow, Actions::ALL, ""))
            .await?;
        registry.attach(PrincipalSelector::Everyone, 1).await?;
        // Simulate a dangling binding in this test's temporary database.
        database
            .open_tree("policy-records")?
            .remove(1_u128.to_be_bytes())?;
        let controller = AccessController::new(registry);
        let error = controller
            .authorize(Principal::Anonymous, Action::Read, "anything")
            .await
            .err()
            .ok_or("registry error became an authorization decision")?;
        check(
            matches!(
                &error,
                Error::Registry(policy_registry::Error::CorruptState(
                    "bound policy is missing"
                ))
            ),
            "registry error was masked",
        )?;
        check(error.source().is_some(), "registry error cause was lost")?;
        Ok(())
    })
}

#[test]
fn compiler_errors_are_not_converted_into_allow() -> TestResult {
    block_on(async {
        let registry = local()?;
        registry
            .put_policy(1, policy(Effect::Allow, Actions::ALL, ""))
            .await?;
        registry.attach(PrincipalSelector::Everyone, 1).await?;
        let controller = AccessController::with_limits(
            registry,
            CompilationLimits {
                max_statements: 0,
                ..CompilationLimits::default()
            },
        );
        check(
            matches!(
                controller
                    .authorize(Principal::Anonymous, Action::Read, "anything")
                    .await,
                Err(Error::CompilationLimit("statements"))
            ),
            "compilation error became an authorization decision",
        )
    })
}

#[test]
fn concurrent_loads_return_consistent_snapshots() -> TestResult {
    fn require_send<T: Send>(_: &T) {}
    let registry = local()?;
    block_on(async {
        registry
            .put_policy(1, policy(Effect::Allow, Actions::READ, "photos/"))
            .await?;
        registry.attach(PrincipalSelector::Everyone, 1).await?;
        Ok::<_, policy_registry::Error>(())
    })?;
    let controller = Arc::new(AccessController::new(registry));
    let mut workers = Vec::new();
    for _ in 0..16 {
        let controller = Arc::clone(&controller);
        workers.push(std::thread::Builder::new().spawn(move || -> TestResult {
            for _ in 0..32 {
                let future = controller.compile(Principal::Authenticated(10));
                require_send(&future);
                let compiled = block_on(future)?;
                check(
                    compiled.revision() == Revision(2),
                    "concurrent loading lost the snapshot revision",
                )?;
                check(
                    compiled.authorize(Action::Read, "photos/a") == Decision::Allow,
                    "concurrent loading lost permissions",
                )?;
            }
            Ok(())
        })?);
    }
    for worker in workers {
        worker.join().map_err(|_| "loading worker panicked")??;
    }
    Ok(())
}

#[test]
fn policy_mutation_during_loading_preserves_revision_consistency() -> TestResult {
    let registry = local()?;
    block_on(async {
        registry
            .put_policy(1, policy(Effect::Allow, Actions::READ, "revision/0/"))
            .await?;
        registry.attach(PrincipalSelector::Everyone, 1).await?;
        Ok::<_, policy_registry::Error>(())
    })?;
    let barrier = Arc::new(Barrier::new(2));
    let writer = registry.clone();
    let start = Arc::clone(&barrier);
    let writer = std::thread::Builder::new().spawn(move || -> TestResult {
        start.wait();
        block_on(async {
            for step in 1..=128_u64 {
                writer
                    .put_policy(
                        1,
                        policy(Effect::Allow, Actions::READ, &format!("revision/{step}/")),
                    )
                    .await?;
            }
            Ok(())
        })
    })?;
    let controller = AccessController::new(registry);
    barrier.wait();
    let result = block_on(async {
        for _ in 0..256 {
            let compiled = controller.compile(Principal::Anonymous).await?;
            let step = compiled
                .revision()
                .0
                .checked_sub(2)
                .ok_or("snapshot revision regressed")?;
            let next = step.checked_add(1).ok_or("test revision overflow")?;
            check(
                compiled.authorize(Action::Read, &format!("revision/{step}/a")) == Decision::Allow,
                "revision and policy came from different transactions",
            )?;
            check(
                compiled.authorize(Action::Read, &format!("revision/{next}/a")) == Decision::Deny,
                "snapshot mixed multiple revisions",
            )?;
        }
        Ok::<_, Box<dyn StdError + Send + Sync>>(())
    });
    writer.join().map_err(|_| "mutation worker panicked")??;
    result
}
