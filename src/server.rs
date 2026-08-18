use std::collections::BTreeMap;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::{application, changes, id_cache};

const INDEX_HTML: &[u8] = include_bytes!("../ui/index.html");
const STYLE_CSS: &[u8] = include_bytes!("../ui/style.css");
const APP_JS: &[u8] = include_bytes!("../ui/app.js");

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DashboardConfig {
    pub base: String,
    pub head: String,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            base: "HEAD~1".to_string(),
            head: "HEAD".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

#[derive(Serialize)]
struct FeatureHistory {
    schema_version: u32,
    git_ref: String,
    features: Vec<FeatureHistoryEntry>,
}

#[derive(Serialize)]
struct FeatureHistoryEntry {
    id: String,
    path: String,
    tree_sha: String,
    change_events: Vec<String>,
}

fn response(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> HttpResponse {
    HttpResponse {
        status,
        content_type,
        body: body.into(),
    }
}

fn error_response(status: u16, message: impl ToString) -> HttpResponse {
    response(status, "text/plain; charset=utf-8", message.to_string())
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_component(value: &str) -> Result<String, &'static str> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex_value(bytes[index + 1]).ok_or("invalid percent encoding")?;
                let low = hex_value(bytes[index + 2]).ok_or("invalid percent encoding")?;
                output.push(high * 16 + low);
                index += 3;
            }
            b'%' => return Err("invalid percent encoding"),
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_| "query parameter is not UTF-8")
}

fn query(target: &str) -> Result<BTreeMap<String, String>, &'static str> {
    let Some((_, raw)) = target.split_once('?') else {
        return Ok(BTreeMap::new());
    };
    raw.split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (key, value) = part.split_once('=').unwrap_or((part, ""));
            Ok((decode_component(key)?, decode_component(value)?))
        })
        .collect()
}

fn feature_history(root: &Path, git_ref: &str) -> io::Result<FeatureHistory> {
    let mut events_by_feature: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut paths: Vec<_> = fs::read_dir(
        root.join(crate::project_root::MARKHARNESS_DIR)
            .join("changes"),
    )
    .into_iter()
    .flatten()
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|path| path.extension().and_then(|value| value.to_str()) == Some("yaml"))
    .collect();
    paths.sort();
    for path in paths {
        let events: Vec<changes::ChangeEvent> = serde_yaml_ng::from_str(&fs::read_to_string(path)?)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        for event in events {
            events_by_feature
                .entry(event.feature_id)
                .or_default()
                .push(event.event_id);
        }
    }
    let features = id_cache::resolve_feature_versions(root, git_ref, true)?
        .into_iter()
        .map(|feature| FeatureHistoryEntry {
            change_events: events_by_feature.remove(&feature.id).unwrap_or_default(),
            id: feature.id,
            path: feature.path,
            tree_sha: feature.tree_sha,
        })
        .collect();
    Ok(FeatureHistory {
        schema_version: 1,
        git_ref: git_ref.to_string(),
        features,
    })
}

pub fn handle_request(
    root: &Path,
    method: &str,
    target: &str,
    config: &DashboardConfig,
) -> HttpResponse {
    if method != "GET" {
        return error_response(405, "method not allowed");
    }
    let path = target.split('?').next().unwrap_or(target);
    match path {
        "/" | "/index.html" => response(200, "text/html; charset=utf-8", INDEX_HTML),
        "/assets/style.css" => response(200, "text/css; charset=utf-8", STYLE_CSS),
        "/assets/app.js" => response(200, "text/javascript; charset=utf-8", APP_JS),
        "/api/config" => match serde_json::to_vec(config) {
            Ok(body) => response(200, "application/json; charset=utf-8", body),
            Err(error) => error_response(500, error),
        },
        "/api/plan" => {
            let params = match query(target) {
                Ok(params) => params,
                Err(error) => return error_response(400, error),
            };
            let base = params.get("base").unwrap_or(&config.base);
            let head = params.get("head").unwrap_or(&config.head);
            match application::build_verification_plan_value(root, base, head, &[]) {
                Ok(plan) => match serde_json::to_vec(&plan) {
                    Ok(body) => response(200, "application/json; charset=utf-8", body),
                    Err(error) => error_response(500, error),
                },
                Err(error) => error_response(422, error),
            }
        }
        "/api/features" => {
            let params = match query(target) {
                Ok(params) => params,
                Err(error) => return error_response(400, error),
            };
            let git_ref = params.get("ref").unwrap_or(&config.head);
            match feature_history(root, git_ref)
                .and_then(|history| serde_json::to_vec(&history).map_err(io::Error::other))
            {
                Ok(body) => response(200, "application/json; charset=utf-8", body),
                Err(error) => error_response(422, error),
            }
        }
        _ => error_response(404, "not found"),
    }
}

fn serve_connection(
    stream: &mut TcpStream,
    root: &Path,
    config: &DashboardConfig,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut request_line = String::new();
    BufReader::new(&mut *stream).read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("");
    let target = parts.next().unwrap_or("");
    let result = handle_request(root, method, target, config);
    let reason = match result.status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        422 => "Unprocessable Content",
        _ => "Internal Server Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nConnection: close\r\n\r\n",
        result.status,
        reason,
        result.content_type,
        result.body.len()
    )?;
    stream.write_all(&result.body)
}

pub fn serve(root: &Path, port: u16, config: DashboardConfig) -> io::Result<()> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let listener = TcpListener::bind(address)?;
    println!("Markharness dashboard: http://{address}");
    println!("Read-only server; press Ctrl+C to stop.");
    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                if let Err(error) = serve_connection(&mut stream, root, &config) {
                    eprintln!("dashboard request error: {error}");
                }
            }
            Err(error) => eprintln!("dashboard connection error: {error}"),
        }
    }
    Ok(())
}
