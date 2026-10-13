use s3s::{S3Request, S3Result, dto::HeadObjectInput};

use crate::{S3Adapter, request};

#[async_trait::async_trait]
impl s3s::access::S3Access for S3Adapter {
    async fn head_object(&self, req: &mut S3Request<HeadObjectInput>) -> S3Result<()> {
        request::head_object::head_object_access(self, req).await
    }
}
