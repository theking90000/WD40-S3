use access_controller::{AccessController, Action, Decision, Principal};
use s3_credentials_registry::S3CredentialRegistry;
use s3s::{
    S3Error, S3Request, S3Result,
    access::S3AccessContext,
    auth::Credentials,
    dto::{
        AbortMultipartUploadInput, CompleteMultipartUploadInput, CopyObjectInput,
        CreateMultipartUploadInput, DeleteObjectInput, DeleteObjectsInput, GetObjectInput,
        HeadObjectInput, ListBucketsInput, ListMultipartUploadsInput, ListObjectsInput,
        ListObjectsV2Input, ListPartsInput, PutObjectInput, UploadPartInput,
    },
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
    async fn abort_multipart_upload(
        &self,
        req: &mut S3Request<AbortMultipartUploadInput>,
    ) -> S3Result<()> {
        request::abort_multipart_upload::abort_multipart_upload_access(self, req).await
    }

    async fn complete_multipart_upload(
        &self,
        req: &mut S3Request<CompleteMultipartUploadInput>,
    ) -> S3Result<()> {
        request::complete_multipart_upload::complete_multipart_upload_access(self, req).await
    }

    async fn copy_object(&self, req: &mut S3Request<CopyObjectInput>) -> S3Result<()> {
        request::copy_object::copy_object_access(self, req).await
    }

    async fn delete_objects(&self, req: &mut S3Request<DeleteObjectsInput>) -> S3Result<()> {
        request::delete_objects::delete_objects_access(self, req).await
    }

    async fn delete_object(&self, req: &mut S3Request<DeleteObjectInput>) -> S3Result<()> {
        request::delete_object::delete_object_access(self, req).await
    }

    async fn upload_part(&self, req: &mut S3Request<UploadPartInput>) -> S3Result<()> {
        request::upload_part::upload_part_access(self, req).await
    }

    async fn create_multipart_upload(
        &self,
        req: &mut S3Request<CreateMultipartUploadInput>,
    ) -> S3Result<()> {
        request::create_multipart_upload::create_multipart_upload_access(self, req).await
    }

    async fn put_object(&self, req: &mut S3Request<PutObjectInput>) -> S3Result<()> {
        request::put_object::put_object_access(self, req).await
    }

    async fn list_multipart_uploads(
        &self,
        req: &mut S3Request<ListMultipartUploadsInput>,
    ) -> S3Result<()> {
        request::list_multipart_uploads::list_multipart_uploads_access(self, req).await
    }

    async fn list_objects(&self, req: &mut S3Request<ListObjectsInput>) -> S3Result<()> {
        request::list_objects::list_objects_access(self, req).await
    }

    async fn list_buckets(&self, req: &mut S3Request<ListBucketsInput>) -> S3Result<()> {
        request::list_buckets::list_buckets_access(self, req).await
    }

    async fn list_objects_v2(&self, req: &mut S3Request<ListObjectsV2Input>) -> S3Result<()> {
        request::list_objects_v2::list_objects_v2_access(self, req).await
    }

    async fn list_parts(&self, req: &mut S3Request<ListPartsInput>) -> S3Result<()> {
        request::list_parts::list_parts_access(self, req).await
    }

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
