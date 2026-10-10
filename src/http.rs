//! The HTTP layer under the GitHub client, behind a trait so that tests can
//! replay recorded answers without the network.

use std::sync::Mutex;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Sends one request; `Err` means no HTTP answer at all (DNS, TLS, timeout).
pub trait Transport: Send + Sync {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, String>;
}

/// The real transport: rustls, a 60-second budget per request, every HTTP
/// status returned as an answer (never as an error).
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .user_agent(concat!("spl-asdlc/", env!("CARGO_PKG_VERSION")))
            .build();
        UreqTransport {
            agent: ureq::Agent::new_with_config(config),
        }
    }
}

/// Bodies above this size are refused rather than read.
const MAX_BODY: u64 = 64 * 1024 * 1024;

impl Transport for UreqTransport {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        let mut builder = ureq::http::Request::builder()
            .method(request.method)
            .uri(&request.url);
        for (k, v) in &request.headers {
            builder = builder.header(k, v);
        }
        let response = match request.body {
            Some(body) => {
                let req = builder.body(body).map_err(|e| e.to_string())?;
                self.agent.run(req)
            }
            None => {
                let req = builder.body(()).map_err(|e| e.to_string())?;
                self.agent.run(req)
            }
        }
        .map_err(|e| e.to_string())?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.to_string())))
            .collect();
        let body = response
            .into_body()
            .with_config()
            .limit(MAX_BODY)
            .read_to_vec()
            .map_err(|e| e.to_string())?;
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

/// A transport that answers from a script and records what was asked.
/// Requests are matched in order of registration by method and URL prefix;
/// a request nothing matches gets a 599 answer naming it.
#[derive(Default)]
pub struct ScriptedTransport {
    routes: Mutex<Vec<Route>>,
    pub log: Mutex<Vec<HttpRequest>>,
}

struct Route {
    method: &'static str,
    url_prefix: String,
    /// The request body must contain this text (GraphQL routes by query).
    body_contains: Option<String>,
    responses: Vec<HttpResponse>,
}

impl ScriptedTransport {
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer `method url…` with `status` and a JSON `body`; registering the
    /// same route again queues a further answer (the last one repeats).
    pub fn on(&self, method: &'static str, url_prefix: &str, status: u16, body: &str) -> &Self {
        self.on_with_headers(method, url_prefix, status, body, &[])
    }

    pub fn on_with_headers(
        &self,
        method: &'static str,
        url_prefix: &str,
        status: u16,
        body: &str,
        headers: &[(&str, &str)],
    ) -> &Self {
        let response = HttpResponse {
            status,
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            body: body.as_bytes().to_vec(),
        };
        self.add(method, url_prefix, None, response);
        self
    }

    /// Answer `method url…` with `status` and a binary `body` (an archive).
    pub fn on_bytes(
        &self,
        method: &'static str,
        url_prefix: &str,
        status: u16,
        body: &[u8],
    ) -> &Self {
        let response = HttpResponse {
            status,
            headers: Vec::new(),
            body: body.to_vec(),
        };
        self.add(method, url_prefix, None, response);
        self
    }

    /// Answer requests whose body contains `body_contains` (a GraphQL query
    /// by one of its words); preferred over a route without a body match.
    pub fn on_body(
        &self,
        method: &'static str,
        url_prefix: &str,
        body_contains: &str,
        status: u16,
        body: &str,
    ) -> &Self {
        let response = HttpResponse {
            status,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        };
        self.add(
            method,
            url_prefix,
            Some(body_contains.to_string()),
            response,
        );
        self
    }

    fn add(
        &self,
        method: &'static str,
        url_prefix: &str,
        body_contains: Option<String>,
        response: HttpResponse,
    ) {
        let mut routes = self.routes.lock().unwrap();
        if let Some(r) = routes.iter_mut().find(|r| {
            r.method == method && r.url_prefix == url_prefix && r.body_contains == body_contains
        }) {
            r.responses.push(response);
        } else {
            routes.push(Route {
                method,
                url_prefix: url_prefix.to_string(),
                body_contains,
                responses: vec![response],
            });
        }
    }

    /// The requests sent so far, as `METHOD url`.
    pub fn calls(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|r| format!("{} {}", r.method, r.url))
            .collect()
    }

    pub fn bodies(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|r| String::from_utf8_lossy(r.body.as_deref().unwrap_or_default()).to_string())
            .collect()
    }
}

impl Transport for ScriptedTransport {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, String> {
        self.log.lock().unwrap().push(request.clone());
        let mut routes = self.routes.lock().unwrap();
        // The longest matching prefix wins, so a specific route can sit
        // beside a general one.
        let body = String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).to_string();
        let best = routes
            .iter_mut()
            .filter(|r| r.method == request.method && request.url.starts_with(&r.url_prefix))
            .filter(|r| {
                r.body_contains
                    .as_ref()
                    .is_none_or(|b| body.contains(b.as_str()))
            })
            .max_by_key(|r| {
                (
                    r.body_contains.as_ref().map(|b| b.len()).unwrap_or(0),
                    r.url_prefix.len(),
                )
            });
        match best {
            Some(route) => {
                if route.responses.len() > 1 {
                    Ok(route.responses.remove(0))
                } else {
                    Ok(route.responses[0].clone())
                }
            }
            None => Ok(HttpResponse {
                status: 599,
                headers: Vec::new(),
                body: format!("no scripted answer for {} {}", request.method, request.url)
                    .into_bytes(),
            }),
        }
    }
}
