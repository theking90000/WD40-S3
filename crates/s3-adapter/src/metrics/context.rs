//! Request attribution and metric destinations.

use metrics_registry::{MetricRecord, Record};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

const SEGMENT: &AsciiSet = &CONTROLS.add(b'/').add(b'%');
const OBJECT_KEY: &AsciiSet = &CONTROLS.add(b'%');

/// Prepared destinations for one request, without a registry or mutable state.
#[derive(Clone, Debug)]
pub struct MetricsContext {
    destinations: Vec<Destination>,
}

#[derive(Clone, Debug)]
struct Destination {
    key: String,
    aggregates: Vec<String>,
}

impl MetricsContext {
    /// Attributes a request without a bucket, such as ListBuckets.
    /// Missing credentials use the reserved `unauthenticated` segment.
    pub fn service(credential_id: Option<&str>) -> Self {
        Self::build(credential_id, None)
    }

    /// Attributes a request to a bucket without inventing an object key.
    pub fn bucket(credential_id: Option<&str>, bucket: &str) -> Self {
        Self::build(credential_id, Some(bucket))
    }

    /// Attributes a request to an object. Slashes inside its key are preserved.
    pub fn object(credential_id: Option<&str>, bucket: &str, key: &str) -> Self {
        let mut context = Self::bucket(credential_id, bucket);
        context.append_object_key(key);
        context
    }

    /// Unknown legacy publishers contribute only to the bucket namespace.
    pub(super) fn stored_object(credential_id: Option<&str>, bucket: &str, key: &str) -> Self {
        let mut context = match credential_id {
            Some(id) => Self::bucket(Some(id), bucket),
            None => Self {
                destinations: vec![Self::bucket_destination(Some(bucket))],
            },
        };
        context.append_object_key(key);
        context
    }

    fn append_object_key(&mut self, key: &str) {
        let key = utf8_percent_encode(key, OBJECT_KEY).to_string();
        for destination in &mut self.destinations {
            destination.key.push_str(&key);
        }
    }

    fn bucket_destination(bucket: Option<&str>) -> Destination {
        let mut by_bucket = Destination {
            key: "/by-buckets/".to_owned(),
            aggregates: vec!["/by-buckets/".to_owned()],
        };
        if let Some(bucket) = bucket {
            by_bucket
                .key
                .push_str(&format!("{}/", utf8_percent_encode(bucket, SEGMENT)));
            by_bucket.aggregates.push(by_bucket.key.clone());
        }
        by_bucket
    }

    fn build(credential_id: Option<&str>, bucket: Option<&str>) -> Self {
        let credential = utf8_percent_encode(
            credential_id.unwrap_or(crate::metadata::UNAUTHENTICATED_CREDENTIAL),
            SEGMENT,
        );
        let credential_prefix = format!("/by-credentials/{credential}/");
        let mut by_credential = Destination {
            key: credential_prefix.clone(),
            aggregates: vec![credential_prefix],
        };
        if let Some(bucket) = bucket {
            let suffix = format!("{}/", utf8_percent_encode(bucket, SEGMENT));
            by_credential.key.push_str(&suffix);
        }
        Self {
            destinations: vec![Self::bucket_destination(bucket), by_credential],
        }
    }

    pub(super) fn records(&self, metrics: Vec<MetricRecord>) -> Vec<Record> {
        self.destinations
            .iter()
            .map(|destination| Record {
                key: destination.key.clone(),
                aggregates: destination.aggregates.clone(),
                metrics: metrics.clone(),
            })
            .collect()
    }
}
