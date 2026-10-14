use s3s::{
    S3Request, S3Response, S3Result,
    dto::{
        CompleteMultipartUploadInput, CompleteMultipartUploadOutput, CopyObjectInput,
        CopyObjectOutput, CreateMultipartUploadInput, CreateMultipartUploadOutput,
        DeleteObjectInput, DeleteObjectOutput, DeleteObjectsInput, DeleteObjectsOutput,
        GetObjectInput, GetObjectOutput, HeadObjectInput, HeadObjectOutput,
        ListMultipartUploadsInput, ListMultipartUploadsOutput, ListObjectsInput, ListObjectsOutput,
        ListObjectsV2Input, ListObjectsV2Output, ListPartsInput, ListPartsOutput, PutObjectInput,
        PutObjectOutput, UploadPartInput, UploadPartOutput,
    },
};

use crate::{S3Adapter, request};

#[async_trait::async_trait]
impl s3s::S3 for S3Adapter {
    async fn complete_multipart_upload(
        &self,
        req: S3Request<CompleteMultipartUploadInput>,
    ) -> S3Result<S3Response<CompleteMultipartUploadOutput>> {
        request::complete_multipart_upload::complete_multipart_upload(self, req).await
    }

    async fn copy_object(
        &self,
        req: S3Request<CopyObjectInput>,
    ) -> S3Result<S3Response<CopyObjectOutput>> {
        request::copy_object::copy_object(self, req).await
    }

    async fn delete_objects(
        &self,
        req: S3Request<DeleteObjectsInput>,
    ) -> S3Result<S3Response<DeleteObjectsOutput>> {
        request::delete_objects::delete_objects(self, req).await
    }

    async fn delete_object(
        &self,
        req: S3Request<DeleteObjectInput>,
    ) -> S3Result<S3Response<DeleteObjectOutput>> {
        request::delete_object::delete_object(self, req).await
    }

    async fn upload_part(
        &self,
        req: S3Request<UploadPartInput>,
    ) -> S3Result<S3Response<UploadPartOutput>> {
        request::upload_part::upload_part(self, req).await
    }

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

    async fn list_multipart_uploads(
        &self,
        req: S3Request<ListMultipartUploadsInput>,
    ) -> S3Result<S3Response<ListMultipartUploadsOutput>> {
        request::list_multipart_uploads::list_multipart_uploads(self, req).await
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

    async fn list_parts(
        &self,
        req: S3Request<ListPartsInput>,
    ) -> S3Result<S3Response<ListPartsOutput>> {
        request::list_parts::list_parts(self, req).await
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
