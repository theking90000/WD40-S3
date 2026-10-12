//! Persistent policies and principal bindings with a global snapshot revision.

#[cfg(feature = "cli")]
pub mod cli;
pub mod error;
#[cfg(any(feature = "grpc-client", feature = "grpc-server"))]
pub mod grpc;
pub mod local;
#[cfg(feature = "grpc-client")]
pub mod remote;
pub mod traits;

pub use error::Error;
pub use error::Error as RegistryError;
#[cfg(feature = "grpc-server")]
pub use grpc::GrpcService;
pub use local::LocalRegistry;
#[cfg(feature = "grpc-client")]
pub use remote::RemoteRegistry;
pub use traits::{
    Action, Actions, Effect, Policy, PolicyId, PolicySnapshot, PrincipalSelector, ResourcePattern,
    Revision, Statement,
};

/// Selects the policy registry implementation at runtime.
#[derive(Clone)]
pub enum PolicyRegistry {
    Local(LocalRegistry),
    #[cfg(feature = "grpc-client")]
    Remote(RemoteRegistry),
}

impl traits::PolicyRegistry for PolicyRegistry {
    async fn get_policy(&self, id: PolicyId) -> Result<Option<Policy>, Error> {
        match self {
            Self::Local(registry) => traits::PolicyRegistry::get_policy(registry, id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::PolicyRegistry::get_policy(registry, id).await,
        }
    }

    async fn put_policy(&self, id: PolicyId, policy: Policy) -> Result<Revision, Error> {
        match self {
            Self::Local(registry) => traits::PolicyRegistry::put_policy(registry, id, policy).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::PolicyRegistry::put_policy(registry, id, policy).await
            }
        }
    }

    async fn delete_policy(&self, id: PolicyId) -> Result<Revision, Error> {
        match self {
            Self::Local(registry) => traits::PolicyRegistry::delete_policy(registry, id).await,
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => traits::PolicyRegistry::delete_policy(registry, id).await,
        }
    }

    async fn attach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> Result<Revision, Error> {
        match self {
            Self::Local(registry) => {
                traits::PolicyRegistry::attach(registry, selector, policy).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::PolicyRegistry::attach(registry, selector, policy).await
            }
        }
    }

    async fn detach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> Result<Revision, Error> {
        match self {
            Self::Local(registry) => {
                traits::PolicyRegistry::detach(registry, selector, policy).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::PolicyRegistry::detach(registry, selector, policy).await
            }
        }
    }

    async fn load_policies(
        &self,
        selectors: &[PrincipalSelector],
    ) -> Result<PolicySnapshot, Error> {
        match self {
            Self::Local(registry) => {
                traits::PolicyRegistry::load_policies(registry, selectors).await
            }
            #[cfg(feature = "grpc-client")]
            Self::Remote(registry) => {
                traits::PolicyRegistry::load_policies(registry, selectors).await
            }
        }
    }
}
