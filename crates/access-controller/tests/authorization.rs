use std::{error::Error as StdError, sync::Arc};

use access_controller::{
    Action, Actions, CompilationLimits, CompiledPolicy, Decision, Error, Revision,
};
use policy_registry::{Effect, Policy, PolicySnapshot, ResourcePattern, Statement};

type TestResult<T = ()> = Result<T, Box<dyn StdError + Send + Sync>>;

fn check(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn rule(effect: Effect, actions: Actions, resource: ResourcePattern) -> Statement {
    Statement {
        effect,
        actions,
        resource,
    }
}

fn compile(statements: Vec<Statement>) -> Result<CompiledPolicy, Error> {
    CompiledPolicy::from_snapshot(
        PolicySnapshot {
            revision: Revision(42),
            policies: vec![(1, Policy { statements })],
        },
        CompilationLimits::default(),
    )
}

#[test]
fn empty_snapshots_policies_and_actions_deny() -> TestResult {
    let empty = CompiledPolicy::from_snapshot(
        PolicySnapshot {
            revision: Revision(7),
            policies: vec![],
        },
        CompilationLimits::default(),
    )?;
    let policy = compile(vec![])?;
    let actions = compile(vec![rule(
        Effect::Allow,
        Actions::EMPTY,
        ResourcePattern::Prefix(String::new()),
    )])?;
    for compiled in [empty, policy, actions] {
        for action in Actions::ALL.iter() {
            for key in ["", "photos/", "photos/a.jpg"] {
                check(
                    compiled.authorize(action, key) == Decision::Deny,
                    "empty permissions allowed access",
                )?;
            }
        }
    }
    Ok(())
}

#[test]
fn policies_merge_masks_without_overwriting_other_effects_or_actions() -> TestResult {
    let snapshot = PolicySnapshot {
        revision: Revision(3),
        policies: vec![
            (
                1,
                Policy {
                    statements: vec![
                        rule(
                            Effect::Allow,
                            Actions::READ,
                            ResourcePattern::Prefix("items/".into()),
                        ),
                        rule(
                            Effect::Allow,
                            Actions::DELETE,
                            ResourcePattern::Exact("items/a".into()),
                        ),
                    ],
                },
            ),
            (
                2,
                Policy {
                    statements: vec![
                        rule(
                            Effect::Allow,
                            Actions::WRITE,
                            ResourcePattern::Prefix("items/".into()),
                        ),
                        rule(
                            Effect::Deny,
                            Actions::READ,
                            ResourcePattern::Prefix("items/".into()),
                        ),
                        rule(
                            Effect::Allow,
                            Actions::LIST,
                            ResourcePattern::Exact("items/a".into()),
                        ),
                        rule(
                            Effect::Deny,
                            Actions::DELETE,
                            ResourcePattern::Exact("items/a".into()),
                        ),
                    ],
                },
            ),
        ],
    };
    let compiled = CompiledPolicy::from_snapshot(snapshot, CompilationLimits::default())?;
    for (action, expected) in [
        (Action::Read, Decision::Deny),
        (Action::Write, Decision::Allow),
        (Action::Delete, Decision::Deny),
        (Action::List, Decision::Allow),
    ] {
        check(
            compiled.authorize(action, "items/a") == expected,
            "resource mask merging overwrote permissions",
        )?;
    }
    Ok(())
}

#[test]
fn exact_prefix_and_literal_resources() -> TestResult {
    let compiled = compile(vec![
        rule(
            Effect::Allow,
            Actions::READ,
            ResourcePattern::Prefix("photos/".into()),
        ),
        rule(
            Effect::Allow,
            Actions::WRITE,
            ResourcePattern::Exact("photos/a.jpg".into()),
        ),
        rule(
            Effect::Allow,
            Actions::DELETE,
            ResourcePattern::Exact("a/../b.*\0".into()),
        ),
        rule(
            Effect::Allow,
            Actions::LIST,
            ResourcePattern::Prefix("literal*".into()),
        ),
    ])?;
    for (action, key, expected) in [
        (Action::Read, "photos/", Decision::Allow),
        (Action::Read, "photos/a.jpg", Decision::Allow),
        (Action::Read, "photos2/a.jpg", Decision::Deny),
        (Action::Read, "photos", Decision::Deny),
        (Action::Write, "photos/a.jpg", Decision::Allow),
        (Action::Write, "photos/a.jpg/child", Decision::Deny),
        (Action::Write, "photos/b.jpg", Decision::Deny),
        (Action::Delete, "a/../b.*\0", Decision::Allow),
        (Action::Delete, "b.*\0", Decision::Deny),
        (Action::Delete, "a/../b.jpg\0", Decision::Deny),
        (Action::List, "literal*", Decision::Allow),
        (Action::List, "literal*/child", Decision::Allow),
        (Action::List, "literalX", Decision::Deny),
    ] {
        check(
            compiled.authorize(action, key) == expected,
            "resource matching changed opaque key semantics",
        )?;
    }
    Ok(())
}

#[test]
fn explicit_denies_override_exact_and_nested_allows() -> TestResult {
    let compiled = compile(vec![
        rule(
            Effect::Allow,
            Actions::ALL,
            ResourcePattern::Prefix("users/alice/".into()),
        ),
        rule(
            Effect::Deny,
            Actions::READ,
            ResourcePattern::Prefix("users/".into()),
        ),
        rule(
            Effect::Allow,
            Actions::READ,
            ResourcePattern::Exact("users/alice/photo.jpg".into()),
        ),
        rule(
            Effect::Deny,
            Actions::WRITE,
            ResourcePattern::Exact("users/alice/private".into()),
        ),
        rule(
            Effect::Deny,
            Actions::DELETE,
            ResourcePattern::Prefix("users/alice/secret/".into()),
        ),
    ])?;
    for (action, key, expected) in [
        (Action::Read, "users/alice/photo.jpg", Decision::Deny),
        (Action::Read, "users/alice/", Decision::Deny),
        (Action::Write, "users/alice/photo.jpg", Decision::Allow),
        (Action::Write, "users/alice/private", Decision::Deny),
        (Action::Write, "users/alice/private/child", Decision::Allow),
        (Action::Delete, "users/alice/secret/", Decision::Deny),
        (
            Action::Delete,
            "users/alice/secret/photo.jpg",
            Decision::Deny,
        ),
        (
            Action::List,
            "users/alice/secret/photo.jpg",
            Decision::Allow,
        ),
        (Action::List, "users/bob/photo.jpg", Decision::Deny),
    ] {
        check(
            compiled.authorize(action, key) == expected,
            "a specific rule replaced a matching denial",
        )?;
    }
    Ok(())
}

#[test]
fn empty_prefix_and_empty_exact_have_distinct_meanings() -> TestResult {
    let compiled = compile(vec![
        rule(
            Effect::Allow,
            Actions::READ,
            ResourcePattern::Prefix(String::new()),
        ),
        rule(
            Effect::Deny,
            Actions::READ,
            ResourcePattern::Exact(String::new()),
        ),
        rule(
            Effect::Allow,
            Actions::WRITE,
            ResourcePattern::Exact(String::new()),
        ),
    ])?;
    check(
        compiled.authorize(Action::Read, "") == Decision::Deny,
        "empty exact denial was ignored",
    )?;
    check(
        compiled.authorize(Action::Read, "anything") == Decision::Allow,
        "empty prefix did not match all keys",
    )?;
    check(
        compiled.authorize(Action::Write, "") == Decision::Allow,
        "empty exact permission was ignored",
    )?;
    check(
        compiled.authorize(Action::Write, "anything") == Decision::Deny,
        "empty exact matched a nonempty key",
    )
}

#[test]
fn compressed_edges_can_split_inside_utf8_characters() -> TestResult {
    let compiled = compile(vec![
        rule(
            Effect::Allow,
            Actions::READ,
            ResourcePattern::Prefix("é/".into()),
        ),
        rule(
            Effect::Allow,
            Actions::WRITE,
            ResourcePattern::Exact("ê/".into()),
        ),
        rule(
            Effect::Deny,
            Actions::READ,
            ResourcePattern::Prefix("é/任意".into()),
        ),
        rule(
            Effect::Allow,
            Actions::LIST,
            ResourcePattern::Prefix("🙂/".into()),
        ),
        rule(
            Effect::Allow,
            Actions::DELETE,
            ResourcePattern::Exact("🙃/".into()),
        ),
    ])?;
    for (action, key, expected) in [
        (Action::Read, "é/", Decision::Allow),
        (Action::Read, "é/photo", Decision::Allow),
        (Action::Read, "é/任意", Decision::Deny),
        (Action::Read, "ê/photo", Decision::Deny),
        (Action::Write, "ê/", Decision::Allow),
        (Action::List, "🙂/photo", Decision::Allow),
        (Action::Delete, "🙃/", Decision::Allow),
        (Action::Delete, "🙂/", Decision::Deny),
    ] {
        check(
            compiled.authorize(action, key) == expected,
            "UTF-8 edge splitting changed matching",
        )?;
    }
    Ok(())
}

#[test]
fn list_visibility_is_evaluated_per_key() -> TestResult {
    let compiled = compile(vec![rule(
        Effect::Allow,
        Actions::LIST,
        ResourcePattern::Exact("photos/a.jpg".into()),
    )])?;
    check(
        compiled.authorize(Action::List, "photos/") == Decision::Deny,
        "an exact child allowed its parent",
    )?;
    let visible: Vec<_> = ["photos/a.jpg", "photos/b.jpg"]
        .into_iter()
        .filter(|key| compiled.authorize(Action::List, key) == Decision::Allow)
        .collect();
    check(
        visible == vec!["photos/a.jpg"],
        "listing imposed a parent-prefix admission check",
    )
}

#[test]
fn compilation_limits_return_errors() -> TestResult {
    let snapshot = PolicySnapshot {
        revision: Revision(0),
        policies: vec![(
            1,
            Policy {
                statements: vec![rule(
                    Effect::Allow,
                    Actions::READ,
                    ResourcePattern::Prefix("x".into()),
                )],
            },
        )],
    };
    for (limits, name) in [
        (
            CompilationLimits {
                max_statements: 0,
                ..CompilationLimits::default()
            },
            "statements",
        ),
        (
            CompilationLimits {
                max_resource_bytes: 0,
                ..CompilationLimits::default()
            },
            "resource bytes",
        ),
        (
            CompilationLimits {
                max_nodes: 1,
                ..CompilationLimits::default()
            },
            "nodes",
        ),
    ] {
        check(
            matches!(CompiledPolicy::from_snapshot(snapshot.clone(), limits), Err(Error::CompilationLimit(limit)) if limit == name),
            "compilation limit did not return the expected error",
        )?;
    }
    let mut large_capacity = String::with_capacity(64);
    large_capacity.push('x');
    let snapshot = PolicySnapshot {
        revision: Revision(0),
        policies: vec![(
            1,
            Policy {
                statements: vec![rule(
                    Effect::Allow,
                    Actions::READ,
                    ResourcePattern::Exact(large_capacity),
                )],
            },
        )],
    };
    check(
        matches!(
            CompiledPolicy::from_snapshot(
                snapshot,
                CompilationLimits {
                    max_resource_bytes: 1,
                    ..CompilationLimits::default()
                }
            ),
            Err(Error::CompilationLimit("resource bytes"))
        ),
        "resource capacity bypassed the memory limit",
    )
}

#[test]
fn long_keys_nested_prefixes_and_shared_buffers() -> TestResult {
    let mut rules = Vec::new();
    for length in 1..=1024 {
        rules.push(rule(
            Effect::Allow,
            Actions::READ,
            ResourcePattern::Prefix("a".repeat(length)),
        ));
    }
    rules.push(rule(
        Effect::Deny,
        Actions::WRITE,
        ResourcePattern::Prefix("a".repeat(512)),
    ));
    let long_key = "a".repeat(65_536);
    rules.push(rule(
        Effect::Allow,
        Actions::WRITE,
        ResourcePattern::Exact(long_key.clone()),
    ));
    for index in 0..10_000 {
        rules.push(rule(
            Effect::Allow,
            Actions::LIST,
            ResourcePattern::Exact(format!("shared/path/{index}")),
        ));
    }
    let compiled = compile(rules)?;
    check(
        compiled.authorize(Action::Read, &long_key) == Decision::Allow,
        "long key lost its prefix permissions",
    )?;
    check(
        compiled.authorize(Action::Write, &long_key) == Decision::Deny,
        "deep exact allow bypassed a parent denial",
    )?;
    check(
        compiled.authorize(Action::List, "shared/path/9999") == Decision::Allow,
        "shared prefix exact rule was lost",
    )?;
    check(
        compiled.authorize(Action::List, "shared/path/10000") == Decision::Deny,
        "shared prefix accidentally became an allow",
    )
}

fn naive(snapshot: &PolicySnapshot, action: Action, key: &str) -> Decision {
    let mut allowed = false;
    for (_, policy) in &snapshot.policies {
        for statement in &policy.statements {
            let matches = match &statement.resource {
                ResourcePattern::Exact(value) => key == value,
                ResourcePattern::Prefix(value) => key.starts_with(value),
            };
            if matches && statement.actions.contains(action) {
                match statement.effect {
                    Effect::Deny => return Decision::Deny,
                    Effect::Allow => allowed = true,
                }
            }
        }
    }
    if allowed {
        Decision::Allow
    } else {
        Decision::Deny
    }
}

/// Reproducible property generation without a new dependency or external I/O.
struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick(&mut self, bound: usize) -> usize {
        usize::try_from(self.next())
            .ok()
            .and_then(|value| value.checked_rem(bound))
            .unwrap_or_default()
    }

    fn key(&mut self) -> TestResult<String> {
        let pieces = ["a", "b", "/", ".", "*", "é", "ê", "任", "意", "\0", "🙂"];
        let mut key = String::new();
        for _ in 0..self.pick(10) {
            key.push_str(
                pieces
                    .get(self.pick(pieces.len()))
                    .ok_or("invalid generator choice")?,
            );
        }
        Ok(key)
    }
}

#[test]
fn generated_policies_match_the_naive_interpreter_in_any_order() -> TestResult {
    let mut generator = Generator(0x7c41_96ab_02f3_8e15);
    for case in 0..1000 {
        let mut keys = vec![String::new(), "photos/".into(), "photos/private/".into()];
        for _ in 0..12 {
            keys.push(generator.key()?);
        }
        let mut policies = Vec::new();
        for id in 0..generator.pick(8) {
            let mut statements = Vec::new();
            for _ in 0..generator.pick(40) {
                let key = keys
                    .get(generator.pick(keys.len()))
                    .ok_or("missing generated key")?
                    .clone();
                let resource = if generator.next() & 1 == 0 {
                    ResourcePattern::Exact(key)
                } else {
                    ResourcePattern::Prefix(key)
                };
                let actions = Actions::ALL
                    .iter()
                    .filter(|_| generator.next() & 1 != 0)
                    .collect();
                let effect = if generator.next() & 1 == 0 {
                    Effect::Allow
                } else {
                    Effect::Deny
                };
                statements.push(rule(effect, actions, resource));
            }
            policies.push((u128::try_from(id)?, Policy { statements }));
        }
        let snapshot = PolicySnapshot {
            revision: Revision(1),
            policies,
        };
        let compiled =
            CompiledPolicy::from_snapshot(snapshot.clone(), CompilationLimits::default())?;
        let mut reversed = snapshot.clone();
        reversed.policies.reverse();
        for (_, policy) in &mut reversed.policies {
            policy.statements.reverse();
        }
        let reversed = CompiledPolicy::from_snapshot(reversed, CompilationLimits::default())?;
        let mut requested = keys.clone();
        requested.extend(keys.iter().map(|key| format!("{key}/child")));
        for _ in 0..12 {
            requested.push(generator.key()?);
        }
        for key in requested {
            for action in Actions::ALL.iter() {
                let expected = naive(&snapshot, action, &key);
                if compiled.authorize(action, &key) != expected
                    || reversed.authorize(action, &key) != expected
                {
                    return Err(format!("generated case {case}, action {action:?}, key {key:?} differs from the reference").into());
                }
            }
        }
    }
    Ok(())
}

#[test]
fn compiled_policies_are_shareable_and_survive_their_creator() -> TestResult {
    fn require_send_sync<T: Send + Sync>(_: &T) {}
    let compiled = Arc::new(compile(vec![
        rule(
            Effect::Allow,
            Actions::READ,
            ResourcePattern::Prefix("photos/".into()),
        ),
        rule(
            Effect::Deny,
            Actions::READ,
            ResourcePattern::Prefix("photos/private/".into()),
        ),
    ])?);
    require_send_sync(&compiled);
    let mut workers = Vec::new();
    for _ in 0..16 {
        let compiled = Arc::clone(&compiled);
        workers.push(std::thread::Builder::new().spawn(move || -> TestResult {
            for _ in 0..1000 {
                check(
                    compiled.authorize(Action::Read, "photos/a") == Decision::Allow,
                    "concurrent read lost an allow",
                )?;
                check(
                    compiled.authorize(Action::Read, "photos/private/a") == Decision::Deny,
                    "concurrent read lost a deny",
                )?;
            }
            Ok(())
        })?);
    }
    drop(compiled);
    for worker in workers {
        worker
            .join()
            .map_err(|_| "authorization worker panicked")??;
    }
    Ok(())
}
