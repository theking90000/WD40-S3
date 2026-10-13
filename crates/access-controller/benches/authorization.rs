use std::{
    error::Error as StdError,
    hint::black_box,
    time::{Duration, Instant},
};

use access_controller::{Action, Actions, CompilationLimits, CompiledPolicy};
use futures_executor::block_on;
use policy_registry::{
    Effect, LocalRegistry, Policy, PolicySnapshot, PrincipalSelector, ResourcePattern, Revision,
    Statement, traits::PolicyRegistry,
};

type BenchResult<T = ()> = Result<T, Box<dyn StdError + Send + Sync>>;

/// Preparation and destruction are outside the measured operation.
fn measure<T, U>(
    name: &str,
    iterations: u32,
    mut prepare: impl FnMut() -> BenchResult<T>,
    mut run: impl FnMut(T) -> BenchResult<U>,
) -> BenchResult {
    let mut elapsed = Duration::ZERO;
    for _ in 0..iterations {
        let input = prepare()?;
        let start = Instant::now();
        let output = run(input)?;
        elapsed = elapsed
            .checked_add(start.elapsed())
            .ok_or("benchmark duration overflow")?;
        drop(black_box(output));
    }
    let mean = elapsed.checked_div(iterations).ok_or("empty benchmark")?;
    println!("{name}: {mean:?} per iteration ({iterations} iterations)");
    Ok(())
}

fn evaluate(name: &str, snapshot: PolicySnapshot, keys: Vec<String>) -> BenchResult {
    measure(
        name,
        20,
        || Ok(snapshot.clone()),
        |snapshot| {
            Ok(CompiledPolicy::from_snapshot(
                snapshot,
                CompilationLimits::default(),
            )?)
        },
    )?;
    let compiled = CompiledPolicy::from_snapshot(snapshot, CompilationLimits::default())?;
    measure(
        &format!("{name} evaluation, {} keys", keys.len()),
        200,
        || Ok(()),
        |()| {
            for key in &keys {
                black_box(compiled.authorize(black_box(Action::List), black_box(key.as_str())));
            }
            Ok(())
        },
    )
}

fn main() -> BenchResult {
    let registry = LocalRegistry::new(
        sled::Config::new()
            .temporary(true)
            .flush_every_ms(None)
            .open()?,
    )?;
    block_on(async {
        for id in 0..64_u128 {
            let mut statements = Vec::new();
            for resource in 0..128 {
                statements.push(Statement {
                    effect: Effect::Allow,
                    actions: Actions::READ | Actions::LIST,
                    resource: ResourcePattern::Prefix(format!("photos/shared/{id}/{resource}/")),
                });
            }
            statements.push(Statement {
                effect: Effect::Deny,
                actions: Actions::LIST,
                resource: ResourcePattern::Prefix(format!("photos/shared/{id}/0/private/")),
            });
            registry.put_policy(id, Policy { statements }).await?;
            for selector in [
                PrincipalSelector::Everyone,
                PrincipalSelector::Specific(1),
                PrincipalSelector::Specific(2),
            ] {
                registry.attach(selector, id).await?;
            }
        }
        Ok::<_, policy_registry::Error>(())
    })?;
    for principal in [1, 2] {
        let selectors = [
            PrincipalSelector::Everyone,
            PrincipalSelector::Specific(principal),
        ];
        measure(
            &format!("registry loading, shared policies, principal {principal}"),
            100,
            || Ok(()),
            |()| Ok(block_on(registry.load_policies(&selectors))?),
        )?;
    }
    let snapshot = block_on(registry.load_policies(&[PrincipalSelector::Everyone]))?;
    let mut keys = Vec::new();
    for id in 0..64 {
        for resource in 0..128 {
            keys.push(format!("photos/shared/{id}/{resource}/file"));
        }
        keys.push(format!("photos/shared/{id}/0/private/file"));
        keys.push(format!("photos/shared/{id}/128/file"));
    }
    evaluate("shared prefixes compilation", snapshot, keys)?;

    let mut statements = Vec::new();
    for depth in 1..=1024 {
        statements.push(Statement {
            effect: Effect::Allow,
            actions: Actions::LIST,
            resource: ResourcePattern::Prefix("a".repeat(depth)),
        });
    }
    let keys: Vec<_> = [1, 511, 512, 1024, 4096]
        .into_iter()
        .map(|length| "a".repeat(length))
        .collect();
    let snapshot = PolicySnapshot {
        revision: Revision(0),
        policies: vec![(
            1,
            Policy {
                statements: statements.clone(),
            },
        )],
    };
    evaluate("nested allow prefixes compilation", snapshot, keys.clone())?;
    statements.push(Statement {
        effect: Effect::Deny,
        actions: Actions::LIST,
        resource: ResourcePattern::Prefix("a".repeat(512)),
    });
    let snapshot = PolicySnapshot {
        revision: Revision(0),
        policies: vec![(1, Policy { statements })],
    };
    evaluate(
        "nested prefixes with parent denial compilation",
        snapshot,
        keys,
    )
}
