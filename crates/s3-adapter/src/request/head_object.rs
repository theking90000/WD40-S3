use s3s::{
    S3Request, S3Response, S3Result,
    dto::{HeadObjectInput, HeadObjectOutput},
};

use crate::S3Adapter;

pub(crate) async fn head_object_access(
    _adapter: &S3Adapter,
    _req: &mut S3Request<HeadObjectInput>,
) -> S3Result<()> {
    Err(s3s::s3_error!(
        NotImplemented,
        "HeadObject access is not implemented yet"
    ))
}

pub(crate) async fn head_object(
    _adapter: &S3Adapter,
    _req: S3Request<HeadObjectInput>,
) -> S3Result<S3Response<HeadObjectOutput>> {
    Err(s3s::s3_error!(
        NotImplemented,
        "HeadObject is not implemented yet"
    ))
}
