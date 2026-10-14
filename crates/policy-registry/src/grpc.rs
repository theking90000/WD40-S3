//! Protocol v1 and a service delegating to the injected registry enum.
use prost::Message;
use tonic::{Code, Status};

use crate::{
    Action, Actions, Effect, Error, Policy, PrincipalSelector, ResourcePattern, Statement,
};

pub mod proto {
    tonic::include_proto!("wd40.policy_registry.v1");
}

pub const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn decode_id(bytes: &[u8]) -> Result<u128, Status> {
    let bytes: [u8; 16] = bytes
        .try_into()
        .map_err(|_| Status::invalid_argument("registry IDs must contain exactly 16 bytes"))?;
    Ok(u128::from_be_bytes(bytes))
}

pub(crate) fn encode_policy(policy: Policy) -> proto::Policy {
    proto::Policy {
        statements: policy
            .statements
            .into_iter()
            .map(|statement| {
                let effect = (match statement.effect {
                    Effect::Allow => proto::Effect::Allow,
                    Effect::Deny => proto::Effect::Deny,
                }) as i32;
                let actions = statement
                    .actions
                    .iter()
                    .map(|action| {
                        (match action {
                            Action::Read => proto::Action::Read,
                            Action::Write => proto::Action::Write,
                            Action::Delete => proto::Action::Delete,
                            Action::List => proto::Action::List,
                        }) as i32
                    })
                    .collect();
                let pattern = match statement.resource {
                    ResourcePattern::Exact(value) => proto::resource_pattern::Pattern::Exact(value),
                    ResourcePattern::Prefix(value) => {
                        proto::resource_pattern::Pattern::Prefix(value)
                    }
                };
                proto::Statement {
                    effect,
                    actions,
                    resource: Some(proto::ResourcePattern {
                        pattern: Some(pattern),
                    }),
                }
            })
            .collect(),
    }
}

pub(crate) fn decode_policy(policy: proto::Policy) -> Result<Policy, Status> {
    let statements = policy
        .statements
        .into_iter()
        .map(|statement| {
            let effect = match proto::Effect::try_from(statement.effect) {
                Ok(proto::Effect::Allow) => Effect::Allow,
                Ok(proto::Effect::Deny) => Effect::Deny,
                _ => return Err(Status::invalid_argument("invalid policy effect")),
            };
            let actions = statement
                .actions
                .into_iter()
                .map(|action| match proto::Action::try_from(action) {
                    Ok(proto::Action::Read) => Ok(Action::Read),
                    Ok(proto::Action::Write) => Ok(Action::Write),
                    Ok(proto::Action::Delete) => Ok(Action::Delete),
                    Ok(proto::Action::List) => Ok(Action::List),
                    _ => Err(Status::invalid_argument("invalid policy action")),
                })
                .collect::<Result<Actions, _>>()?;
            let resource = match statement.resource.and_then(|resource| resource.pattern) {
                Some(proto::resource_pattern::Pattern::Exact(value)) => {
                    ResourcePattern::Exact(value)
                }
                Some(proto::resource_pattern::Pattern::Prefix(value)) => {
                    ResourcePattern::Prefix(value)
                }
                None => return Err(Status::invalid_argument("missing resource pattern")),
            };
            Ok(Statement {
                effect,
                actions,
                resource,
            })
        })
        .collect::<Result<Vec<_>, Status>>()?;
    Ok(Policy { statements })
}

#[cfg(feature = "grpc-client")]
pub(crate) fn encode_selector(selector: PrincipalSelector) -> proto::PrincipalSelector {
    use proto::principal_selector::Selector;
    let selector = match selector {
        PrincipalSelector::Everyone => Selector::Everyone(proto::Empty {}),
        PrincipalSelector::Authenticated => Selector::Authenticated(proto::Empty {}),
        PrincipalSelector::Specific(id) => Selector::Specific(id.to_be_bytes().to_vec()),
    };
    proto::PrincipalSelector {
        selector: Some(selector),
    }
}

#[cfg(feature = "grpc-server")]
fn decode_selector(selector: proto::PrincipalSelector) -> Result<PrincipalSelector, Status> {
    use proto::principal_selector::Selector;
    match selector.selector {
        Some(Selector::Everyone(_)) => Ok(PrincipalSelector::Everyone),
        Some(Selector::Authenticated(_)) => Ok(PrincipalSelector::Authenticated),
        Some(Selector::Specific(bytes)) => Ok(PrincipalSelector::Specific(decode_id(&bytes)?)),
        None => Err(Status::invalid_argument("missing principal selector")),
    }
}

#[cfg(feature = "grpc-server")]
pub(crate) fn into_status(error: Error) -> Status {
    use proto::registry_error::Kind;
    let message = error.to_string();
    let (code, details) = match error {
        Error::PolicyNotFound(id) => (
            Code::NotFound,
            proto::RegistryError {
                kind: Kind::PolicyNotFound as i32,
                policy_id: id.to_be_bytes().to_vec(),
            },
        ),
        Error::RevisionOverflow => (
            Code::ResourceExhausted,
            proto::RegistryError {
                kind: Kind::RevisionOverflow as i32,
                ..Default::default()
            },
        ),
        Error::Rpc(status) => return status,
        #[cfg(feature = "grpc-client")]
        Error::Transport(_) => return Status::unavailable("upstream registry transport failed"),
        _ => return Status::internal("registry storage operation failed"),
    };
    Status::with_details(code, message, details.encode_to_vec().into())
}

#[cfg(feature = "grpc-client")]
pub(crate) fn from_status(status: Status) -> Error {
    use proto::registry_error::Kind;
    let decoded = (|| -> Option<Error> {
        let details = proto::RegistryError::decode(status.details()).ok()?;
        Some(match (status.code(), Kind::try_from(details.kind).ok()?) {
            (Code::NotFound, Kind::PolicyNotFound) => {
                Error::PolicyNotFound(decode_id(&details.policy_id).ok()?)
            }
            (Code::ResourceExhausted, Kind::RevisionOverflow) => Error::RevisionOverflow,
            _ => return None,
        })
    })();
    match decoded {
        Some(error) => error,
        None => Error::Rpc(status),
    }
}

#[cfg(feature = "grpc-server")]
#[derive(Clone)]
pub struct GrpcService {
    registry: crate::PolicyRegistry,
}

#[cfg(feature = "grpc-server")]
impl GrpcService {
    pub fn new(registry: crate::PolicyRegistry) -> Self {
        Self { registry }
    }

    pub fn into_server(self) -> proto::policy_registry_server::PolicyRegistryServer<Self> {
        proto::policy_registry_server::PolicyRegistryServer::new(self)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE)
    }
}

#[cfg(feature = "grpc-server")]
#[tonic::async_trait]
impl proto::policy_registry_server::PolicyRegistry for GrpcService {
    async fn create_policy(
        &self,
        request: tonic::Request<proto::CreatePolicyRequest>,
    ) -> Result<tonic::Response<proto::CreatePolicyResponse>, Status> {
        use crate::traits::PolicyRegistry as _;
        let policy = decode_policy(
            request
                .into_inner()
                .policy
                .ok_or_else(|| Status::invalid_argument("missing policy"))?,
        )?;
        let id = self
            .registry
            .create_policy(policy)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::CreatePolicyResponse {
            id: id.to_be_bytes().to_vec(),
        }))
    }

    async fn get_policy(
        &self,
        request: tonic::Request<proto::PolicyRequest>,
    ) -> Result<tonic::Response<proto::GetPolicyResponse>, Status> {
        use crate::traits::PolicyRegistry as _;
        let id = decode_id(&request.into_inner().id)?;
        let policy = self.registry.get_policy(id).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::GetPolicyResponse {
            policy: policy.map(encode_policy),
        }))
    }

    async fn put_policy(
        &self,
        request: tonic::Request<proto::PutPolicyRequest>,
    ) -> Result<tonic::Response<proto::RevisionResponse>, Status> {
        use crate::traits::PolicyRegistry as _;
        let request = request.into_inner();
        let id = decode_id(&request.id)?;
        let policy = decode_policy(
            request
                .policy
                .ok_or_else(|| Status::invalid_argument("missing policy"))?,
        )?;
        let revision = self
            .registry
            .put_policy(id, policy)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::RevisionResponse {
            revision: revision.0,
        }))
    }

    async fn delete_policy(
        &self,
        request: tonic::Request<proto::PolicyRequest>,
    ) -> Result<tonic::Response<proto::RevisionResponse>, Status> {
        use crate::traits::PolicyRegistry as _;
        let id = decode_id(&request.into_inner().id)?;
        let revision = self.registry.delete_policy(id).await.map_err(into_status)?;
        Ok(tonic::Response::new(proto::RevisionResponse {
            revision: revision.0,
        }))
    }

    async fn attach(
        &self,
        request: tonic::Request<proto::BindingRequest>,
    ) -> Result<tonic::Response<proto::RevisionResponse>, Status> {
        use crate::traits::PolicyRegistry as _;
        let request = request.into_inner();
        let selector = decode_selector(
            request
                .selector
                .ok_or_else(|| Status::invalid_argument("missing principal selector"))?,
        )?;
        let id = decode_id(&request.policy_id)?;
        let revision = self
            .registry
            .attach(selector, id)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::RevisionResponse {
            revision: revision.0,
        }))
    }

    async fn detach(
        &self,
        request: tonic::Request<proto::BindingRequest>,
    ) -> Result<tonic::Response<proto::RevisionResponse>, Status> {
        use crate::traits::PolicyRegistry as _;
        let request = request.into_inner();
        let selector = decode_selector(
            request
                .selector
                .ok_or_else(|| Status::invalid_argument("missing principal selector"))?,
        )?;
        let id = decode_id(&request.policy_id)?;
        let revision = self
            .registry
            .detach(selector, id)
            .await
            .map_err(into_status)?;
        Ok(tonic::Response::new(proto::RevisionResponse {
            revision: revision.0,
        }))
    }

    async fn load_policies(
        &self,
        request: tonic::Request<proto::LoadPoliciesRequest>,
    ) -> Result<tonic::Response<proto::PolicySnapshot>, Status> {
        use crate::traits::PolicyRegistry as _;
        let selectors = request
            .into_inner()
            .selectors
            .into_iter()
            .map(decode_selector)
            .collect::<Result<Vec<_>, _>>()?;
        let snapshot = self
            .registry
            .load_policies(&selectors)
            .await
            .map_err(into_status)?;
        let policies = snapshot
            .policies
            .into_iter()
            .map(|(id, policy)| proto::PolicyEntry {
                id: id.to_be_bytes().to_vec(),
                policy: Some(encode_policy(policy)),
            })
            .collect();
        Ok(tonic::Response::new(proto::PolicySnapshot {
            revision: snapshot.revision.0,
            policies,
        }))
    }
}
