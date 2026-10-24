use s3s::{
    S3Request, S3Response, S3Result,
    dto::{ListObjectsInput, ListObjectsOutput, ListObjectsV2Input},
};

use crate::{S3Adapter, access::S3Access};

use super::list_objects_v2;

fn to_v2(input: ListObjectsInput) -> ListObjectsV2Input {
    ListObjectsV2Input {
        bucket: input.bucket,
        delimiter: input.delimiter,
        encoding_type: input.encoding_type,
        expected_bucket_owner: input.expected_bucket_owner,
        max_keys: input.max_keys,
        optional_object_attributes: input.optional_object_attributes,
        prefix: input.prefix,
        request_payer: input.request_payer,
        start_after: input.marker,
        ..Default::default()
    }
}

pub(crate) async fn list_objects_access(
    access: &S3Access,
    req: &mut S3Request<ListObjectsInput>,
) -> S3Result<()> {
    let policy = list_objects_v2::list_policy(
        access,
        req.credentials.as_ref(),
        &req.input.bucket,
        req.input.prefix.as_deref(),
    )
    .await?;
    req.extensions.insert(policy);
    Ok(())
}

pub(crate) async fn list_objects(
    adapter: &S3Adapter,
    req: S3Request<ListObjectsInput>,
    #[cfg(feature = "metrics")] metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<ListObjectsOutput>> {
    let (response, next_marker) =
        list_objects_v2::list_page(adapter, req.map_input(to_v2), false).await?;
    success!(metrics);
    Ok(response.map_output(|output| {
        let next_marker = if output.is_truncated == Some(true) && output.delimiter.is_some() {
            next_marker
        } else {
            None
        };
        ListObjectsOutput {
            name: output.name,
            prefix: output.prefix,
            marker: output.start_after,
            max_keys: output.max_keys,
            is_truncated: output.is_truncated,
            contents: output.contents,
            common_prefixes: output.common_prefixes,
            delimiter: output.delimiter,
            next_marker,
            encoding_type: output.encoding_type,
            request_charged: output.request_charged,
        }
    }))
}
