use std::{
    error::Error,
    io,
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use backends::{DiscordFile, DiscordRegistry, FileBackend, RegistryBackend};
use blob_system::{
    BlobSystem, BlobWriterConfig, Compression, Encryption, FrameBudget, ReadBlob, SchedulerConfig,
    Window, WriteBlobPart,
};
use bytes::Bytes;
use http_adapter::{HttpAdapter, HttpAdapterConfig, HttpBody};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::{
    Method, Request, Response, StatusCode,
    body::{Frame, Incoming},
    header,
    service::{Service, service_fn},
};
use hyper_util::rt::TokioIo;
use object_registry::{ObjectRegistry, traits::ObjectRegistry as _};
use tokio_tcp_pool::{Endpoint, Pool, Route};

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

fn adapter(
    address: SocketAddr,
    prefix: &str,
    compression: Compression,
    encryption: Encryption,
) -> TestResult<(HttpAdapter, ObjectRegistry, BlobSystem)> {
    let database = sled::Config::new()
        .temporary(true)
        .flush_every_ms(None)
        .open()?;
    let pool = Pool::builder(Route::Direct {
        target: Endpoint::new("127.0.0.1", address.port())?,
    })
    .build()?;
    let backend = FileBackend::Discord(DiscordFile::new(pool.clone(), pool.clone()));
    let registry_backend = RegistryBackend::Discord(DiscordRegistry::new(
        pool,
        vec!["https://discord.com/api/webhooks/1/test".to_owned()],
        Vec::new(),
    ));
    let objects = ObjectRegistry::Local(object_registry::LocalRegistry::new(database.clone())?);
    let blobs = BlobSystem::new(
        blob_registry::BlobRegistry::Local(blob_registry::LocalRegistry::new(database.clone())?),
        file_registry::FileRegistry::Local(file_registry::LocalRegistry::new(
            database.clone(),
            registry_backend,
        )?),
        stream_registry::StreamRegistry::Local(stream_registry::LocalRegistry::new(database)?),
        backend,
    );
    let config = HttpAdapterConfig {
        prefix: prefix.to_owned(),
        writer: BlobWriterConfig {
            encryption,
            compression,
            frame_size: 64,
            max_file_size: 256,
        },
        scheduler: SchedulerConfig::new(Window::new(4, 16, 4)?, 0),
    };
    Ok((
        HttpAdapter::new(config, objects.clone(), blobs.clone(), FrameBudget::new(32))?,
        objects,
        blobs,
    ))
}

async fn request(
    adapter: &HttpAdapter,
    method: Method,
    path: &str,
    range: Option<&str>,
) -> TestResult<Response<HttpBody>> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(range) = range {
        builder = builder.header(header::RANGE, range);
    }
    Ok(adapter.call(builder.body(Full::new(Bytes::new()))?).await?)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn empty_objects_routes_metadata_and_send() -> TestResult {
    fn send<T: Send>() {}
    send::<HttpBody>();
    send::<HttpAdapter>();
    send::<ReadBlob<'static>>();
    send::<WriteBlobPart<'static>>();
    let (adapter, objects, blobs) = adapter(
        "127.0.0.1:1".parse()?,
        "tests///",
        Compression::None,
        Encryption::None,
    )?;
    let upload = adapter.call(
        Request::builder()
            .method(Method::POST)
            .uri("/object")
            .header(header::CONTENT_TYPE, "text/plain")
            .body(Full::new(Bytes::new()))?,
    );
    fn future_send<T: Send>(future: T) -> T {
        future
    }
    let uploaded = future_send(upload).await?;
    assert_eq!(uploaded.status(), StatusCode::CREATED);
    let location = uploaded
        .headers()
        .get(header::LOCATION)
        .ok_or("missing Location")?
        .to_str()?
        .to_owned();
    assert!(uploaded.into_body().collect().await?.to_bytes().is_empty());
    let id = location
        .strip_prefix("/object/")
        .ok_or("invalid Location")?;
    assert_eq!(id.len(), 32);
    let record = objects.head(&format!("tests/{id}"), None).await?;
    assert_eq!(record.metadata.get("size").map(String::as_str), Some("0"));
    assert!(objects.head(&format!("neighbor/{id}"), None).await.is_err());
    for method in [Method::GET, Method::HEAD] {
        let response = request(&adapter, method, &location, None).await?;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_LENGTH)
                .ok_or("missing size")?,
            "0"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .ok_or("missing type")?,
            "text/plain"
        );
        assert!(response.into_body().collect().await?.to_bytes().is_empty());
    }
    let head = request(&adapter, Method::HEAD, &location, Some("bytes=broken")).await?;
    assert_eq!(head.status(), StatusCode::OK);
    let unsatisfiable = request(&adapter, Method::GET, &location, Some("bytes=0-")).await?;
    assert_eq!(unsatisfiable.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        unsatisfiable
            .headers()
            .get(header::CONTENT_RANGE)
            .ok_or("missing range")?,
        "bytes */0"
    );
    for (method, path, status) in [
        (Method::GET, "/missing", StatusCode::NOT_FOUND),
        (Method::GET, "/object/invalid", StatusCode::BAD_REQUEST),
        (
            Method::HEAD,
            "/object/00000000000000000000000000000000",
            StatusCode::NOT_FOUND,
        ),
        (Method::PUT, "/object", StatusCode::METHOD_NOT_ALLOWED),
        (
            Method::POST,
            location.as_str(),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
    ] {
        let output = request(&adapter, method, path, None).await?;
        assert_eq!(output.status(), status);
        if status == StatusCode::METHOD_NOT_ALLOWED {
            assert!(output.headers().contains_key(header::ALLOW));
        }
        assert!(output.into_body().collect().await?.to_bytes().is_empty());
    }
    // HEAD resolves metadata only, even when the blob reference does not exist.
    let absent = "11111111111111111111111111111111";
    objects
        .store(
            &format!("tests/{absent}"),
            u128::MAX,
            object_registry::Metadata::from([
                ("size".to_owned(), "3".to_owned()),
                (
                    "content-type".to_owned(),
                    "application/octet-stream".to_owned(),
                ),
            ]),
        )
        .await?;
    assert_eq!(
        request(&adapter, Method::HEAD, &format!("/object/{absent}"), None)
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&adapter, Method::GET, &format!("/object/{absent}"), None)
            .await?
            .status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    for prefix in ["", "/", "bad\0prefix"] {
        let config = HttpAdapterConfig {
            prefix: prefix.to_owned(),
            writer: BlobWriterConfig {
                encryption: Encryption::None,
                compression: Compression::None,
                frame_size: 64,
                max_file_size: 256,
            },
            scheduler: SchedulerConfig::default(),
        };
        assert!(
            HttpAdapter::new(config, objects.clone(), blobs.clone(), FrameBudget::new(32)).is_err()
        );
    }
    let failed = StreamBody::new(futures_util::stream::once(async {
        Err::<Frame<Bytes>, _>(io::Error::other("input failed"))
    }));
    assert_eq!(
        adapter
            .call(
                Request::builder()
                    .method(Method::POST)
                    .uri("/object")
                    .body(failed)?
            )
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        objects
            .list("tests/", None, None, false)
            .await?
            .records
            .len(),
        2
    );
    Ok(())
}

// A loopback Discord HTTP backend exercises the real codecs, files and registries.
async fn backend(
    request: Request<Incoming>,
    files: Arc<Mutex<Vec<Bytes>>>,
) -> TestResult<Response<Full<Bytes>>> {
    let output = if request.method() == Method::POST {
        let boundary = request
            .headers()
            .get(header::CONTENT_TYPE)
            .ok_or("missing multipart type")?
            .to_str()?
            .split_once("boundary=")
            .ok_or("missing boundary")?
            .1
            .to_owned();
        let body = request.into_body().collect().await?.to_bytes();
        let marker = b"name=\"files[0]\"";
        let start = body
            .windows(marker.len())
            .position(|bytes| bytes == marker)
            .ok_or("missing file part")?;
        let headers = body.get(start..).ok_or("invalid file part")?;
        let offset = headers
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .ok_or("missing file headers")?
            .checked_add(4)
            .ok_or("offset overflow")?;
        let payload = headers
            .get(offset..)
            .ok_or("invalid payload offset")?
            .strip_suffix(format!("\r\n--{boundary}--\r\n").as_bytes())
            .ok_or("missing closing boundary")?;
        let id = {
            let mut files = files.lock().map_err(|_| "backend lock poisoned")?;
            let id = files.len();
            files.push(Bytes::copy_from_slice(payload));
            id
        };
        let json = format!(
            "{{\"id\":\"{id}\",\"attachments\":[{{\"url\":\"https://cdn.discordapp.com/attachments/1/{id}/file.bin?ex=ffffffffffffffff&is=0&hm=00\"}}]}}"
        );
        Response::new(Full::new(Bytes::from(json)))
    } else {
        let id = request
            .uri()
            .path()
            .split('/')
            .nth(3)
            .ok_or("missing attachment id")?
            .parse::<usize>()?;
        let payload = files
            .lock()
            .map_err(|_| "backend lock poisoned")?
            .get(id)
            .cloned()
            .ok_or("missing attachment")?;
        let range = request
            .headers()
            .get(header::RANGE)
            .ok_or("missing backend range")?
            .to_str()?
            .strip_prefix("bytes=")
            .ok_or("invalid range")?;
        let (start, end) = range.split_once('-').ok_or("invalid bounds")?;
        let start = start.parse::<usize>()?;
        let end = if end.is_empty() {
            payload.len()
        } else {
            end.parse::<usize>()?
                .checked_add(1)
                .ok_or("range overflow")?
        };
        let bytes =
            Bytes::copy_from_slice(payload.get(start..end).ok_or("invalid physical range")?);
        let mut output = Response::new(Full::new(bytes));
        *output.status_mut() = StatusCode::PARTIAL_CONTENT;
        output
    };
    let mut output = output;
    output.headers_mut().insert(
        header::CONNECTION,
        hyper::header::HeaderValue::from_static("close"),
    );
    Ok(output)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_nonempty_storage_smoke() -> TestResult {
    tokio::time::timeout(Duration::from_secs(30), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let files = Arc::new(Mutex::new(Vec::new()));
        let server = tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                let files = files.clone();
                tokio::spawn(async move {
                    let service = service_fn(move |request| backend(request, files.clone()));
                    hyper::server::conn::http1::Builder::new()
                        .serve_connection(TokioIo::new(socket), service)
                        .await
                });
            }
        });
        let (adapter, _, _) = adapter(address, "smoke", Compression::Lz4, Encryption::Aes256)?;
        let mut tasks = tokio::task::JoinSet::new();
        for seed in 0_u8..4 {
            let adapter = adapter.clone();
            tasks.spawn(async move {
                let payload: Vec<u8> = (0_u16..2048)
                    .map(|index| u8::try_from(index % 251).map(|byte| byte.wrapping_add(seed)))
                    .collect::<Result<_, _>>()?;
                let input = Bytes::from(payload);
                let uploaded = adapter
                    .call(
                        Request::builder()
                            .method(Method::POST)
                            .uri("/object")
                            .header(header::CONTENT_TYPE, "application/test")
                            .body(Full::new(input.clone()))?,
                    )
                    .await?;
                assert_eq!(uploaded.status(), StatusCode::CREATED);
                let location = uploaded
                    .headers()
                    .get(header::LOCATION)
                    .ok_or("missing Location")?
                    .to_str()?
                    .to_owned();
                let complete = request(&adapter, Method::GET, &location, None).await?;
                assert_eq!(complete.status(), StatusCode::OK);
                assert_eq!(complete.into_body().collect().await?.to_bytes(), input);
                for (range, selected, content_range) in [
                    ("bytes=17-113", 17..114, "bytes 17-113/2048"),
                    ("bytes=2000-", 2000..2048, "bytes 2000-2047/2048"),
                    ("bytes=-27", 2021..2048, "bytes 2021-2047/2048"),
                ] {
                    let partial = request(&adapter, Method::GET, &location, Some(range)).await?;
                    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
                    assert_eq!(
                        partial
                            .headers()
                            .get(header::CONTENT_RANGE)
                            .ok_or("missing range")?,
                        content_range
                    );
                    assert_eq!(
                        partial.into_body().collect().await?.to_bytes().as_ref(),
                        input.get(selected).ok_or("invalid expected selection")?
                    );
                }
                let head = request(&adapter, Method::HEAD, &location, Some("bytes=17-113")).await?;
                assert_eq!(head.status(), StatusCode::OK);
                assert_eq!(
                    head.headers()
                        .get(header::CONTENT_LENGTH)
                        .ok_or("missing size")?,
                    "2048"
                );
                assert!(head.into_body().collect().await?.to_bytes().is_empty());
                for range in ["bytes=0-1,3-4", "items=0-1"] {
                    let response = request(&adapter, Method::GET, &location, Some(range)).await?;
                    assert_eq!(response.status(), StatusCode::OK);
                    assert_eq!(response.into_body().collect().await?.to_bytes(), input);
                }
                let conditional = adapter
                    .call(
                        Request::builder()
                            .uri(&location)
                            .header(header::RANGE, "bytes=0-1")
                            .header(header::IF_RANGE, "\"unknown\"")
                            .body(Full::new(Bytes::new()))?,
                    )
                    .await?;
                assert_eq!(conditional.status(), StatusCode::OK);
                assert_eq!(conditional.into_body().collect().await?.to_bytes(), input);
                assert_eq!(
                    request(&adapter, Method::GET, &location, Some("bytes=invalid"))
                        .await?
                        .status(),
                    StatusCode::BAD_REQUEST
                );
                let failed = StreamBody::new(futures_util::stream::iter([
                    Ok(Frame::data(Bytes::from_static(b"unpublished"))),
                    Err(io::Error::other("interrupted input")),
                ]));
                assert_eq!(
                    adapter
                        .call(
                            Request::builder()
                                .method(Method::POST)
                                .uri("/object")
                                .body(failed)?
                        )
                        .await?
                        .status(),
                    StatusCode::BAD_REQUEST
                );
                Ok::<_, Box<dyn Error + Send + Sync>>(())
            });
        }
        while let Some(result) = tasks.join_next().await {
            result??;
        }
        server.abort();
        Ok::<_, Box<dyn Error + Send + Sync>>(())
    })
    .await??;
    Ok(())
}
