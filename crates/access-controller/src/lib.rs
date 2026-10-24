//! Authorization over coherent policy-registry snapshots.
//!
//! The caller supplies an already authenticated principal and enforces the
//! returned decision before performing an operation. Compiled policies are
//! immutable and retain their loading revision; they do not refresh themselves.

mod compiled;
mod error;

use std::sync::Arc;

use policy_registry::{PolicySnapshot, PrincipalSelector, traits::PolicyRegistry as _};

pub use compiled::CompiledPolicy;
pub use error::Error;
pub use error::Error as AccessError;
pub use policy_registry::{Action, Actions, PolicyRegistry, Revision};

/// Authentication and identity existence are the caller's responsibility.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Principal {
    Anonymous,
    Authenticated(u128),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Decision {
    Allow,
    Deny,
}

/// Bounds compilation work and index size, after the registry has loaded data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CompilationLimits {
    pub max_statements: usize,
    /// Sum of resource string capacities, including repetitions and empty rules.
    pub max_resource_bytes: usize,
    /// Includes the root, so zero cannot compile even an empty snapshot.
    pub max_nodes: usize,
}

impl Default for CompilationLimits {
    fn default() -> Self {
        Self {
            max_statements: 100_000,
            max_resource_bytes: 16 * 1024 * 1024,
            max_nodes: 200_001,
        }
    }
}

/// Owned, cloneable policy registry handle, selected through the Local/Remote enum.
/// Construction performs no I/O and creates no tasks or cache.
#[derive(Clone)]
pub struct AccessController {
    registry: PolicyRegistry,
    limits: CompilationLimits,
}

impl AccessController {
    pub fn new(registry: PolicyRegistry) -> Self {
        Self::with_limits(registry, CompilationLimits::default())
    }

    pub fn with_limits(registry: PolicyRegistry, limits: CompilationLimits) -> Self {
        Self { registry, limits }
    }

    /// Loads one fresh snapshot and evaluates a single key. A technical error
    /// is returned to the caller and never converted into an Allow decision.
    pub async fn authorize(
        &self,
        principal: Principal,
        action: Action,
        key: &str,
    ) -> Result<Decision, Error> {
        Ok(self.compile(principal).await?.authorize(action, key))
    }

    /// Loads all applicable selectors in one coherent registry read.
    /// The returned snapshot can outlive this controller and be shared between
    /// threads. Later policy changes do not modify or revoke this value.
    pub async fn compile(&self, principal: Principal) -> Result<Arc<CompiledPolicy>, Error> {
        self.compile_snapshot(self.load_policies(principal).await?)
    }

    /// Loads the policies for a principal, including applicable global selectors.
    pub async fn load_policies(&self, principal: Principal) -> Result<PolicySnapshot, Error> {
        Ok(match principal {
            Principal::Anonymous => {
                self.registry
                    .load_policies(&[PrincipalSelector::Everyone])
                    .await?
            }
            Principal::Authenticated(id) => {
                self.registry
                    .load_policies(&[
                        PrincipalSelector::Everyone,
                        PrincipalSelector::Authenticated,
                        PrincipalSelector::Specific(id),
                    ])
                    .await?
            }
        })
    }

    /// Loads only policies directly attached to a principal, excluding global selectors.
    pub async fn load_principal_policies(
        &self,
        principal_id: u128,
    ) -> Result<PolicySnapshot, Error> {
        Ok(self
            .registry
            .load_policies(&[PrincipalSelector::Specific(principal_id)])
            .await?)
    }

    /// Compiles an already loaded snapshot using this controller's limits.
    pub fn compile_snapshot(&self, snapshot: PolicySnapshot) -> Result<Arc<CompiledPolicy>, Error> {
        Ok(Arc::new(CompiledPolicy::from_snapshot(
            snapshot,
            self.limits,
        )?))
    }
}
