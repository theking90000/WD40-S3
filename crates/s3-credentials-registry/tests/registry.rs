use std::{
    sync::{Arc, Barrier, mpsc},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use futures_executor::block_on;
use s3_credentials_registry::{
    Credential, Error, KeyId, KeySecret, LocalRegistry, S3CredentialRegistry,
    traits::S3CredentialRegistry as _,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

#[test]
fn unicode_lengths_serialization_and_secret_redaction() -> TestResult {
    let id = KeyId::new("🔑".repeat(20))?;
    let secret = KeySecret::new("密".repeat(40))?;
    check(
        id.as_str().len() > 20 && secret.as_str().len() > 40,
        "Unicode counted as bytes",
    )?;
    check(
        KeyId::try_from(id.as_str().to_owned())? == id,
        "TryFrom changed ID",
    )?;
    check(id.as_str().parse::<KeyId>()? == id, "FromStr changed ID")?;
    check(
        secret.as_str().parse::<KeySecret>()? == secret,
        "FromStr changed secret",
    )?;
    for length in [0, 19, 21] {
        check(
            matches!(KeyId::new("🔑".repeat(length)), Err(Error::InvalidKeyId)),
            "invalid ID accepted",
        )?;
        let bytes = postcard::to_allocvec(&"🔑".repeat(length))?;
        check(
            postcard::from_bytes::<KeyId>(&bytes).is_err(),
            "deserialization bypassed ID validation",
        )?;
    }
    for length in [0, 39, 41] {
        check(
            matches!(
                KeySecret::new("密".repeat(length)),
                Err(Error::InvalidKeySecret)
            ),
            "invalid secret accepted",
        )?;
        let bytes = postcard::to_allocvec(&"密".repeat(length))?;
        check(
            postcard::from_bytes::<KeySecret>(&bytes).is_err(),
            "deserialization bypassed secret validation",
        )?;
    }
    let id_bytes = postcard::to_allocvec(&id)?;
    check(
        postcard::from_bytes::<KeyId>(&id_bytes)? == id,
        "ID roundtrip changed value",
    )?;
    let credential = Credential {
        principal_id: u128::MAX,
        secret,
    };
    let bytes = postcard::to_allocvec(&credential)?;
    check(
        postcard::from_bytes::<Credential>(&bytes)? == credential,
        "credential roundtrip changed value",
    )?;
    check(
        !format!("{credential:?}").contains(credential.secret.as_str()),
        "credential Debug leaked secret",
    )?;
    check(
        !format!("{:?}", credential.secret).contains(credential.secret.as_str()),
        "secret Debug leaked secret",
    )?;
    check(
        KeyId::new("!".repeat(20)).is_ok(),
        "ID alphabet was restricted",
    )?;
    check(
        KeySecret::new("!".repeat(40)).is_ok(),
        "secret alphabet was restricted",
    )
}

#[test]
fn lifecycle_duplicate_and_corrupt_records() -> TestResult {
    block_on(async {
        let db = database()?;
        let registry = S3CredentialRegistry::Local(LocalRegistry::new(db.clone())?);
        let id = KeyId::new("e\u{301}".repeat(10))?;
        let distinct = KeyId::new("é".repeat(20))?;
        let credential = Credential {
            principal_id: 0,
            secret: KeySecret::new("密".repeat(40))?,
        };
        check(
            registry.get_key(&id).await?.is_none(),
            "missing key returned a credential",
        )?;
        registry.revoke_key(&id).await?;
        registry
            .register_key(
                id.clone(),
                credential.secret.clone(),
                credential.principal_id,
            )
            .await?;
        check(
            registry.get_key(&id).await? == Some(credential.clone()),
            "credential changed",
        )?;
        check(
            registry.get_key(&distinct).await?.is_none(),
            "ID was normalized",
        )?;
        for replacement in [
            credential.clone(),
            Credential {
                principal_id: u128::MAX,
                secret: KeySecret::new("x".repeat(40))?,
            },
        ] {
            check(
                matches!(
                    registry.register_key(id.clone(), replacement.secret, replacement.principal_id).await,
                    Err(Error::KeyAlreadyExists(existing)) if existing == id
                ),
                "duplicate did not return typed error",
            )?;
        }
        check(
            registry.get_key(&id).await? == Some(credential.clone()),
            "duplicate overwrote credential",
        )?;
        let tree = db.open_tree("s3-credentials")?;
        let mut bytes = postcard::to_allocvec(&credential)?;
        bytes.push(0);
        tree.insert(id.as_str().as_bytes(), bytes)?;
        check(
            matches!(registry.get_key(&id).await, Err(Error::CorruptState(_))),
            "trailing data accepted",
        )?;
        tree.insert(
            id.as_str().as_bytes(),
            postcard::to_allocvec(&(0_u128, "short"))?,
        )?;
        check(
            matches!(registry.get_key(&id).await, Err(Error::Serialization(_))),
            "invalid persisted secret accepted",
        )?;
        tree.insert(id.as_str().as_bytes(), vec![0xff])?;
        check(
            matches!(registry.get_key(&id).await, Err(Error::Serialization(_))),
            "corrupt record accepted",
        )?;
        registry.revoke_key(&id).await?;
        registry.revoke_key(&id).await?;
        check(
            registry.get_key(&id).await?.is_none(),
            "revoked credential remained",
        )?;
        registry
            .register_key(id.clone(), credential.secret.clone(), u128::MAX)
            .await?;
        check(
            registry.get_key(&id).await?.map(|value| value.principal_id) == Some(u128::MAX),
            "re-registration failed",
        )
    })
}

#[test]
fn concurrent_independent_handles_insert_exactly_once() -> TestResult {
    let db = database()?;
    let id = KeyId::new("A".repeat(20))?;
    let barrier = Arc::new(Barrier::new(9));
    let (sender, receiver) = mpsc::channel();
    let mut workers = Vec::new();
    for principal in 0..8_u128 {
        let registry = LocalRegistry::new(db.clone())?;
        let id = id.clone();
        let barrier = barrier.clone();
        let sender = sender.clone();
        workers.push(std::thread::spawn(move || {
            let outcome: TestResult<Option<Credential>> = block_on(async {
                let secret = KeySecret::new(format!("{principal:040}"))?;
                barrier.wait();
                match registry
                    .register_key(id.clone(), secret.clone(), principal)
                    .await
                {
                    Ok(()) => Ok(Some(Credential {
                        principal_id: principal,
                        secret,
                    })),
                    Err(Error::KeyAlreadyExists(existing)) if existing == id => Ok(None),
                    Err(error) => Err(error.into()),
                }
            });
            let _ = sender.send(outcome);
        }));
    }
    barrier.wait();
    let mut winners = Vec::new();
    for _ in 0..8 {
        if let Some(winner) = receiver.recv_timeout(Duration::from_secs(20))?? {
            winners.push(winner);
        }
    }
    for worker in workers {
        worker.join().map_err(|_| "registration worker panicked")?;
    }
    check(
        winners.len() == 1,
        "registration did not have exactly one winner",
    )?;
    let winner = winners.pop().ok_or("missing winner")?;
    let registry = LocalRegistry::new(db)?;
    check(
        block_on(registry.get_key(&id))? == Some(winner),
        "stored credential differs from winner",
    )
}

#[test]
fn reopen_preserves_registration_and_revocation() -> TestResult {
    let unique = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let path =
        std::env::temp_dir().join(format!("wd40-credentials-{}-{unique}", std::process::id()));
    let result: TestResult = block_on(async {
        let id = KeyId::new("A".repeat(20))?;
        let credential = Credential {
            principal_id: u128::MAX,
            secret: KeySecret::new("s".repeat(40))?,
        };
        {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            registry
                .register_key(
                    id.clone(),
                    credential.secret.clone(),
                    credential.principal_id,
                )
                .await?;
        }
        {
            let registry = LocalRegistry::new(sled::open(&path)?)?;
            check(
                registry.get_key(&id).await? == Some(credential),
                "reopen lost registration",
            )?;
            registry.revoke_key(&id).await?;
        }
        let registry = LocalRegistry::new(sled::open(&path)?)?;
        check(
            registry.get_key(&id).await?.is_none(),
            "reopen restored revoked key",
        )
    });
    std::fs::remove_dir_all(path)?;
    result
}

#[test]
fn independent_databases_flush_without_starvation() -> TestResult {
    let barrier = Arc::new(Barrier::new(17));
    let (sender, receiver) = mpsc::channel();
    let mut workers = Vec::new();
    for principal in 0..16_u128 {
        let registry = LocalRegistry::new(database()?)?;
        let barrier = barrier.clone();
        let sender = sender.clone();
        workers.push(std::thread::spawn(move || {
            let outcome: TestResult = block_on(async {
                let id = KeyId::new("A".repeat(20))?;
                let secret = KeySecret::new("s".repeat(40))?;
                barrier.wait();
                registry.register_key(id.clone(), secret, principal).await?;
                registry.revoke_key(&id).await?;
                Ok(())
            });
            let _ = sender.send(outcome);
        }));
    }
    barrier.wait();
    for _ in 0..16 {
        receiver.recv_timeout(Duration::from_secs(20))??;
    }
    for worker in workers {
        worker.join().map_err(|_| "database worker panicked")?;
    }
    Ok(())
}
