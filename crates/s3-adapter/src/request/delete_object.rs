use access_controller::Action;
use object_registry::{VersionId, traits::ObjectRegistry as _};
use s3s::{
    S3Request, S3Response, S3Result,
    dto::{DeleteObjectInput, DeleteObjectOutput},
};

use crate::{S3Adapter, access::S3Access, error, to_key};

pub(crate) async fn delete_object_access(
    access: &S3Access,
    req: &mut S3Request<DeleteObjectInput>,
) -> S3Result<()> {
    access
        .authorize(
            req.credentials.as_ref(),
            &to_key!(req.input),
            Action::Delete,
        )
        .await
}

pub(crate) async fn delete_object(
    adapter: &S3Adapter,
    req: S3Request<DeleteObjectInput>,
) -> S3Result<S3Response<DeleteObjectOutput>> {
    let version_id = validate_input(&req.input)?;
    let key = to_key!(req.input);

    let delete_marker = if let Some(version) = version_id {
        match adapter.objects.head(&key, Some(version)).await {
            Ok(record) => Some(record.is_delete_marker),
            Err(object_registry::Error::VersionNotFound { .. }) => None,
            Err(error) => return Err(error::object_registry_error(error)),
        }
    } else {
        Some(true)
    };

    let result = adapter
        .objects
        .delete(&key, version_id)
        .await
        .map_err(error::object_registry_error)?;

    Ok(S3Response::new(DeleteObjectOutput {
        delete_marker,
        version_id: Some(result.version_id.to_string()),
        ..Default::default()
    }))
}

fn validate_input(input: &DeleteObjectInput) -> S3Result<Option<VersionId>> {
    if input.bucket.is_empty()
        || input.bucket.contains(['/', '\0'])
        || input.key.is_empty()
        || input.key.contains('\0')
    {
        return Err(s3s::s3_error!(InvalidArgument, "invalid bucket or key"));
    }

    if input.bypass_governance_retention.is_some()
        || input.expected_bucket_owner.is_some()
        || input.if_match.is_some()
        || input.if_match_last_modified_time.is_some()
        || input.if_match_size.is_some()
        || input.mfa.is_some()
        || input.request_payer.is_some()
    {
        return Err(s3s::s3_error!(
            NotImplemented,
            "unsupported DeleteObject option"
        ));
    }

    input
        .version_id
        .as_deref()
        .map(str::parse::<VersionId>)
        .transpose()
        .map_err(|error| s3s::s3_error!(error, InvalidArgument))
}

#[cfg(test)]
mod tests {
    use super::*;
    use s3s::S3ErrorCode;

    #[test]
    fn versions_and_unsupported_options_are_validated() -> S3Result<()> {
        let mut input = DeleteObjectInput {
            bucket: "bucket".to_owned(),
            key: "key".to_owned(),
            ..Default::default()
        };
        if validate_input(&input)?.is_some() {
            return Err(s3s::s3_error!(InternalError, "missing version was changed"));
        }
        for version in ["", "null", "-1", "18446744073709551616"] {
            input.version_id = Some(version.to_owned());
            if !matches!(validate_input(&input), Err(error) if error.code() == &S3ErrorCode::InvalidArgument)
            {
                return Err(s3s::s3_error!(InternalError, "invalid version accepted"));
            }
        }
        input.version_id = Some(VersionId::MAX.to_string());
        if validate_input(&input)? != Some(VersionId::MAX) {
            return Err(s3s::s3_error!(InternalError, "version was changed"));
        }
        input.bypass_governance_retention = Some(true);
        if !matches!(validate_input(&input), Err(error) if error.code() == &S3ErrorCode::NotImplemented)
        {
            return Err(s3s::s3_error!(
                InternalError,
                "unsupported delete option accepted"
            ));
        }
        Ok(())
    }
}
