use std::{fmt, future::Future, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::Error;

pub type PrincipalId = u128;

/// An identifier containing exactly 20 Unicode scalar values, without normalization.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct KeyId(String);

impl KeyId {
    pub fn new(value: String) -> Result<Self, Error> {
        if value.chars().count() != 20 {
            return Err(Error::InvalidKeyId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for KeyId {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        Self::new(value)
    }
}

impl FromStr for KeyId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Error> {
        Self::new(value.to_owned())
    }
}

/// A secret containing exactly 40 Unicode scalar values. Debug never prints it.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String")]
pub struct KeySecret(String);

impl KeySecret {
    pub fn new(value: String) -> Result<Self, Error> {
        if value.chars().count() != 40 {
            return Err(Error::InvalidKeySecret);
        }
        Ok(Self(value))
    }

    /// Exposes the secret explicitly for serialization or signing.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for KeySecret {
    type Error = Error;

    fn try_from(value: String) -> Result<Self, Error> {
        Self::new(value)
    }
}

impl FromStr for KeySecret {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self, Error> {
        Self::new(value.to_owned())
    }
}

impl fmt::Debug for KeySecret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("KeySecret([REDACTED])")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Credential {
    pub principal_id: PrincipalId,
    pub secret: KeySecret,
}

pub trait S3CredentialRegistry {
    /// Inserts a new key. Every existing identifier is rejected, even if identical.
    fn register_key(
        &self,
        id: KeyId,
        secret: KeySecret,
        principal: PrincipalId,
    ) -> impl Future<Output = Result<(), Error>> + Send;

    fn get_key(&self, id: &KeyId)
    -> impl Future<Output = Result<Option<Credential>, Error>> + Send;

    /// Lists the principal's key IDs in lexicographic order, without secrets.
    fn list_keys(
        &self,
        principal: PrincipalId,
    ) -> impl Future<Output = Result<Vec<KeyId>, Error>> + Send;

    /// Removes the record. Revoking an absent key succeeds.
    fn revoke_key(&self, id: &KeyId) -> impl Future<Output = Result<(), Error>> + Send;
}
