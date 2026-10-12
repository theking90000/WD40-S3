use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::Error;

pub type PolicyId = u128;
pub type Actions = Vec<Action>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Read,
    Write,
    Delete,
    List,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Policy {
    pub statements: Vec<Statement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Statement {
    pub effect: Effect,
    pub actions: Actions,
    pub resource: ResourcePattern,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ResourcePattern {
    Exact(String),
    Prefix(String),
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum PrincipalSelector {
    Everyone,
    Authenticated,
    Specific(u128),
}

/// Global registry revision, starting at zero and incremented on state changes.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct Revision(pub u64);

/// Policies and revision from one consistent read, ordered by ID without duplicates.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PolicySnapshot {
    pub revision: Revision,
    pub policies: Vec<(PolicyId, Policy)>,
}

pub trait PolicyRegistry {
    fn get_policy(
        &self,
        id: PolicyId,
    ) -> impl Future<Output = Result<Option<Policy>, Error>> + Send;

    /// Creates or replaces a policy. Identical replacements retain the revision.
    fn put_policy(
        &self,
        id: PolicyId,
        policy: Policy,
    ) -> impl Future<Output = Result<Revision, Error>> + Send;

    /// Removes a policy and its bindings. Missing policies retain the revision.
    fn delete_policy(&self, id: PolicyId) -> impl Future<Output = Result<Revision, Error>> + Send;

    /// Attaches an existing policy. Repeated attachments retain the revision.
    fn attach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> impl Future<Output = Result<Revision, Error>> + Send;

    /// Removes a binding. Missing bindings retain the revision.
    fn detach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> impl Future<Output = Result<Revision, Error>> + Send;

    /// Loads the union of policies for exactly the supplied selectors.
    /// Everyone and Authenticated must be supplied explicitly by the caller.
    fn load_policies(
        &self,
        selectors: &[PrincipalSelector],
    ) -> impl Future<Output = Result<PolicySnapshot, Error>> + Send;
}
