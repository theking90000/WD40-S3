use s3s::{
    S3Request, S3Response, S3Result,
    dto::{HeadObjectInput, HeadObjectOutput},
};

use crate::{S3Adapter, request};

#[async_trait::async_trait]
impl s3s::S3 for S3Adapter {
    async fn head_object(
        &self,
        req: S3Request<HeadObjectInput>,
    ) -> S3Result<S3Response<HeadObjectOutput>> {
        request::head_object::head_object(self, req).await
    }
}
