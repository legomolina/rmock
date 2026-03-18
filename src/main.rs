use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};
use axum::body::Body;
use axum::extract::{Request, State};
use axum::response::Response;
use axum::Router;
use axum::routing::any;
use notify::Watcher;
use serde::Deserialize;
use matchit::Router as MatchRouter;
use reqwest::Client;

const DEFAULT_PORT: u16 = 3000;
const DEFAULT_CONFIGURATION_FILE: &str = "./routes.yml";

#[derive(Debug, Deserialize)]
struct ConfigurationDefaultParams {
    proxy: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Deserialize)]
struct Configuration {
    default: Option<ConfigurationDefaultParams>,
    endpoints: Vec<Endpoint>,
}

struct ServerState {
    configuration: Configuration,
    router: MatchRouter<usize>,
}

#[derive(Debug, Deserialize)]
struct Endpoint {
    path: String,
    method: String,
    response: ResponseConfig,
}

#[derive(Debug, Deserialize)]
struct ResponseConfig {
    headers: Option<HashMap<String, String>>,
    status: Option<u16>,
    body: Option<String>,
    proxy: Option<String>,
}

fn load_config(config_path: &str) -> ServerState {
    let config_file = std::fs::read_to_string(config_path).expect("Failed to read config file");
    let configuration: Configuration = serde_yaml::from_str(&config_file).expect("Failed to parse config file");
    let mut router = MatchRouter::new();

    for (i, endpoint) in configuration.endpoints.iter().enumerate() {
        if let Err(e) = router.insert(&endpoint.path, i) {
            panic!("Error in route '{}': {}", endpoint.path, e);
        }
    }

    ServerState { configuration, router }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();

    let config_file_path = args.get(1)
        .filter(|arg| !arg.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or(DEFAULT_CONFIGURATION_FILE);

    let cli_port = args.iter()
        .position(|arg| arg == "--port")
        .and_then(|pos| args.get(pos + 1))
        .and_then(|p| p.parse::<u16>().ok());

    let server_state = load_config(config_file_path);
    let config_port = server_state.configuration.default.as_ref().and_then(|d| d.port);
    let port = cli_port.or(config_port).unwrap_or(DEFAULT_PORT);

    let shared_state = Arc::new(RwLock::new(server_state));
    let state_for_watcher = Arc::clone(&shared_state);
    let config_path_for_watcher = config_file_path.to_string();

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        match res {
            Ok(_) => {
                println!("Config file changed, reloading...");
                    let new_config = load_config(&config_path_for_watcher);
                    let mut w = state_for_watcher.write().unwrap();
                    *w = new_config;
                    println!("Config file reloaded");
            }
            Err(e) => println!("Error watching config file: {:?}", e),
        }
    }).unwrap();

    watcher.watch(Path::new(config_file_path), notify::RecursiveMode::NonRecursive).unwrap();

    let app = Router::new()
        .route("/{*path}", any(handler_mock))
        .with_state(shared_state);

    let bind_addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await.unwrap();
    println!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

async fn handler_mock(State(state): State<Arc<RwLock<ServerState>>>, req: Request) -> Response {
    let path = req.uri().path().to_string();
    let method = req.method().as_str().to_string();

    let (proxy_url, endpoint_config, default_proxy) = {
        let server_state = state.read().unwrap();
        let router = &server_state.router;
        let default_proxy = server_state.configuration.default.as_ref().and_then(|d| d.proxy.clone());

        match router.at(&path) {
            Ok(matched) => {
                let endpoint_index = *matched.value;
                let e = &server_state.configuration.endpoints[endpoint_index];

                let proxy_url = e.response.proxy.clone();
                let endpoint_config = if proxy_url.is_none() {
                    Some((
                        e.method.clone(),
                        e.response.status,
                        e.response.headers.clone(),
                        e.response.body.clone(),
                        matched.params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<Vec<_>>()
                    ))
                } else {
                    None
                };

                (proxy_url, endpoint_config, default_proxy)
            }
            Err(_) => (None, None, default_proxy)
        }
    };

    if let Some(proxy) = proxy_url {
        return proxy_handler(&proxy, req).await;
    }

    if let Some((endpoint_method, status, headers, body, params)) = endpoint_config {
        if endpoint_method.to_uppercase() != method.to_uppercase() {
            return Response::builder()
                .status(405)
                .body(Body::from("Method not allowed"))
                .unwrap();
        }

        let mut response_body = body.unwrap_or_default();

        if !response_body.is_empty() {
            for (key, value) in params {
                let placeholder = format!(".{}", key);
                response_body = response_body.replace(&placeholder, &value);
            }
        }

        let mut response = Response::builder()
            .status(status.unwrap_or(200));

        if let Some(hdrs) = headers {
            for (key, value) in hdrs {
                response = response.header(key, value);
            }
        }

        return response.body(Body::from(response_body)).unwrap();
    }

    if let Some(proxy) = default_proxy {
        return proxy_handler(&proxy, req).await;
    }

    Response::builder()
        .status(404)
        .body(Body::from("Route not defined in endpoints"))
        .unwrap()
}

async fn proxy_handler(url: &str, req: Request) -> Response {
    let client = Client::new();
    let method = req.method().clone();
    let headers = req.headers().clone();
    let body_data = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap_or_default();
    let mut proxy_req = client.request(method, url).body(body_data);

    for (key, value) in headers {
        proxy_req = proxy_req.header(key.unwrap(), value);
    }

    match proxy_req.send().await {
        Ok(res) => {
            let status = res.status();
            let headers = res.headers().clone();
            let body_bytes = res.bytes().await.unwrap_or_default();
            let mut response = Response::builder().status(status);

            for (key, value) in headers {
                if let Some(key) = key {
                    response = response.header(key, value);
                }
            }

            response.body(Body::from(body_bytes)).unwrap()
        }
        Err(_) => {
            Response::builder()
                .status(502)
                .body(Body::from("Proxy error: Destination unreachable"))
                .unwrap()
        }
    }
}