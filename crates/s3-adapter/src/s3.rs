use s3s::{
    S3Request, S3Response, S3Result,
    dto::{
        CreateMultipartUploadInput, CreateMultipartUploadOutput, GetObjectInput, GetObjectOutput,
        HeadObjectInput, HeadObjectOutput, ListObjectsInput, ListObjectsOutput, ListObjectsV2Input,
        ListObjectsV2Output, PutObjectInput, PutObjectOutput,
    },
};

use crate::{S3Adapter, request};

#[async_trait::async_trait]
impl s3s::S3 for S3Adapter {
    async fn create_multipart_upload(
        &self,
        req: S3Request<CreateMultipartUploadInput>,
    ) -> S3Result<S3Response<CreateMultipartUploadOutput>> {
        request::create_multipart_upload::create_multipart_upload(self, req).await
    }

    async fn put_object(
        &self,
        req: S3Request<PutObjectInput>,
    ) -> S3Result<S3Response<PutObjectOutput>> {
        request::put_object::put_object(self, req).await
    }

    async fn list_objects(
        &self,
        req: S3Request<ListObjectsInput>,
    ) -> S3Result<S3Response<ListObjectsOutput>> {
        request::list_objects::list_objects(self, req).await
    }

    async fn list_objects_v2(
        &self,
        req: S3Request<ListObjectsV2Input>,
    ) -> S3Result<S3Response<ListObjectsV2Output>> {
        request::list_objects_v2::list_objects_v2(self, req).await
    }

    async fn get_object(
        &self,
        req: S3Request<GetObjectInput>,
    ) -> S3Result<S3Response<GetObjectOutput>> {
        request::get_object::get_object(self, req).await
    }

    async fn head_object(
        &self,
        req: S3Request<HeadObjectInput>,
    ) -> S3Result<S3Response<HeadObjectOutput>> {
        request::head_object::head_object(self, req).await
    }
}
