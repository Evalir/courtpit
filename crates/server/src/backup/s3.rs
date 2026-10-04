//! A minimal S3 client: AWS Signature V4 (header-based) over `reqwest`, path-style URLs, and
//! the six calls a backup needs (multipart create/part/complete/abort, list, delete).
//!
//! Hand-written instead of `aws-sdk-s3` to avoid the aws-smithy/aws-config dependency stack;
//! `reqwest` and `sha2` are already here, signing adds only `hmac`.

use std::{fmt, fmt::Write as _, time::Duration};

use anyhow::Context;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use reqwest::{Method, header};
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;

use super::{DumpReader, ObjectStore, StoreFuture, truncate};

/// Multipart part size; S3 needs at least 5 MiB for every part but the last.
const PART_SIZE: usize = 8 * 1024 * 1024;
/// Per-request timeout, so a stalled connection fails the job instead of hanging a tick.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
const ALGORITHM: &str = "AWS4-HMAC-SHA256";
/// SigV4 leaves `A-Za-z0-9-._~` as is and percent-encodes everything else.
const ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

fn uri_encode(text: &str) -> String {
    utf8_percent_encode(text, ENCODE).to_string()
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &str) -> anyhow::Result<Vec<u8>> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).context("invalid HMAC key")?;
    mac.update(data.as_bytes());
    Ok(mac.finalize().into_bytes().to_vec())
}

/// `20130524T000000Z`, the `x-amz-date` value.
fn amz_date(at: DateTime<Utc>) -> String {
    at.format("%Y%m%dT%H%M%SZ").to_string()
}

/// The canonical query string: names and values encoded, sorted by name.
fn canonical_query(params: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = params
        .iter()
        .map(|(name, value)| (uri_encode(name), uri_encode(value)))
        .collect();
    pairs.sort();
    let pairs: Vec<String> = pairs
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    pairs.join("&")
}

/// What a signature covers.
struct SignedRequest<'a> {
    method: &'a str,
    /// The URL path exactly as sent (already percent-encoded).
    path: &'a str,
    /// The query string exactly as sent, in canonical form.
    query: &'a str,
    /// Every header to sign (at least `host`, `x-amz-content-sha256` and `x-amz-date`).
    headers: &'a [(&'a str, &'a str)],
    /// Hex SHA-256 of the body.
    payload_sha256: &'a str,
}

struct Credentials {
    access_key: String,
    secret_key: String,
    region: String,
}

impl Credentials {
    /// The `Authorization` header value for `request` made at `at` (SigV4, service `s3`).
    fn authorization(
        &self,
        at: DateTime<Utc>,
        request: &SignedRequest<'_>,
    ) -> anyhow::Result<String> {
        let mut headers: Vec<(String, &str)> = request
            .headers
            .iter()
            .map(|(name, value)| (name.to_ascii_lowercase(), value.trim()))
            .collect();
        headers.sort();
        let mut canonical_headers = String::new();
        for (name, value) in &headers {
            let _ = writeln!(canonical_headers, "{name}:{value}");
        }
        let names: Vec<&str> = headers.iter().map(|(name, _)| name.as_str()).collect();
        let signed_headers = names.join(";");
        let canonical_request = format!(
            "{}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{}",
            request.method, request.path, request.query, request.payload_sha256
        );

        let date = at.format("%Y%m%d");
        let scope = format!("{date}/{}/s3/aws4_request", self.region);
        let to_sign = format!(
            "{ALGORITHM}\n{}\n{scope}\n{}",
            amz_date(at),
            sha256_hex(canonical_request.as_bytes())
        );
        let mut key = hmac_sha256(
            format!("AWS4{}", self.secret_key).as_bytes(),
            &date.to_string(),
        )?;
        for part in [self.region.as_str(), "s3", "aws4_request"] {
            key = hmac_sha256(&key, part)?;
        }
        let signature = hex(&hmac_sha256(&key, &to_sign)?);
        Ok(format!(
            "{ALGORITHM} Credential={}/{scope},SignedHeaders={signed_headers},Signature={signature}",
            self.access_key
        ))
    }
}

/// An S3-compatible bucket reached with path-style URLs (`{endpoint}/{bucket}/{key}`).
pub struct S3Store {
    client: reqwest::Client,
    endpoint: String,
    bucket: String,
    credentials: Credentials,
    part_size: usize,
}

impl fmt::Debug for S3Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Store")
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .finish_non_exhaustive()
    }
}

impl S3Store {
    /// Creates a client for `bucket` at `endpoint` (e.g. `https://<account>.r2.cloudflarestorage.com`).
    pub fn new(
        endpoint: &str,
        bucket: &str,
        region: &str,
        access_key: &str,
        secret_key: &str,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .context("building the S3 HTTP client")?,
            endpoint: endpoint.trim_end_matches('/').to_owned(),
            bucket: bucket.to_owned(),
            credentials: Credentials {
                access_key: access_key.to_owned(),
                secret_key: secret_key.to_owned(),
                region: region.to_owned(),
            },
            part_size: PART_SIZE,
        })
    }

    /// Smaller parts so tests need not stream megabytes.
    #[cfg(test)]
    const fn with_part_size(mut self, bytes: usize) -> Self {
        self.part_size = bytes;
        self
    }

    /// Sends one signed request for `key` (the bucket itself when empty) and returns the
    /// response if it succeeded; otherwise an error carrying S3's (truncated) message.
    async fn send(
        &self,
        method: Method,
        key: &str,
        query: &[(&str, &str)],
        body: Vec<u8>,
    ) -> anyhow::Result<reqwest::Response> {
        let mut url = format!("{}/{}", self.endpoint, uri_encode(&self.bucket));
        if !key.is_empty() {
            let segments: Vec<String> = key.split('/').map(uri_encode).collect();
            url = format!("{url}/{}", segments.join("/"));
        }
        if !query.is_empty() {
            url = format!("{url}?{}", canonical_query(query));
        }
        let url = reqwest::Url::parse(&url).context("building the S3 request URL")?;
        let host = match (url.host_str(), url.port()) {
            (Some(host), Some(port)) => format!("{host}:{port}"),
            (Some(host), None) => host.to_owned(),
            (None, _) => anyhow::bail!("the S3 endpoint has no host"),
        };

        let at = Utc::now();
        let date = amz_date(at);
        let payload_sha256 = sha256_hex(&body);
        let authorization = self.credentials.authorization(
            at,
            &SignedRequest {
                method: method.as_str(),
                path: url.path(),
                query: url.query().unwrap_or_default(),
                headers: &[
                    ("host", &host),
                    ("x-amz-content-sha256", &payload_sha256),
                    ("x-amz-date", &date),
                ],
                payload_sha256: &payload_sha256,
            },
        )?;
        let described = format!("{method} {}", url.path());
        let response = self
            .client
            .request(method, url)
            .header(header::HOST, host)
            .header("x-amz-content-sha256", payload_sha256)
            .header("x-amz-date", date)
            .header(header::AUTHORIZATION, authorization)
            .body(body)
            .send()
            .await
            .with_context(|| format!("S3 {described}"))?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let text = response.text().await.unwrap_or_default();
        anyhow::bail!("S3 {described} returned {status}: {}", truncate(&text, 500))
    }

    async fn create_upload(&self, key: &str) -> anyhow::Result<String> {
        let response = self
            .send(Method::POST, key, &[("uploads", "")], Vec::new())
            .await?;
        let xml = response.text().await?;
        element(&xml, "UploadId").context("S3 returned no UploadId")
    }

    /// Uploads `part` as part `number`; returns its ETag.
    async fn upload_part(
        &self,
        key: &str,
        upload_id: &str,
        number: usize,
        part: Vec<u8>,
    ) -> anyhow::Result<String> {
        let number = number.to_string();
        let response = self
            .send(
                Method::PUT,
                key,
                &[("partNumber", &number), ("uploadId", upload_id)],
                part,
            )
            .await?;
        let etag = response.headers().get(header::ETAG);
        let etag = etag.and_then(|value| value.to_str().ok());
        etag.map(str::to_owned)
            .context("S3 returned no ETag for a part")
    }

    async fn complete_upload(
        &self,
        key: &str,
        upload_id: &str,
        etags: &[String],
    ) -> anyhow::Result<()> {
        let mut body = String::from("<CompleteMultipartUpload>");
        for (index, etag) in etags.iter().enumerate() {
            let _ = write!(
                body,
                "<Part><PartNumber>{}</PartNumber><ETag>{etag}</ETag></Part>",
                index + 1
            );
        }
        body.push_str("</CompleteMultipartUpload>");
        let response = self
            .send(
                Method::POST,
                key,
                &[("uploadId", upload_id)],
                body.into_bytes(),
            )
            .await?;
        // S3 can answer 200 and still report a failure in the body.
        let xml = response.text().await?;
        if xml.contains("<Error>") {
            anyhow::bail!("S3 failed to complete the upload: {}", truncate(&xml, 500));
        }
        Ok(())
    }

    /// Reads `body` in `part_size` parts, uploads them and completes the upload.
    async fn upload_parts(
        &self,
        key: &str,
        upload_id: &str,
        body: &mut DumpReader,
    ) -> anyhow::Result<u64> {
        let mut etags = Vec::new();
        let mut total = 0;
        loop {
            let part = read_part(body, self.part_size)
                .await
                .context("reading the dump")?;
            // A short (or empty) part means the dump ended.
            let last = part.len() < self.part_size;
            if !part.is_empty() {
                total += part.len() as u64;
                let number = etags.len() + 1;
                let etag = self.upload_part(key, upload_id, number, part).await?;
                etags.push(etag);
            }
            if last {
                break;
            }
        }
        anyhow::ensure!(total > 0, "the dump was empty");
        self.complete_upload(key, upload_id, &etags).await?;
        Ok(total)
    }

    async fn list_page(
        &self,
        prefix: &str,
        token: Option<&str>,
    ) -> anyhow::Result<(Vec<String>, Option<String>)> {
        let mut query = vec![("list-type", "2"), ("prefix", prefix)];
        if let Some(token) = token {
            query.push(("continuation-token", token));
        }
        let xml = self
            .send(Method::GET, "", &query, Vec::new())
            .await?
            .text()
            .await?;
        let keys = elements(&xml, "Contents")
            .filter_map(|entry| element(entry, "Key"))
            .collect();
        if element(&xml, "IsTruncated").as_deref() != Some("true") {
            return Ok((keys, None));
        }
        let next = element(&xml, "NextContinuationToken")
            .context("S3 listing is truncated but has no continuation token")?;
        Ok((keys, Some(next)))
    }
}

/// Reads up to `size` bytes, fewer only at the end of the stream.
async fn read_part(body: &mut DumpReader, size: usize) -> std::io::Result<Vec<u8>> {
    let mut part = vec![0; size];
    let mut filled = 0;
    while filled < size {
        let read = body.read(&mut part[filled..]).await?;
        if read == 0 {
            break;
        }
        filled += read;
    }
    part.truncate(filled);
    Ok(part)
}

impl ObjectStore for S3Store {
    fn put_stream<'a>(&'a self, key: &'a str, mut body: DumpReader) -> StoreFuture<'a, u64> {
        Box::pin(async move {
            let upload_id = self.create_upload(key).await?;
            let uploaded = self.upload_parts(key, &upload_id, &mut body).await;
            if uploaded.is_err() {
                // Don't leave a half-finished upload accruing storage.
                if let Err(err) = self
                    .send(Method::DELETE, key, &[("uploadId", &upload_id)], Vec::new())
                    .await
                {
                    tracing::warn!(error = ?err, %key, "aborting the multipart upload failed");
                }
            }
            uploaded
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> StoreFuture<'a, Vec<String>> {
        Box::pin(async move {
            let mut keys = Vec::new();
            let mut token = None;
            loop {
                let (page, next) = self.list_page(prefix, token.as_deref()).await?;
                keys.extend(page);
                if next.is_none() {
                    return Ok(keys);
                }
                token = next;
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StoreFuture<'a, ()> {
        Box::pin(async move {
            let _ = self.send(Method::DELETE, key, &[], Vec::new()).await?;
            Ok(())
        })
    }
}

/// The raw text between each `<tag>` and `</tag>` in `xml`, in order. S3 escapes `<` in text,
/// so a literal tag in the markup is never part of a value.
fn elements<'a>(xml: &'a str, tag: &str) -> impl Iterator<Item = &'a str> {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut rest = xml;
    std::iter::from_fn(move || {
        let (_, after) = rest.split_once(&open)?;
        let (inner, tail) = after.split_once(&close)?;
        rest = tail;
        Some(inner)
    })
}

/// The unescaped text of the first `<tag>` in `xml`.
fn element(xml: &str, tag: &str) -> Option<String> {
    elements(xml, tag).next().map(unescape)
}

/// Decodes the XML entities S3 emits (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&#NN;`).
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let decoded = after
            .split_once(';')
            .and_then(|(entity, tail)| Some((decode_entity(entity)?, tail)));
        if let Some((ch, tail)) = decoded {
            out.push(ch);
            rest = tail;
        } else {
            out.push('&');
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let digits = entity.strip_prefix('#')?;
            let code = digits
                .strip_prefix(['x', 'X'])
                .map_or_else(|| digits.parse(), |hex| u32::from_str_radix(hex, 16));
            char::from_u32(code.ok()?)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io,
        pin::Pin,
        sync::{Arc, Mutex},
        task::{Context, Poll},
    };

    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, Method as HttpMethod, StatusCode, Uri},
    };
    use chrono::TimeZone;
    use tokio::io::{AsyncRead, ReadBuf};

    use super::*;

    fn aws_example() -> (Credentials, DateTime<Utc>) {
        let credentials = Credentials {
            access_key: "AKIAIOSFODNN7EXAMPLE".into(),
            secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into(),
            region: "us-east-1".into(),
        };
        (
            credentials,
            Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap(),
        )
    }

    const EMPTY_SHA: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    /// "GET Object" from AWS's SigV4 documentation (Authorization header, single chunk).
    #[test]
    fn sigv4_matches_the_aws_get_object_example() {
        let (credentials, at) = aws_example();
        let auth = credentials
            .authorization(
                at,
                &SignedRequest {
                    method: "GET",
                    path: "/test.txt",
                    query: "",
                    headers: &[
                        ("Host", "examplebucket.s3.amazonaws.com"),
                        ("Range", "bytes=0-9"),
                        ("x-amz-content-sha256", EMPTY_SHA),
                        ("x-amz-date", "20130524T000000Z"),
                    ],
                    payload_sha256: EMPTY_SHA,
                },
            )
            .unwrap();
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request,\
             SignedHeaders=host;range;x-amz-content-sha256;x-amz-date,\
             Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
        );
    }

    /// "GET Bucket (List Objects)" from the same documentation: exercises the query string.
    #[test]
    fn sigv4_matches_the_aws_list_objects_example() {
        let (credentials, at) = aws_example();
        let query = canonical_query(&[("prefix", "J"), ("max-keys", "2")]);
        assert_eq!(query, "max-keys=2&prefix=J");
        let auth = credentials
            .authorization(
                at,
                &SignedRequest {
                    method: "GET",
                    path: "/",
                    query: &query,
                    headers: &[
                        ("host", "examplebucket.s3.amazonaws.com"),
                        ("x-amz-content-sha256", EMPTY_SHA),
                        ("x-amz-date", "20130524T000000Z"),
                    ],
                    payload_sha256: EMPTY_SHA,
                },
            )
            .unwrap();
        assert!(
            auth.ends_with(
                "Signature=34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
            ),
            "{auth}"
        );
    }

    #[test]
    fn xml_values_are_extracted_and_unescaped() {
        let xml = "<R><IsTruncated>true</IsTruncated><Contents><Key>a&amp;b/c&#233;&#x41;</Key>\
                   <Size>1</Size></Contents><Contents><Key>x&lt;y&gt;&quot;&apos;</Key></Contents>\
                   <NextContinuationToken>t&amp;1</NextContinuationToken></R>";
        let keys: Vec<_> = elements(xml, "Contents")
            .filter_map(|entry| element(entry, "Key"))
            .collect();
        assert_eq!(keys, ["a&b/céA", "x<y>\"'"]);
        assert_eq!(
            element(xml, "NextContinuationToken").as_deref(),
            Some("t&1")
        );
        assert_eq!(element(xml, "Missing"), None);
        assert_eq!(unescape("a & b &bogus; &#xZZ;"), "a & b &bogus; &#xZZ;");
    }

    /// A tiny in-memory S3: just enough of the API, verifying each request's signature.
    #[derive(Default)]
    struct FakeS3 {
        parts: Mutex<BTreeMap<usize, Vec<u8>>>,
        objects: Mutex<BTreeMap<String, Vec<u8>>>,
        /// `METHOD path?query` of every request, in order.
        calls: Mutex<Vec<String>>,
    }

    type Reply = (StatusCode, HeaderMap, String);

    fn reply(status: StatusCode, body: &str) -> Reply {
        (status, HeaderMap::new(), body.to_owned())
    }

    /// Whether the request's `Authorization` is the signature of what actually arrived.
    fn signed_correctly(method: &HttpMethod, uri: &Uri, headers: &HeaderMap, body: &[u8]) -> bool {
        let header = |name: &str| headers.get(name).unwrap().to_str().unwrap().to_owned();
        let amz_date = header("x-amz-date");
        let at = chrono::NaiveDateTime::parse_from_str(&amz_date, "%Y%m%dT%H%M%SZ")
            .unwrap()
            .and_utc();
        let (credentials, _) = aws_example();
        let credentials = Credentials {
            region: "auto".into(),
            ..credentials
        };
        let expected = credentials
            .authorization(
                at,
                &SignedRequest {
                    method: method.as_str(),
                    path: uri.path(),
                    query: uri.query().unwrap_or_default(),
                    headers: &[
                        ("host", &header("host")),
                        ("x-amz-content-sha256", &header("x-amz-content-sha256")),
                        ("x-amz-date", &amz_date),
                    ],
                    payload_sha256: &sha256_hex(body),
                },
            )
            .unwrap();
        header("authorization") == expected
    }

    impl FakeS3 {
        fn put_part(&self, query: &str, body: &[u8]) -> Reply {
            let number = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("partNumber="));
            let number: usize = number.unwrap().parse().unwrap();
            let _ = self.parts.lock().unwrap().insert(number, body.to_vec());
            let mut headers = HeaderMap::new();
            let _ = headers.insert("etag", format!("\"etag-{number}\"").parse().unwrap());
            (StatusCode::OK, headers, String::new())
        }

        fn complete(&self, path: &str, body: &[u8]) -> Reply {
            let text = String::from_utf8(body.to_vec()).unwrap();
            let parts = std::mem::take(&mut *self.parts.lock().unwrap());
            assert_eq!(text.matches("<Part>").count(), parts.len(), "{text}");
            assert!(text.contains("<ETag>\"etag-1\"</ETag>"), "{text}");
            let joined: Vec<u8> = parts.into_values().flatten().collect();
            let _ = self.objects.lock().unwrap().insert(path.to_owned(), joined);
            reply(StatusCode::OK, "<CompleteMultipartUploadResult/>")
        }
    }

    async fn s3_handler(
        State(fake): State<Arc<FakeS3>>,
        method: HttpMethod,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> Reply {
        if !signed_correctly(&method, &uri, &headers, &body) {
            return reply(StatusCode::FORBIDDEN, "<Error/>");
        }
        let query = uri.query().unwrap_or_default();
        let call = format!("{method} {}?{query}", uri.path());
        fake.calls.lock().unwrap().push(call);
        match (method.as_str(), query) {
            ("POST", "uploads=") => reply(StatusCode::OK, "<R><UploadId>up-1</UploadId></R>"),
            ("PUT", query) if query.contains("partNumber=") => fake.put_part(query, &body),
            ("POST", "uploadId=up-1") => fake.complete(uri.path(), &body),
            ("DELETE", "uploadId=up-1") => {
                fake.parts.lock().unwrap().clear();
                reply(StatusCode::NO_CONTENT, "")
            }
            ("DELETE", "") => {
                let _ = fake.objects.lock().unwrap().remove(uri.path());
                reply(StatusCode::NO_CONTENT, "")
            }
            // Two pages of one key each; the second is reached with the continuation token.
            ("GET", query) if query.contains("continuation-token=next%26page") => reply(
                StatusCode::OK,
                "<R><IsTruncated>false</IsTruncated><Contents><Key>p/b&amp;b</Key></Contents></R>",
            ),
            ("GET", query) if query.starts_with("list-type=2") => reply(
                StatusCode::OK,
                "<R><IsTruncated>true</IsTruncated><Contents><Key>p/a</Key></Contents>\
                 <NextContinuationToken>next&amp;page</NextContinuationToken></R>",
            ),
            _ => reply(
                StatusCode::BAD_REQUEST,
                &format!("<Error>{method} {uri}</Error>"),
            ),
        }
    }

    async fn fake_store(part_size: usize) -> (S3Store, Arc<FakeS3>) {
        let fake = Arc::new(FakeS3::default());
        let app = Router::new().fallback(s3_handler).with_state(fake.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap()
        }));
        let (credentials, _) = aws_example();
        let store = S3Store::new(
            &format!("http://{addr}/"),
            "bkt",
            "auto",
            &credentials.access_key,
            &credentials.secret_key,
        )
        .unwrap()
        .with_part_size(part_size);
        (store, fake)
    }

    /// Yields `data`, then fails like a dump that died.
    struct Dying(Vec<u8>);

    impl AsyncRead for Dying {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.0.is_empty() {
                return Poll::Ready(Err(io::Error::other("pg_dump failed")));
            }
            let take = self.0.len().min(buf.remaining());
            buf.put_slice(&self.0[..take]);
            let _ = self.0.drain(..take);
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn multipart_upload_list_and_delete_roundtrip() {
        let (store, fake) = fake_store(10).await;
        let data: Vec<u8> = (0..25).collect();
        let stored = store
            .put_stream("p/2026/a b.dump", Box::new(io::Cursor::new(data.clone())))
            .await
            .unwrap();
        assert_eq!(stored, 25);
        assert_eq!(
            fake.objects.lock().unwrap().get("/bkt/p/2026/a%20b.dump"),
            Some(&data),
            "three parts reassembled under the encoded key"
        );
        assert_eq!(store.list("p/").await.unwrap(), ["p/a", "p/b&b"]);
        store.delete("p/2026/a b.dump").await.unwrap();
        assert!(fake.objects.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_failing_dump_aborts_the_upload() {
        let (store, fake) = fake_store(10).await;
        let err = store
            .put_stream("p/x.dump", Box::new(Dying(vec![7; 15])))
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains("pg_dump failed"), "{err:#}");
        assert!(fake.objects.lock().unwrap().is_empty());
        let calls = fake.calls.lock().unwrap();
        assert_eq!(
            calls.last().unwrap(),
            "DELETE /bkt/p/x.dump?uploadId=up-1",
            "{calls:?}"
        );
        assert!(
            !calls
                .iter()
                .any(|call| call.contains("uploadId=up-1") && call.starts_with("POST"))
        );
    }

    #[tokio::test]
    async fn an_empty_dump_is_an_error() {
        let (store, fake) = fake_store(10).await;
        let err = store
            .put_stream("p/x.dump", Box::new(io::Cursor::new(Vec::new())))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
        assert!(
            fake.calls
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .starts_with("DELETE")
        );
    }

    #[tokio::test]
    async fn s3_errors_carry_status_and_message() {
        let (store, _) = fake_store(10).await;
        // The fake rejects what it can't route; the error names the call and the response.
        let err = store
            .send(Method::GET, "nothing", &[], Vec::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("400"), "{err}");
    }
}
