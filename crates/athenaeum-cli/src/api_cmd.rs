//! Full hosted API access, with discovery supplied by the server's OpenAPI catalogue.
use std::io::Read;
use std::path::PathBuf;

use athenaeum_core::portable::portable_error;
use athenaeum_core::Result;
use clap::{Args, Subcommand, ValueEnum};
use reqwest::Method;
use serde_json::{json, Value};

use crate::library::{LibraryClient, SERVER_ENV};
use crate::Outcome;

#[derive(Args)]
pub struct ApiArgs {
    /// Library origin; uses ATHENAEUM_LIBRARY_URL or the default Library when omitted.
    #[arg(long, global = true)]
    server: Option<String>,
    /// Public OpenAPI origin, if different from the Library proxy. Used by routes/schema only.
    #[arg(long, global = true)]
    schema_server: Option<String>,
    #[command(subcommand)]
    command: ApiCommand,
}

#[derive(Subcommand)]
enum ApiCommand {
    /// Print every API operation, its parameters, and request schema from the server.
    Routes {
        /// Filter operation path, method, summary, or tag.
        filter: Option<String>,
        /// Read a saved OpenAPI file instead of contacting a server.
        #[arg(long)]
        spec: Option<PathBuf>,
    },
    /// Fetch the full OpenAPI document.
    Schema,
    /// GET an API v1 route, e.g. repositories or user.
    Get(RequestArgs),
    /// POST JSON to an API v1 route.
    Post(RequestArgs),
    /// PATCH JSON on an API v1 route.
    Patch(RequestArgs),
    /// PUT JSON on an API v1 route.
    Put(RequestArgs),
    /// DELETE an API v1 resource (subject to the server's authorization).
    Delete(RequestArgs),
    /// Call an API v1 route with an explicit method.
    Request {
        #[arg(value_enum)]
        method: MethodArg,
        #[command(flatten)]
        request: RequestArgs,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum MethodArg {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}
impl MethodArg {
    fn method(self) -> Method {
        match self {
            Self::Get => Method::GET,
            Self::Post => Method::POST,
            Self::Put => Method::PUT,
            Self::Patch => Method::PATCH,
            Self::Delete => Method::DELETE,
            Self::Head => Method::HEAD,
            Self::Options => Method::OPTIONS,
        }
    }
}

#[derive(Args)]
struct RequestArgs {
    /// API-relative path (optionally prefixed /api/v1/). No absolute URLs.
    path: String,
    /// JSON request body. For credentials, prefer --body-file or --stdin.
    #[arg(long, conflicts_with_all = ["body_file", "stdin"])]
    body: Option<String>,
    #[arg(long, conflicts_with = "stdin")]
    body_file: Option<PathBuf>,
    /// Read a JSON body from standard input.
    #[arg(long)]
    stdin: bool,
    /// Query parameter as key=value. Repeatable; values are URL-encoded.
    #[arg(long, short = 'q')]
    query: Vec<String>,
    /// Show method, origin, path, and redacted body without making a request.
    #[arg(long)]
    dry_run: bool,
}

fn routes(spec: &Value, filter: Option<&str>) -> Result<Value> {
    let paths = spec
        .get("paths")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            portable_error("invalid_response", "OpenAPI document has no paths object")
        })?;
    let mut rows = Vec::new();
    for (path, item) in paths {
        let Some(operations) = item.as_object() else {
            continue;
        };
        for (method, operation) in operations {
            if !["get", "post", "put", "patch", "delete", "head", "options"]
                .contains(&method.as_str())
            {
                continue;
            }
            let row = json!({"method": method.to_uppercase(), "path": path,
                "summary": operation.get("summary"), "operationId": operation.get("operationId"),
                "tags": operation.get("tags"), "parameters": operation.get("parameters"),
                "requestBody": operation.get("requestBody"), "security": operation.get("security").or(spec.get("security")),
                "command": format!("wiki api {method} {}", path.trim_start_matches("/api/v1/"))});
            if filter.is_none_or(|f| row.to_string().to_lowercase().contains(&f.to_lowercase())) {
                rows.push(row);
            }
        }
    }
    Ok(
        json!({"operations": rows, "count": rows.len(), "schemas": spec.pointer("/components/schemas")}),
    )
}

pub fn run(json_out: bool, args: &ApiArgs) -> Result<Outcome> {
    let server = args
        .server
        .clone()
        .or_else(|| std::env::var(SERVER_ENV).ok())
        .unwrap_or_else(|| crate::setup_cmd::DEFAULT_SERVER.into());
    let client = LibraryClient::for_server(&server)?;
    // The hosted Library proxies /api/v1 but serves its SPA at /openapi.json.
    // Fetch the public catalogue from the API origin without sending credentials.
    let schema_server = args.schema_server.as_deref().unwrap_or_else(|| {
        if server.trim_end_matches('/') == crate::setup_cmd::DEFAULT_SERVER {
            "https://api.hanoryx.com"
        } else {
            &server
        }
    });
    let schema_client = LibraryClient::new(schema_server, None)?;
    let (method, request) = match &args.command {
        ApiCommand::Routes { filter, spec } => {
            let spec = match spec {
                Some(path) => serde_json::from_slice(&std::fs::read(path)?)?,
                None => schema_client.openapi()?,
            };
            let report = routes(&spec, filter.as_deref())?;
            crate::emit(json_out, &report, || {
                for row in report["operations"].as_array().unwrap() {
                    println!(
                        "{:<7} {:<65} {}",
                        row["method"].as_str().unwrap(),
                        row["path"].as_str().unwrap(),
                        row["summary"].as_str().unwrap_or_default()
                    );
                }
                Ok(())
            })?;
            return Ok(Outcome::Ok);
        }
        ApiCommand::Schema => {
            let spec = schema_client.openapi()?;
            println!("{}", serde_json::to_string_pretty(&spec)?);
            return Ok(Outcome::Ok);
        }
        ApiCommand::Get(request) => (Method::GET, request),
        ApiCommand::Post(request) => (Method::POST, request),
        ApiCommand::Patch(request) => (Method::PATCH, request),
        ApiCommand::Put(request) => (Method::PUT, request),
        ApiCommand::Delete(request) => (Method::DELETE, request),
        ApiCommand::Request { method, request } => (method.method(), request),
    };
    let body = if let Some(text) = &request.body {
        Some(serde_json::from_str::<Value>(text)?)
    } else if let Some(path) = &request.body_file {
        Some(serde_json::from_slice(&std::fs::read(path)?)?)
    } else if request.stdin {
        let mut text = String::new();
        std::io::stdin()
            .take(16 * 1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(portable_error("invalid_request", "Body exceeds 16 MiB"));
        }
        Some(serde_json::from_str(&text)?)
    } else {
        None
    };
    let query = request
        .query
        .iter()
        .map(|part| {
            part.split_once('=')
                .filter(|(key, _)| !key.is_empty())
                .map(|(k, v)| (k.into(), v.into()))
                .ok_or_else(|| portable_error("invalid_request", "--query needs key=value"))
        })
        .collect::<Result<Vec<(String, String)>>>()?;
    let report = if request.dry_run {
        json!({"dryRun": true, "method": method.as_str(), "server": client.server(), "path": request.path,
            "query": crate::config_cmd::redact(&query.iter().map(|(k,v)| (k.clone(), json!(v))).collect::<serde_json::Map<_,_>>().into()),
            "body": body.as_ref().map(crate::config_cmd::redact)})
    } else {
        client.api_request(method, &request.path, &query, body.as_ref())?
    };
    crate::emit(json_out, &report, || {
        println!("{}", serde_json::to_string_pretty(&report)?);
        Ok(())
    })?;
    Ok(Outcome::Ok)
}
