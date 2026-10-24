use std::sync::Arc;

use access_controller::{Action, CompiledPolicy, Decision};
use s3s::{
    S3Error, S3Request, S3Response, S3Result,
    auth::Credentials,
    dto::{
        DeleteObjectInput, DeleteObjectOutput, DeleteObjectsInput, DeleteObjectsOutput,
        DeletedObject, Error as ObjectError, ObjectIdentifier,
    },
};

use crate::{S3Adapter, access::S3Access, to_key};

pub(crate) async fn delete_objects_access(
    access: &S3Access,
    req: &mut S3Request<DeleteObjectsInput>,
) -> S3Result<()> {
    let policy = compile_policy(access, req.credentials.as_ref()).await?;
    req.extensions.insert(policy);
    Ok(())
}

async fn compile_policy(
    access: &S3Access,
    credentials: Option<&Credentials>,
) -> S3Result<Arc<CompiledPolicy>> {
    let principal = access.get_principal(credentials).await?;
    access
        .access
        .compile(principal)
        .await
        .map_err(S3Error::internal_error)
}

pub(crate) async fn delete_objects(
    adapter: &S3Adapter,
    mut req: S3Request<DeleteObjectsInput>,
    #[cfg(feature = "metrics")] mut metrics: Option<crate::metrics::RequestMetrics>,
) -> S3Result<S3Response<DeleteObjectsOutput>> {
    validate_input(&req.input)?;
    let policy = match req.extensions.remove::<Arc<CompiledPolicy>>() {
        Some(policy) => policy,
        None => {
            let access = S3Access::new(adapter.credentials.clone(), adapter.access.clone());
            compile_policy(&access, req.credentials.as_ref()).await?
        }
    };

    let quiet = req.input.delete.quiet.unwrap_or(false);
    let bucket = req.input.bucket;
    let mut output = DeleteObjectsOutput::default();

    for object in req.input.delete.objects {
        let input = DeleteObjectInput {
            bucket: bucket.clone(),
            key: object.key.clone(),
            version_id: object.version_id.clone(),
            ..Default::default()
        };
        let result = if policy.authorize(Action::Delete, &to_key!(input)) != Decision::Allow {
            Err(s3s::s3_error!(AccessDenied))
        } else {
            match validate_object_options(&object) {
                Ok(()) => {
                    super::delete_object::delete(
                        adapter,
                        &input,
                        #[cfg(feature = "metrics")]
                        &mut metrics,
                    )
                    .await
                }
                Err(error) => Err(error),
            }
        };
        record_result(&mut output, object, result, quiet);
    }

    success!(metrics);

    Ok(S3Response::new(output))
}

fn validate_input(input: &DeleteObjectsInput) -> S3Result<()> {
    if input.bucket.is_empty() || input.bucket.contains(['/', '\0']) {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket"));
    }
    if !(1..=1_000).contains(&input.delete.objects.len()) {
        return Err(s3s::s3_error!(
            MalformedXML,
            "expected between 1 and 1000 objects"
        ));
    }
    reject_unsupported!(
        "DeleteObjects",
        input,
        bypass_governance_retention,
        checksum_algorithm,
        expected_bucket_owner,
        mfa,
        request_payer,
    );
    Ok(())
}

fn validate_object_options(object: &ObjectIdentifier) -> S3Result<()> {
    reject_unsupported!("DeleteObjects", object, e_tag, last_modified_time, size,);
    Ok(())
}

fn record_result(
    output: &mut DeleteObjectsOutput,
    object: ObjectIdentifier,
    result: S3Result<DeleteObjectOutput>,
    quiet: bool,
) {
    match result {
        Ok(result) if !quiet => {
            let delete_marker_version_id = if result.delete_marker == Some(true) {
                result.version_id
            } else {
                None
            };
            output
                .deleted
                .get_or_insert_with(Vec::new)
                .push(DeletedObject {
                    key: Some(object.key),
                    version_id: object.version_id,
                    delete_marker: result.delete_marker,
                    delete_marker_version_id,
                });
        }
        Ok(_) => {}
        Err(error) => output
            .errors
            .get_or_insert_with(Vec::new)
            .push(ObjectError {
                code: Some(error.code().as_str().to_owned()),
                message: Some(
                    error
                        .message()
                        .unwrap_or_else(|| error.code().as_str())
                        .to_owned(),
                ),
                key: Some(object.key),
                version_id: object.version_id,
            }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_bounds_quiet_errors_and_marker_ids() -> S3Result<()> {
        let object = ObjectIdentifier {
            key: "key".to_owned(),
            ..Default::default()
        };
        let mut input = DeleteObjectsInput {
            bucket: "bucket".to_owned(),
            bypass_governance_retention: None,
            checksum_algorithm: None,
            delete: Default::default(),
            expected_bucket_owner: None,
            mfa: None,
            request_payer: None,
        };
        if validate_input(&input).is_ok() {
            return Err(s3s::s3_error!(InternalError, "empty batch accepted"));
        }
        input.delete.objects = vec![object.clone(); 1_000];
        validate_input(&input)?;
        input.delete.objects.push(object.clone());
        if validate_input(&input).is_ok() {
            return Err(s3s::s3_error!(InternalError, "oversized batch accepted"));
        }

        let marker = DeleteObjectOutput {
            delete_marker: Some(true),
            version_id: Some("42".to_owned()),
            ..Default::default()
        };
        let mut output = DeleteObjectsOutput::default();
        record_result(&mut output, object.clone(), Ok(marker.clone()), true);
        record_result(
            &mut output,
            object.clone(),
            Err(s3s::s3_error!(AccessDenied)),
            true,
        );
        if output.deleted.is_some()
            || !matches!(output.errors.as_deref(), Some([error]) if error.code.as_deref() == Some("AccessDenied") && error.key.as_deref() == Some("key"))
        {
            return Err(s3s::s3_error!(
                InternalError,
                "quiet mode lost an error or returned a success"
            ));
        }
        record_result(&mut output, object, Ok(marker), false);
        if !matches!(
            output.deleted.as_deref(),
            Some([deleted]) if deleted.version_id.is_none()
                && deleted.delete_marker == Some(true)
                && deleted.delete_marker_version_id.as_deref() == Some("42")
        ) {
            return Err(s3s::s3_error!(
                InternalError,
                "delete marker version was misplaced"
            ));
        }
        Ok(())
    }
}
