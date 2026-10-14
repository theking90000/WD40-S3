//! Connected gRPC client. Failed operations are not replayed.
use tonic::{Status, transport::Endpoint};

use crate::{
    Error, Policy, PolicyId, PolicySnapshot, PrincipalSelector, Revision,
    grpc::{self, proto},
    traits::PolicyRegistry,
};

/// Clones share their HTTP/2 channel without an application mutex.
#[derive(Clone)]
pub struct RemoteRegistry {
    client: proto::policy_registry_client::PolicyRegistryClient<tonic::transport::Channel>,
}

impl RemoteRegistry {
    /// Connects to the caller's endpoint. Requires a Tokio runtime.
    pub async fn connect(endpoint: Endpoint) -> Result<Self, Error> {
        match endpoint.uri().scheme_str() {
            Some("http") => {}
            Some("https") => {
                #[cfg(not(feature = "grpc-tls"))]
                return Err(Error::Rpc(Status::invalid_argument(
                    "HTTPS requires grpc-tls",
                )));
            }
            _ => {
                return Err(Error::Rpc(Status::invalid_argument(
                    "registry endpoint requires http or https",
                )));
            }
        }
        let channel = endpoint.connect().await.map_err(Error::Transport)?;
        Ok(Self {
            client: proto::policy_registry_client::PolicyRegistryClient::new(channel)
                .max_decoding_message_size(grpc::MAX_MESSAGE_SIZE)
                .max_encoding_message_size(grpc::MAX_MESSAGE_SIZE),
        })
    }
}

impl PolicyRegistry for RemoteRegistry {
    async fn create_policy(&self, policy: Policy) -> Result<PolicyId, Error> {
        let response = self
            .client
            .clone()
            .create_policy(proto::CreatePolicyRequest {
                policy: Some(grpc::encode_policy(policy)),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        grpc::decode_id(&response.id)
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))
    }

    async fn get_policy(&self, id: PolicyId) -> Result<Option<Policy>, Error> {
        let response = self
            .client
            .clone()
            .get_policy(proto::PolicyRequest {
                id: id.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        response
            .policy
            .map(grpc::decode_policy)
            .transpose()
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))
    }

    async fn put_policy(&self, id: PolicyId, policy: Policy) -> Result<Revision, Error> {
        let response = self
            .client
            .clone()
            .put_policy(proto::PutPolicyRequest {
                id: id.to_be_bytes().to_vec(),
                policy: Some(grpc::encode_policy(policy)),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Revision(response.revision))
    }

    async fn delete_policy(&self, id: PolicyId) -> Result<Revision, Error> {
        let response = self
            .client
            .clone()
            .delete_policy(proto::PolicyRequest {
                id: id.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Revision(response.revision))
    }

    async fn attach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> Result<Revision, Error> {
        let response = self
            .client
            .clone()
            .attach(proto::BindingRequest {
                selector: Some(grpc::encode_selector(selector)),
                policy_id: policy.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Revision(response.revision))
    }

    async fn detach(
        &self,
        selector: PrincipalSelector,
        policy: PolicyId,
    ) -> Result<Revision, Error> {
        let response = self
            .client
            .clone()
            .detach(proto::BindingRequest {
                selector: Some(grpc::encode_selector(selector)),
                policy_id: policy.to_be_bytes().to_vec(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        Ok(Revision(response.revision))
    }

    async fn load_policies(
        &self,
        selectors: &[PrincipalSelector],
    ) -> Result<PolicySnapshot, Error> {
        let response = self
            .client
            .clone()
            .load_policies(proto::LoadPoliciesRequest {
                selectors: selectors
                    .iter()
                    .copied()
                    .map(grpc::encode_selector)
                    .collect(),
            })
            .await
            .map_err(grpc::from_status)?
            .into_inner();
        let policies = response
            .policies
            .into_iter()
            .map(|entry| {
                let id = grpc::decode_id(&entry.id)?;
                let policy = entry
                    .policy
                    .ok_or_else(|| Status::invalid_argument("missing snapshot policy"))?;
                Ok((id, grpc::decode_policy(policy)?))
            })
            .collect::<Result<Vec<_>, Status>>()
            .map_err(|error| Error::Rpc(Status::data_loss(error.message())))?;
        Ok(PolicySnapshot {
            revision: Revision(response.revision),
            policies,
        })
    }
}
