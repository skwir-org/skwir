//! The HTTP client of tile downloads (ADR-020): HTTP/2 where the server
//! speaks it, negotiated in TLS, so every tile of a view travels at once on
//! one connection; HTTP/1.1 otherwise, on a few kept-alive connections.
//! hyper and rustls as gpui already links them, with the system's
//! certificates (`rustls-platform-verifier`), run by the download thread's
//! own tokio runtime.
//!
//! The TLS configuration is a parameter: the application passes the
//! system's, and only a test passes another, with the one certificate
//! authority it trusts. Nothing at runtime can be told to trust more.
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use bytes::Bytes;
use http_body_util::{BodyExt, Empty, Limited};
use hyper::header::{HeaderName, HeaderValue, USER_AGENT};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use hyper_util::rt::TokioExecutor;

/// A request takes at most this long, from connecting to the last byte.
const DEADLINE: Duration = Duration::from_secs(15);
/// A tile larger than this is not a tile.
const MAX_BODY: usize = 4 * 1024 * 1024;
/// How long an idle connection is kept.
const IDLE: Duration = Duration::from_secs(60);
/// A server answered within this: its connection is surely still open.
const WARM: Duration = Duration::from_secs(50);

/// What a server answered.
pub struct Answer {
    pub status: u16,
    pub headers: hyper::HeaderMap,
    pub body: Vec<u8>,
    /// The protocol the answer came in (`HTTP/2.0`, `HTTP/1.1`).
    pub version: hyper::Version,
}

pub struct Http {
    client: Client<hyper_rustls::HttpsConnector<HttpConnector>, Empty<Bytes>>,
    user_agent: HeaderValue,
    /// By server (scheme, host, port): when it last answered. Whether it
    /// speaks HTTP/2 is only learnt once a connection is open, so a burst
    /// of requests to a server not connected to would open a connection
    /// each; the first goes alone and the others wait for its connection,
    /// then share it (HTTP/2) or open their own (HTTP/1.1).
    servers: std::sync::Mutex<
        std::collections::HashMap<String, Arc<tokio::sync::Mutex<Option<std::time::Instant>>>>,
    >,
}

impl Http {
    /// A client that trusts what the system trusts.
    pub fn system(user_agent: &str) -> anyhow::Result<Http> {
        use rustls_platform_verifier::BuilderVerifierExt as _;
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()?
            .with_platform_verifier()?
            .with_no_client_auth();
        Http::new(tls, user_agent)
    }

    /// A client over `tls`, which must be the system's outside tests.
    pub fn new(tls: rustls::ClientConfig, user_agent: &str) -> anyhow::Result<Http> {
        let connector = hyper_rustls::HttpsConnectorBuilder::new()
            .with_tls_config(tls)
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();
        let client = Client::builder(TokioExecutor::new())
            .pool_idle_timeout(IDLE)
            .build(connector);
        Ok(Http {
            client,
            user_agent: HeaderValue::from_str(user_agent)?,
            servers: std::sync::Mutex::default(),
        })
    }

    /// GET `url` with `headers`, which replace bibli's User-Agent when
    /// they name one. A connection that fails is tried once more: a server
    /// closes idle ones, and the first request on one it closed fails.
    /// The first request to a server not connected to goes alone
    /// (`servers`).
    pub async fn get(&self, url: &str, headers: &[(String, String)]) -> anyhow::Result<Answer> {
        let uri: hyper::Uri = url.parse().context("not an address")?;
        let server = format!(
            "{}://{}",
            uri.scheme_str().unwrap_or(""),
            uri.authority().map_or("", |a| a.as_str())
        );
        let gate = Arc::clone(
            self.servers
                .lock()
                .map_err(|_| anyhow::anyhow!("the downloads stopped"))?
                .entry(server)
                .or_default(),
        );
        let mut last = gate.lock().await;
        if last.is_none_or(|at| at.elapsed() > WARM) {
            // Alone: the others wait here for this connection.
            let answer = self.get_retrying(url, headers).await;
            if answer.is_ok() {
                *last = Some(std::time::Instant::now());
            }
            return answer;
        }
        drop(last);
        let answer = self.get_retrying(url, headers).await;
        if answer.is_ok() {
            *gate.lock().await = Some(std::time::Instant::now());
        }
        answer
    }

    async fn get_retrying(
        &self,
        url: &str,
        headers: &[(String, String)],
    ) -> anyhow::Result<Answer> {
        match self.get_once(url, headers).await {
            Err(error) if error.is::<hyper_util::client::legacy::Error>() => {
                tracing::debug!("map tiles: asking again after a failed connection");
                self.get_once(url, headers).await
            }
            result => result,
        }
    }

    async fn get_once(&self, url: &str, headers: &[(String, String)]) -> anyhow::Result<Answer> {
        let mut request = hyper::Request::get(url)
            .body(Empty::<Bytes>::new())
            .context("not an address")?;
        let map = request.headers_mut();
        map.insert(USER_AGENT, self.user_agent.clone());
        for (name, value) in headers {
            map.insert(
                HeaderName::from_bytes(name.as_bytes())?,
                HeaderValue::from_str(value)?,
            );
        }
        tokio::time::timeout(DEADLINE, async {
            let response = self.client.request(request).await?;
            let (parts, body) = response.into_parts();
            let body = Limited::new(body, MAX_BODY)
                .collect()
                .await
                .map_err(|error| anyhow::anyhow!("the tile could not be read: {error}"))?
                .to_bytes()
                .to_vec();
            Ok(Answer {
                status: parts.status.as_u16(),
                headers: parts.headers,
                body,
                version: parts.version,
            })
        })
        .await
        .map_err(|_| anyhow::anyhow!("the tile server did not answer in time"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use hyper::service::service_fn;
    use tokio::net::TcpListener;

    /// A local HTTPS server speaking HTTP/2 (and HTTP/1.1), with the test
    /// certificate: each answer after `delay`, counting connections and the
    /// requests that came in HTTP/2.
    async fn server(delay: Duration) -> (String, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let certs: Vec<_> = rustls_pemfile::certs(&mut &include_bytes!("testdata/server.pem")[..])
            .collect::<Result<_, _>>()
            .unwrap();
        let key = rustls_pemfile::private_key(&mut &include_bytes!("testdata/server.key")[..])
            .unwrap()
            .unwrap();
        let mut tls = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .unwrap();
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(tls));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!(
            "https://localhost:{}",
            listener.local_addr().unwrap().port()
        );
        let (connections, h2) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (counted, h2_counted) = (Arc::clone(&connections), Arc::clone(&h2));
        tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                counted.fetch_add(1, Ordering::SeqCst);
                let (acceptor, h2) = (acceptor.clone(), Arc::clone(&h2_counted));
                tokio::spawn(async move {
                    let stream = acceptor.accept(stream).await.unwrap();
                    let service =
                        service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                            let h2 = Arc::clone(&h2);
                            async move {
                                if request.version() == hyper::Version::HTTP_2 {
                                    h2.fetch_add(1, Ordering::SeqCst);
                                }
                                tokio::time::sleep(delay).await;
                                let agent = request
                                    .headers()
                                    .get("user-agent")
                                    .map(|v| v.to_str().unwrap().to_owned())
                                    .unwrap_or_default();
                                Ok::<_, std::convert::Infallible>(hyper::Response::new(
                                    http_body_util::Full::new(Bytes::from(agent)),
                                ))
                            }
                        });
                    let _ = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
                });
            }
        });
        (address, connections, h2)
    }

    /// A client trusting the test's certificate authority, and only it.
    fn client() -> Http {
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut &include_bytes!("testdata/ca.pem")[..]) {
            roots.add(cert.unwrap()).unwrap();
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        Http::new(tls, "bibli/test").unwrap()
    }

    #[tokio::test]
    async fn a_view_of_tiles_travels_at_once_on_one_connection() {
        let delay = Duration::from_millis(150);
        let (address, connections, h2) = server(delay).await;
        let http = Arc::new(client());
        let started = std::time::Instant::now();
        let requests: Vec<_> = (0..12)
            .map(|i| {
                let (http, url) = (Arc::clone(&http), format!("{address}/3/{i}/1.png"));
                tokio::spawn(async move { http.get(&url, &[]).await.unwrap() })
            })
            .collect();
        for request in requests {
            let answer = request.await.unwrap();
            assert_eq!(answer.status, 200);
            assert_eq!(answer.version, hyper::Version::HTTP_2);
            assert_eq!(answer.body, b"bibli/test");
        }
        let took = started.elapsed();
        // Twelve tiles in about one server delay, not twelve (or six, on
        // two connections), and all on one connection.
        assert!(took < delay * 4, "{took:?}");
        assert_eq!(h2.load(Ordering::SeqCst), 12);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_server_header_replaces_bibli_s_user_agent() {
        let (address, _, _) = server(Duration::ZERO).await;
        let answer = client()
            .get(
                &format!("{address}/0/0/0.png"),
                &[("User-Agent".into(), "Mine/1".into())],
            )
            .await
            .unwrap();
        assert_eq!(answer.body, b"Mine/1");
    }

    #[tokio::test]
    async fn a_certificate_the_client_does_not_trust_is_refused() {
        if bibli_core::system::under_wine("Windows' certificate checks") {
            return;
        }
        let (address, _, _) = server(Duration::ZERO).await;
        // The system's authorities never signed the test's certificate.
        let http = Http::system("bibli/test").unwrap();
        assert!(
            http.get(&format!("{address}/0/0/0.png"), &[])
                .await
                .is_err()
        );
    }
}
