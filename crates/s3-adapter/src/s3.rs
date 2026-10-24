use s3s::{S3Request, S3Response, S3Result};

use crate::{S3Adapter, request};

// Generate the whole impl so async_trait sees every method after expansion.
macro_rules! s3_handlers {
    ($($scope:ident $name:ident($input:ident, $output:ident);)+) => {
        #[async_trait::async_trait]
        impl s3s::S3 for S3Adapter {
            $(
                async fn $name(
                    &self,
                    req: S3Request<s3s::dto::$input>,
                ) -> S3Result<S3Response<s3s::dto::$output>> {
                    #[cfg(feature = "metrics")]
                    let metrics = self
                        .request_metrics(|| s3_handlers!(@context $scope, req))
                        .map_err(s3s::S3Error::internal_error)?;

                    #[cfg(feature = "metrics")]
                    {
                        request::$name::$name(self, req, metrics).await
                    }
                    #[cfg(not(feature = "metrics"))]
                    {
                        request::$name::$name(self, req).await
                    }
                }
            )+
        }
    };
    (@context object, $req:ident) => {
        crate::metrics::MetricsContext::object(
            $req.credentials.as_ref().map(|credential| credential.access_key.as_str()),
            &$req.input.bucket,
            &$req.input.key,
        )
    };
    (@context bucket, $req:ident) => {
        crate::metrics::MetricsContext::bucket(
            $req.credentials.as_ref().map(|credential| credential.access_key.as_str()),
            &$req.input.bucket,
        )
    };
    (@context service, $req:ident) => {
        crate::metrics::MetricsContext::service(
            $req.credentials.as_ref().map(|credential| credential.access_key.as_str()),
        )
    };
}

s3_handlers! {
    object abort_multipart_upload(AbortMultipartUploadInput, AbortMultipartUploadOutput);
    object complete_multipart_upload(CompleteMultipartUploadInput, CompleteMultipartUploadOutput);
    object copy_object(CopyObjectInput, CopyObjectOutput);
    bucket delete_objects(DeleteObjectsInput, DeleteObjectsOutput);
    object delete_object(DeleteObjectInput, DeleteObjectOutput);
    object upload_part(UploadPartInput, UploadPartOutput);
    object create_multipart_upload(CreateMultipartUploadInput, CreateMultipartUploadOutput);
    object put_object(PutObjectInput, PutObjectOutput);
    bucket list_multipart_uploads(ListMultipartUploadsInput, ListMultipartUploadsOutput);
    bucket list_objects(ListObjectsInput, ListObjectsOutput);
    service list_buckets(ListBucketsInput, ListBucketsOutput);
    bucket list_objects_v2(ListObjectsV2Input, ListObjectsV2Output);
    object list_parts(ListPartsInput, ListPartsOutput);
    object get_object(GetObjectInput, GetObjectOutput);
    object head_object(HeadObjectInput, HeadObjectOutput);
}
