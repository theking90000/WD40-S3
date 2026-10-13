use access_controller::{AccessController, Action, Decision, Principal};
use s3_credentials_registry::S3CredentialRegistry;
use s3s::{
    S3Error, S3Request, S3Result,
    access::S3AccessContext,
    auth::Credentials,
    dto::{GetObjectInput, HeadObjectInput},
};

use crate::{auth::get_credential, request};

#[derive(Clone)]
pub struct S3Access {
    pub(crate) registry: S3CredentialRegistry,
    pub(crate) access: AccessController,
}

impl S3Access {
    pub fn new(registry: S3CredentialRegistry, access: AccessController) -> Self {
        Self { registry, access }
    }

    /// Resolves authenticated credentials through the registry, or returns Anonymous.
    pub async fn get_principal(&self, credentials: Option<&Credentials>) -> S3Result<Principal> {
        let Some(credentials) = credentials else {
            return Ok(Principal::Anonymous);
        };
        let credential = get_credential(&self.registry, &credentials.access_key).await?;
        Ok(Principal::Authenticated(credential.principal_id))
    }

    /// Returns AccessDenied for a denial and InternalError for a technical failure.
    pub async fn authorize(
        &self,
        credentials: Option<&Credentials>,
        key: &str,
        action: Action,
    ) -> S3Result<()> {
        let principal = self.get_principal(credentials).await?;
        match self
            .access
            .authorize(principal, action, key)
            .await
            .map_err(S3Error::internal_error)?
        {
            Decision::Allow => Ok(()),
            Decision::Deny => Err(s3s::s3_error!(AccessDenied)),
        }
    }
}

#[async_trait::async_trait]
impl s3s::access::S3Access for S3Access {
    async fn check(&self, _cx: &mut S3AccessContext<'_>) -> S3Result<()> {
        Ok(())
    }

    async fn get_object(&self, req: &mut S3Request<GetObjectInput>) -> S3Result<()> {
        request::get_object::get_object_access(self, req).await
    }

    async fn head_object(&self, req: &mut S3Request<HeadObjectInput>) -> S3Result<()> {
        request::head_object::head_object_access(self, req).await
    }
}
