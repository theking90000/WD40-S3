//! Anonymous HTTP object creation and streaming reads over existing WD40 handles.

#[cfg(feature = "cli")]
pub mod cli;
mod range;

use std::{convert::Infallible, future::Future, io, pin::Pin};

use blob_system::{
    BlobSystem, BlobWriterConfig, BytesFrameAllocator, FrameBudget, ReadFrame, SchedulerConfig,
};
use bytes::Bytes;
use futures_util::{StreamExt, TryStreamExt, io::AsyncWriteExt};
use http_body_util::{BodyExt, Empty, StreamBody, combinators::UnsyncBoxBody};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Body, Frame},
    header,
    service::Service,
};
use object_registry::{Metadata, ObjectRegistry, traits::ObjectRegistry as _};

/// A streaming response body that is Send without requiring Sync.
pub type HttpBody = UnsyncBoxBody<Bytes, io::Error>;

/// Settings independent of the listener, runtime and command line.
#[derive(Clone, Debug)]
pub struct HttpAdapterConfig {
    pub prefix: String,
    pub writer: BlobWriterConfig,
    pub scheduler: SchedulerConfig,
}

/// Cloneable Hyper service. Constructors start no tasks and perform no I/O.
#[derive(Clone)]
pub struct HttpAdapter {
    config: HttpAdapterConfig,
    objects: ObjectRegistry,
    blobs: BlobSystem,
    budget: FrameBudget,
}

impl HttpAdapter {
    pub fn new(
        mut config: HttpAdapterConfig,
        objects: ObjectRegistry,
        blobs: BlobSystem,
        budget: FrameBudget,
    ) -> io::Result<Self> {
        config.prefix = config.prefix.trim_end_matches('/').to_owned();
        if config.prefix.is_empty() || config.prefix.contains('\0') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "prefix must be nonempty and contain no NUL",
            ));
        }
        let mut scheduler = config.scheduler;
        blobs.validate_write_config(config.writer, &budget, &mut scheduler)?;
        blobs.read(0, 0..0, &budget, &mut scheduler)?;
        Ok(Self {
            config,
            objects,
            blobs,
            budget,
        })
    }

    async fn handle<B>(&self, request: Request<B>) -> Response<HttpBody>
    where
        B: Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let result = self.dispatch(request).await;
        match result {
            Ok(response) => response,
            Err(status) => response(status),
        }
    }

    async fn dispatch<B>(&self, request: Request<B>) -> Result<Response<HttpBody>, StatusCode>
    where
        B: Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let path = request.uri().path();
        if path == "/object" {
            if request.method() != Method::POST {
                return Ok(method_not_allowed("POST"));
            }
            return self.upload(request).await;
        }
        let Some(id) = path.strip_prefix("/object/") else {
            return Err(StatusCode::NOT_FOUND);
        };
        if id.contains('/') {
            return Err(StatusCode::NOT_FOUND);
        }
        let base62 = id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric());
        // ponytail: existing hexadecimal keys remain readable without a migration.
        let legacy = id.len() == 32
            && id
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !base62 && !legacy {
            return Err(StatusCode::BAD_REQUEST);
        }
        if request.method() != Method::GET && request.method() != Method::HEAD {
            return Ok(method_not_allowed("GET, HEAD"));
        }
        let id = id.to_owned();
        let (parts, _) = request.into_parts();
        self.download(parts.method, parts.headers, &id).await
    }

    async fn upload<B>(&self, request: Request<B>) -> Result<Response<HttpBody>, StatusCode>
    where
        B: Body<Data = Bytes> + Send + 'static,
        B::Error: std::error::Error + Send + Sync + 'static,
    {
        let content_type = request
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| StatusCode::BAD_REQUEST)?
            .unwrap_or_else(|| "application/octet-stream".to_owned());
        let mut metadata = Metadata::new();
        for (name, value) in request.headers() {
            if let Some(key) = name.as_str().strip_prefix("x-meta-") {
                if key.is_empty() {
                    return Err(StatusCode::BAD_REQUEST);
                }
                let value = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
                metadata
                    .entry(name.as_str().to_owned())
                    .and_modify(|stored| {
                        stored.push_str(", ");
                        stored.push_str(value);
                    })
                    .or_insert_with(|| value.to_owned());
            }
        }
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(internal)?;
        let id = encode_id(u128::from_be_bytes(random)).map_err(internal)?;
        let mut body = Box::pin(request.into_body());
        // ponytail: defer blob allocation until the first nonempty frame or EOF.
        let first = loop {
            match body.as_mut().frame().await {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) if !data.is_empty() => break Some(data),
                    _ => continue,
                },
                Some(Err(_)) => return Err(StatusCode::BAD_REQUEST),
                None => break None,
            }
        };
        let blob_id = self.blobs.create().await.map_err(internal)?;
        let parts = if let Some(first) = first {
            let mut scheduler = self.config.scheduler;
            let frame_size = usize::try_from(self.config.writer.frame_size).map_err(internal)?;
            let mut allocator = BytesFrameAllocator::new(frame_size).map_err(internal)?;
            let mut writer = self
                .blobs
                .write_part(
                    blob_id,
                    1,
                    self.config.writer,
                    &mut allocator,
                    &self.budget,
                    &mut scheduler,
                )
                .await
                .map_err(internal)?;
            writer.write_all(&first).await.map_err(internal)?;
            loop {
                let mut pinned_body = body.as_mut();
                let next = tokio::select! {
                    biased;
                    result = writer.progress() => { return Err(internal(result)); }
                    frame = pinned_body.frame() => frame,
                };
                match next {
                    Some(Ok(frame)) => {
                        if let Ok(data) = frame.into_data() {
                            writer.write_all(&data).await.map_err(internal)?;
                        }
                    }
                    Some(Err(_)) => return Err(StatusCode::BAD_REQUEST),
                    None => break,
                }
            }
            writer.close().await.map_err(internal)?;
            vec![1]
        } else {
            Vec::new()
        };
        let length = self
            .blobs
            .finalize(blob_id, parts)
            .await
            .map_err(internal)?;
        metadata.insert("size".to_owned(), length.to_string());
        metadata.insert("content-type".to_owned(), content_type);
        self.objects
            .store(&format!("{}/{}", self.config.prefix, id), blob_id, metadata)
            .await
            .map_err(internal)?;
        let mut output = response(StatusCode::CREATED);
        output.headers_mut().insert(
            header::LOCATION,
            format!("/object/{id}").parse().map_err(internal)?,
        );
        Ok(output)
    }

    async fn download(
        &self,
        method: Method,
        headers: hyper::HeaderMap,
        id: &str,
    ) -> Result<Response<HttpBody>, StatusCode> {
        let record = self
            .objects
            .head(&format!("{}/{}", self.config.prefix, id), None)
            .await
            .map_err(|error| match error {
                object_registry::Error::ObjectNotFound(_) => StatusCode::NOT_FOUND,
                error => internal(error),
            })?;
        let blob_id = record.blob_id.ok_or(StatusCode::NOT_FOUND)?;
        let length = record
            .metadata
            .get("size")
            .ok_or_else(|| internal("missing object size metadata"))?
            .parse::<u64>()
            .map_err(internal)?;
        let content_type = record
            .metadata
            .get("content-type")
            .ok_or_else(|| internal("missing object content-type metadata"))?
            .parse::<hyper::header::HeaderValue>()
            .map_err(internal)?;
        let selection = if method == Method::HEAD || headers.contains_key(header::IF_RANGE) {
            range::Selection::Full
        } else {
            let mut values = headers.get_all(header::RANGE).iter();
            let value = values
                .next()
                .map(|value| value.to_str())
                .transpose()
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            // ponytail: multiple Range fields, like multiple ranges, request a full response.
            if values.next().is_some() {
                range::Selection::Full
            } else {
                range::select(value, length).map_err(|_| StatusCode::BAD_REQUEST)?
            }
        };
        if selection == range::Selection::Unsatisfiable {
            let mut output = response(StatusCode::RANGE_NOT_SATISFIABLE);
            output.headers_mut().insert(
                header::CONTENT_RANGE,
                format!("bytes */{length}").parse().map_err(internal)?,
            );
            return Ok(output);
        }
        let (status, selected) = match selection {
            range::Selection::Partial(selected) => (StatusCode::PARTIAL_CONTENT, selected),
            _ => (StatusCode::OK, 0..length),
        };
        let count = selected
            .end
            .checked_sub(selected.start)
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        let mut output = response(status);
        output
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
        output.headers_mut().insert(
            header::ACCEPT_RANGES,
            hyper::header::HeaderValue::from_static("bytes"),
        );
        output.headers_mut().insert(
            header::CONTENT_LENGTH,
            count.to_string().parse().map_err(internal)?,
        );
        for (name, value) in &record.metadata {
            if name.starts_with("x-meta-") {
                output.headers_mut().insert(
                    hyper::header::HeaderName::from_bytes(name.as_bytes()).map_err(internal)?,
                    value
                        .parse::<hyper::header::HeaderValue>()
                        .map_err(internal)?,
                );
            }
        }
        if status == StatusCode::PARTIAL_CONTENT {
            let end = selected
                .end
                .checked_sub(1)
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
            output.headers_mut().insert(
                header::CONTENT_RANGE,
                format!("bytes {}-{end}/{length}", selected.start)
                    .parse()
                    .map_err(internal)?,
            );
        }
        if method == Method::HEAD || count == 0 {
            return Ok(output);
        }
        let blobs = self.blobs.clone();
        let budget = self.budget.clone();
        let mut scheduler = self.config.scheduler;
        let data = async_stream::try_stream! {
            let mut reader = blobs.read(blob_id, selected, &budget, &mut scheduler).map_err(io::Error::from)?;
            let mut delivered = 0_u64;
            let mut frame = ReadFrame::default();
            while reader.read_frame(&mut frame).await.map_err(io::Error::from)?.is_some() {
                let bytes = selected_bytes(std::mem::take(&mut frame))?;
                delivered = delivered.checked_add(u64::try_from(bytes.len()).map_err(io::Error::other)?)
                    .ok_or_else(|| io::Error::other("response length overflow"))?;
                if delivered > count { Err(io::Error::other("blob exceeds recorded size"))?; }
                yield Frame::data(bytes);
            }
            if delivered != count { Err(io::Error::other("blob is shorter than recorded size"))?; }
        };
        let mut data = Box::pin(data);
        let first = data
            .try_next()
            .await
            .map_err(internal)?
            .ok_or_else(|| internal("nonempty blob returned no frames"))?;
        let stream =
            futures_util::stream::once(async move { Ok::<_, io::Error>(first) }).chain(data);
        *output.body_mut() = BodyExt::boxed_unsync(StreamBody::new(stream));
        Ok(output)
    }
}

impl<B> Service<Request<B>> for HttpAdapter
where
    B: Body<Data = Bytes> + Send + 'static,
    B::Error: std::error::Error + Send + Sync + 'static,
{
    type Response = Response<HttpBody>;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, request: Request<B>) -> Self::Future {
        let adapter = self.clone();
        Box::pin(async move { Ok(adapter.handle(request).await) })
    }
}

fn internal<T: std::fmt::Debug>(error: T) -> StatusCode {
    eprintln!("HTTP Adapter internal error: {error:?}");
    StatusCode::INTERNAL_SERVER_ERROR
}

const ID_ALPHABET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

fn encode_id(mut value: u128) -> io::Result<String> {
    let mut encoded = [b'0'; 22];
    for digit in encoded.iter_mut().rev() {
        let index = usize::try_from(value % 62).map_err(io::Error::other)?;
        *digit = ID_ALPHABET
            .get(index)
            .copied()
            .ok_or_else(|| io::Error::other("invalid base62 digit"))?;
        value /= 62;
    }
    String::from_utf8(encoded.to_vec()).map_err(io::Error::other)
}

fn response(status: StatusCode) -> Response<HttpBody> {
    let mut output = Response::new(
        Empty::<Bytes>::new()
            .map_err(|error: Infallible| match error {})
            .boxed_unsync(),
    );
    *output.status_mut() = status;
    output
}

fn method_not_allowed(allow: &'static str) -> Response<HttpBody> {
    let mut output = response(StatusCode::METHOD_NOT_ALLOWED);
    output.headers_mut().insert(
        header::ALLOW,
        hyper::header::HeaderValue::from_static(allow),
    );
    output
}

// Bytes owns the selected frame without copying its payload or using panicking slices.
struct SelectedFrame(ReadFrame);
impl AsRef<[u8]> for SelectedFrame {
    fn as_ref(&self) -> &[u8] {
        match self.0.buffer.get(self.0.selected.clone()) {
            Some(bytes) => bytes,
            None => &[],
        }
    }
}

fn selected_bytes(frame: ReadFrame) -> io::Result<Bytes> {
    if frame.selected.is_empty() || frame.buffer.get(frame.selected.clone()).is_none() {
        return Err(io::Error::other("invalid decoded frame selection"));
    }
    Ok(Bytes::from_owner(SelectedFrame(frame)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base62_preserves_all_128_bits() -> io::Result<()> {
        for value in [0, 1, 61, 62, u128::MAX] {
            let id = encode_id(value)?;
            assert_eq!(id.len(), 22);
            assert!(id.bytes().all(|byte| byte.is_ascii_alphanumeric()));
            let decoded = id.bytes().try_fold(0_u128, |decoded, byte| {
                let digit = ID_ALPHABET
                    .iter()
                    .position(|candidate| *candidate == byte)
                    .ok_or_else(|| io::Error::other("invalid encoded digit"))?;
                decoded
                    .checked_mul(62)
                    .and_then(|decoded| decoded.checked_add(digit as u128))
                    .ok_or_else(|| io::Error::other("base62 overflow"))
            })?;
            assert_eq!(decoded, value);
        }
        Ok(())
    }
}
