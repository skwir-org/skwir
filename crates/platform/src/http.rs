//! The HTTP client of the plugins that make requests (ADR-020: map
//! tiles; ADR-142: whether a new version is out), moved here from
//! `map-tiles` once a second plugin needed it (the `http` feature): HTTP/2
//! where the server speaks it, negotiated in TLS, so every tile of a view
//! travels at once on one connection; HTTP/1.1 otherwise, on a few
//! kept-alive connections. hyper and rustls as gpui already links them,
//! with the system's certificates (`rustls-platform-verifier`), run by the
//! caller's own tokio runtime. A utility, not a contract: the core has no
//! network layer, and only a plugin declaring `network` uses it.
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
use skwir_core::words;

/// A request takes at most this long, from its call to the last byte (the
/// wait for another request's connection to the server and a second try
/// included), unless the client says otherwise (`Http::limited`). The
/// requests of a burst to a server not yet connected to share it with the
/// first one's connection: on a slow link, a view's other map tiles have
/// what the first tile left them, and a server that never answers fails
/// them all at once, not one after another.
const DEADLINE: Duration = Duration::from_secs(15);
/// An answer larger than this is refused, unless the client says
/// otherwise: a tile larger is not a tile.
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
    /// The longest a request takes, and the largest answer read.
    deadline: Duration,
    max_body: usize,
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
            deadline: DEADLINE,
            max_body: MAX_BODY,
            servers: std::sync::Mutex::default(),
        })
    }

    /// The same client, a request taking at most `deadline` and an answer
    /// read up to `max_body` bytes: a check for a new version waits less
    /// and reads less than a tile does.
    #[must_use]
    pub fn limited(mut self, deadline: Duration, max_body: usize) -> Http {
        self.deadline = deadline;
        self.max_body = max_body;
        self
    }

    /// GET `url` with `headers`, which replace skwir's User-Agent when
    /// they name one, within the client's deadline. A connection that
    /// fails is tried once more: a server closes idle ones, and the first
    /// request on one it closed fails. The first request to a server not
    /// connected to goes alone (`servers`). Why it got no answer is said
    /// in Skwir's words; what the network library said is logged.
    ///
    /// The deadline is a timer of the caller's runtime, which cannot fire
    /// while the system's certificate check runs: on Windows and macOS
    /// that check may ask the network itself whether a certificate was
    /// revoked (Windows waits up to 10 s for it), and a request then takes
    /// that much longer. On Linux the check reads only this machine.
    pub async fn get(&self, url: &str, headers: &[(String, String)]) -> anyhow::Result<Answer> {
        tokio::time::timeout(self.deadline, self.get_gated(url, headers))
            .await
            .map_err(|_| {
                anyhow::anyhow!(words!(
                    http_timed_out = "the server did not answer in time",
                    note = "Why a request failed: the server it was sent to was waited for long enough."
                ))
            })?
    }

    async fn get_gated(&self, url: &str, headers: &[(String, String)]) -> anyhow::Result<Answer> {
        let uri: hyper::Uri = url.parse().context(words!(
            http_not_address = "not an address",
            note = "Why a request was not made: what it was to be sent to is no web address."
        ))?;
        let server = format!(
            "{}://{}",
            uri.scheme_str().unwrap_or(""),
            uri.authority().map_or("", |a| a.as_str())
        );
        let gate = Arc::clone(
            self.servers
                .lock()
                .map_err(|_| {
                    anyhow::anyhow!(words!(
                        http_stopped = "the requests stopped",
                        note = "Why a request was not made: what makes them broke down."
                    ))
                })?
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
        let failed = match self.get_once(url, headers).await {
            Err(Failure::Connection(_)) => {
                tracing::debug!("http: asking again after a failed connection");
                self.get_once(url, headers).await
            }
            result => result,
        };
        failed.map_err(|failure| match failure {
            Failure::Connection(error) => {
                tracing::debug!("http: {error:#}");
                anyhow::Error::new(unreached(&error))
            }
            Failure::Other(error) => error,
        })
    }

    async fn get_once(&self, url: &str, headers: &[(String, String)]) -> Result<Answer, Failure> {
        let mut request = hyper::Request::get(url)
            .body(Empty::<Bytes>::new())
            .context(words!(http_not_address = "not an address"))
            .map_err(Failure::Other)?;
        let map = request.headers_mut();
        map.insert(USER_AGENT, self.user_agent.clone());
        for (name, value) in headers {
            let name = HeaderName::from_bytes(name.as_bytes()).map_err(Failure::other)?;
            map.insert(name, HeaderValue::from_str(value).map_err(Failure::other)?);
        }
        let response = self
            .client
            .request(request)
            .await
            .map_err(Failure::Connection)?;
        let (parts, body) = response.into_parts();
        let body = Limited::new(body, self.max_body)
            .collect()
            .await
            .map_err(|error| {
                tracing::debug!("http: the answer was not read whole: {error}");
                Failure::Other(anyhow::anyhow!(
                    if error.is::<http_body_util::LengthLimitError>() {
                        words!(
                            http_too_large = "the server's answer is larger than expected",
                            note = "Why a request failed: what the server sent is larger than what is read of it."
                        )
                    } else {
                        broke_off()
                    }
                ))
            })?
            .to_bytes()
            .to_vec();
        Ok(Answer {
            status: parts.status.as_u16(),
            headers: parts.headers,
            body,
            version: parts.version,
        })
    }
}

/// Why a request got no answer: its connection (none made, or one lost
/// before the answer), tried once more; or anything else.
enum Failure {
    Connection(hyper_util::client::legacy::Error),
    Other(anyhow::Error),
}

impl Failure {
    fn other(error: impl Into<anyhow::Error>) -> Failure {
        Failure::Other(error.into())
    }
}

/// Why no connection to the server answered, in Skwir's words, from what
/// the network library and the system said (`error` and its causes).
fn unreached(error: &hyper_util::client::legacy::Error) -> skwir_core::words::Words {
    use std::io::ErrorKind as K;
    let not_secure = || {
        words!(
            http_not_secure = "no secure connection to the server could be made",
            note = "Why a request failed: the server's certificate, or its encryption, was not accepted."
        )
    };
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(link) = cause {
        if link.is::<rustls::Error>() {
            return not_secure();
        }
        let io = link.downcast_ref::<std::io::Error>();
        match io.map(std::io::Error::kind) {
            Some(K::ConnectionRefused) => {
                return words!(
                    http_refused = "the server refused the connection",
                    note = "Why a request failed: nothing listens at the server's address."
                );
            }
            Some(K::NetworkUnreachable | K::HostUnreachable | K::NetworkDown) => {
                return words!(
                    http_no_route = "the network does not reach the server",
                    note = "Why a request failed: no network, or none that leads to the server."
                );
            }
            _ => {}
        }
        // An io error's `source` is its inner error's own: the inner one
        // (a TLS error the connector wrapped) is the next link.
        cause = match io.and_then(std::io::Error::get_ref) {
            Some(inner) => Some(inner),
            None => link.source(),
        };
    }
    if error.is_connect() {
        words!(
            http_unreachable = "the server could not be reached",
            note = "Why a request failed: no connection to it could be made (its name not found, for one)."
        )
    } else {
        broke_off()
    }
}

/// A connection lost before the whole answer came.
fn broke_off() -> skwir_core::words::Words {
    words!(
        http_broke_off = "the connection to the server broke off",
        note =
            "Why a request failed: the connection was lost before the server's whole answer came."
    )
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
        use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
        let certs: Vec<_> = CertificateDer::pem_slice_iter(include_bytes!("testdata/server.pem"))
            .collect::<Result<_, _>>()
            .unwrap();
        let key = PrivateKeyDer::from_pem_slice(include_bytes!("testdata/server.key")).unwrap();
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
        use rustls::pki_types::{CertificateDer, pem::PemObject};
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(include_bytes!("testdata/ca.pem")) {
            roots.add(cert.unwrap()).unwrap();
        }
        let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        Http::new(tls, "skwir/test").unwrap()
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
            assert_eq!(answer.body, b"skwir/test");
        }
        let took = started.elapsed();
        // Twelve tiles in about one server delay, not twelve (or six, on
        // two connections), and all on one connection.
        assert!(took < delay * 4, "{took:?}");
        assert_eq!(h2.load(Ordering::SeqCst), 12);
        assert_eq!(connections.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_server_header_replaces_skwir_s_user_agent() {
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

    /// A client limited to fewer bytes than the answer refuses it, and one
    /// limited to less time than the server takes gives up.
    #[tokio::test]
    async fn a_limited_client_reads_no_more_and_waits_no_longer() {
        let (address, _, _) = server(Duration::ZERO).await;
        let small = client().limited(DEADLINE, 4);
        let refused = small.get(&format!("{address}/a"), &[]).await;
        assert_eq!(said(refused), "the server's answer is larger than expected");
        let enough = client().limited(DEADLINE, 64);
        assert_eq!(
            enough.get(&format!("{address}/a"), &[]).await.unwrap().body,
            b"skwir/test"
        );
        let (slow, _, _) = server(Duration::from_secs(5)).await;
        let impatient = client().limited(Duration::from_millis(100), 64);
        let started = std::time::Instant::now();
        assert!(impatient.get(&format!("{slow}/a"), &[]).await.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    /// The deadline is the whole request's: the wait for another
    /// request's connection to the same server counts.
    #[tokio::test]
    async fn the_deadline_counts_the_wait_for_another_request() {
        let (address, _, _) = server(Duration::from_millis(800)).await;
        let http = Arc::new(client().limited(Duration::from_millis(1200), 64));
        let first = {
            let (http, url) = (Arc::clone(&http), format!("{address}/1"));
            tokio::spawn(async move { http.get(&url, &[]).await })
        };
        // The first goes alone, the second waits for its connection.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let second = http.get(&format!("{address}/2"), &[]).await;
        assert!(first.await.unwrap().is_ok());
        assert!(second.is_err(), "800 ms waited, then 800 more: past 1200");
    }

    #[tokio::test]
    async fn a_certificate_the_client_does_not_trust_is_refused() {
        if skwir_core::system::under_wine("Windows' certificate checks") {
            return;
        }
        let (address, _, _) = server(Duration::ZERO).await;
        // The system's authorities never signed the test's certificate.
        let http = Http::system("skwir/test").unwrap();
        assert_eq!(
            said(http.get(&format!("{address}/0/0/0.png"), &[]).await),
            "no secure connection to the server could be made"
        );
    }

    /// Why no answer came is said in Skwir's words, never the network
    /// library's: a server that refuses the connection says so.
    #[tokio::test]
    async fn a_refused_connection_is_said_in_skwirs_words() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let http = client();
        assert_eq!(
            said(http.get(&format!("http://127.0.0.1:{port}/a"), &[]).await),
            "the server refused the connection"
        );
    }

    /// What a failed request says, in English.
    fn said(answer: anyhow::Result<Answer>) -> String {
        match answer {
            Ok(answer) => panic!("answered {}", answer.status),
            Err(error) => skwir_core::words::Words::of_error(error).english(),
        }
    }
}
