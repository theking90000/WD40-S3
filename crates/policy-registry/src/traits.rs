use std::{
    fmt,
    future::Future,
    ops::{BitOr, BitOrAssign},
};

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{SeqAccess, Visitor},
    ser::SerializeSeq,
};

use crate::Error;

pub type PolicyId = u128;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Read,
    Write,
    Delete,
    List,
}

/// An action set stored as a bitmask. Serde retains the action-list format.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Actions(u8);

impl Actions {
    pub const EMPTY: Self = Self(0);
    pub const READ: Self = Self(0b0001);
    pub const WRITE: Self = Self(0b0010);
    pub const DELETE: Self = Self(0b0100);
    pub const LIST: Self = Self(0b1000);
    pub const ALL: Self = Self(0b1111);

    const fn action_bits(action: Action) -> u8 {
        match action {
            Action::Read => Self::READ.0,
            Action::Write => Self::WRITE.0,
            Action::Delete => Self::DELETE.0,
            Action::List => Self::LIST.0,
        }
    }

    pub const fn contains(self, action: Action) -> bool {
        self.0 & Self::action_bits(action) != 0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Iterates once per action, in Read, Write, Delete, List order.
    pub fn iter(self) -> impl Iterator<Item = Action> {
        [Action::Read, Action::Write, Action::Delete, Action::List]
            .into_iter()
            .filter(move |&action| self.contains(action))
    }
}

impl From<Action> for Actions {
    fn from(action: Action) -> Self {
        Self(Self::action_bits(action))
    }
}

impl FromIterator<Action> for Actions {
    fn from_iter<T: IntoIterator<Item = Action>>(iter: T) -> Self {
        iter.into_iter()
            .fold(Self::EMPTY, |actions, action| actions | Self::from(action))
    }
}

impl BitOr for Actions {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for Actions {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl Serialize for Actions {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.iter().count()))?;
        for action in self.iter() {
            sequence.serialize_element(&action)?;
        }
        sequence.end()
    }
}

impl<'de> Deserialize<'de> for Actions {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ActionsVisitor;

        impl<'de> Visitor<'de> for ActionsVisitor {
            type Value = Actions;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a sequence of policy actions")
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Actions, A::Error> {
                let mut actions = Actions::EMPTY;
                while let Some(action) = sequence.next_element::<Action>()? {
                    actions |= Actions::from(action);
                }
                Ok(actions)
            }
        }

        deserializer.deserialize_seq(ActionsVisitor)
    }
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
