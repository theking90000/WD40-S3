/// Rejects the first unsupported option without including its value in the error.
macro_rules! reject_unsupported {
    ($operation:expr, $input:expr, $($field:ident),+ $(,)?) => {{
        let input = &$input;
        reject_unsupported!($operation; $($field => input.$field.is_some()),+);
    }};
    ($operation:expr; $($field:ident => $unsupported:expr),+ $(,)?) => {{
        let operation = $operation;
        $(
            if $unsupported {
                return Err(s3s::s3_error!(
                    NotImplemented,
                    "unsupported {} option: {}",
                    operation,
                    stringify!($field)
                ));
            }
        )+
    }};
}

/// Finishes an optional tracker only when metrics are compiled in.
macro_rules! success {
    ($metrics:expr) => {{
        #[cfg(feature = "metrics")]
        if let Some(metrics) = $metrics {
            metrics.finish($crate::metrics::RequestOutcome::Success);
        }
    }};
}

pub(crate) mod abort_multipart_upload;
pub(crate) mod complete_multipart_upload;
pub(crate) mod copy_object;
pub(crate) mod create_multipart_upload;
pub(crate) mod delete_object;
pub(crate) mod delete_objects;
pub(crate) mod get_object;
pub(crate) mod head_object;
pub(crate) mod list_buckets;
pub(crate) mod list_multipart_uploads;
pub(crate) mod list_objects;
pub(crate) mod list_objects_v2;
pub(crate) mod list_parts;
pub(crate) mod put_object;
pub(crate) mod upload_part;
