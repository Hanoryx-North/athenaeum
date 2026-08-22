//! Web and desktop-search backends.
//!
//! Ported from llm_wiki (https://github.com/nashsu/llm_wiki), GPL-3.0, © Yong Su.
//! Modified for Athenaeum.
//!
//! Extracted from llm_wiki's `agent/tools.rs`, where these sat alongside the agent
//! loop. They are not agent code: they issue one HTTP request to a search provider
//! and normalize the reply. Athenaeum needs them for deep research and source
//! discovery; the agent loop they used to share a file with belongs to
//! DeepSeek Harness and is not ported. See `docs/porting-athenaeum.md`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::external_search::file_url_for_path;

const WEB_SEARCH_TIMEOUT_SECS: u64 = 30;
const DEFAULT_ANYTXT_ENDPOINT: &str = "http://127.0.0.1:9920";
const DEFAULT_ANYTXT_LIMIT: usize = 20;
const ANYTXT_LAST_MODIFY_END: i64 = 2_147_483_647;

/// One search hit, normalized across providers.
///
/// llm_wiki returned `AgentReference` here, which carried extra fields only its
/// agent loop read. This is the same wire contract minus those fields.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchReference {
    pub title: String,
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchConfig {
    pub provider: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub ollama_url: Option<String>,
    #[serde(default)]
    pub sear_xng_url: Option<String>,
    #[serde(default)]
    pub sear_xng_categories: Option<Vec<String>>,
    #[serde(default)]
    pub serp_api_engine: Option<String>,
    #[serde(default)]
    pub provider_configs: Option<BTreeMap<String, WebSearchProviderOverride>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct WebSearchProviderOverride {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub ollama_url: Option<String>,
    #[serde(default)]
    pub sear_xng_url: Option<String>,
    #[serde(default)]
    pub sear_xng_categories: Option<Vec<String>>,
    #[serde(default)]
    pub serp_api_engine: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AnyTxtConfig {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub filter_dir: Option<String>,
    #[serde(default)]
    pub filter_ext: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
}

impl WebSearchConfig {
    fn resolved(&self) -> Self {
        let provider = self.provider.trim().to_ascii_lowercase();
        let Some(override_cfg) = self
            .provider_configs
            .as_ref()
            .and_then(|configs| configs.get(&provider))
        else {
            return self.clone();
        };
        Self {
            provider: self.provider.clone(),
            api_key: override_cfg
                .api_key
                .clone()
                .unwrap_or_else(|| self.api_key.clone()),
            ollama_url: override_cfg
                .ollama_url
                .clone()
                .or_else(|| self.ollama_url.clone()),
            sear_xng_url: override_cfg
                .sear_xng_url
                .clone()
                .or_else(|| self.sear_xng_url.clone()),
            sear_xng_categories: override_cfg
                .sear_xng_categories
                .clone()
                .or_else(|| self.sear_xng_categories.clone()),
            serp_api_engine: override_cfg
                .serp_api_engine
                .clone()
                .or_else(|| self.serp_api_engine.clone()),
            provider_configs: self.provider_configs.clone(),
        }
    }
}

// Keep the spec list close to the executor even though the current planner
// still uses fixed tool names. API/MCP tool discovery and future native
// tool-calling should use this list instead of duplicating tool metadata.
pub async fn run_web_search(
    query: &str,
    config: Option<WebSearchConfig>,
    top_k: usize,
) -> Result<Vec<SearchReference>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let Some(config) = config else {
        return Err(
            "Web search is enabled for this turn but no search provider is configured.".to_string(),
        );
    };
    let config = config.resolved();
    let provider = config.provider.trim().to_ascii_lowercase();
    if provider.is_empty() || provider == "none" {
        return Err("Web search provider is not configured.".to_string());
    }
    let max_results = web_search_result_limit(&provider, top_k);
    let client = crate::proxy::configure_http_client(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(WEB_SEARCH_TIMEOUT_SECS))
        .build()
        .map_err(|err| format!("Failed to build web search client: {err}"))?;
    let raw = match provider.as_str() {
        "firecrawl" => firecrawl_search(&client, query, &config, max_results).await?,
        "searxng" => searxng_search(&client, query, &config, max_results).await?,
        "tavily" => tavily_search(&client, query, &config, max_results).await?,
        "ollama" => ollama_search(&client, query, &config, max_results).await?,
        "brave" => brave_search(&client, query, &config, max_results).await?,
        "bocha" => bocha_search(&client, query, &config, max_results).await?,
        "serpapi" => serpapi_search(&client, query, &config, max_results).await?,
        other => {
            return Err(format!(
                "Web search provider '{other}' is not supported by the Rust Agent yet"
            ))
        }
    };
    Ok(web_items_to_references(raw, max_results))
}

fn web_search_result_limit(provider: &str, requested: usize) -> usize {
    // Bocha documents a 1-50 range. Existing providers retain the historical
    // 20-result ceiling so adding Bocha cannot increase their request cost.
    let provider_max = if provider == "bocha" { 50 } else { 20 };
    requested.clamp(1, provider_max)
}

pub async fn run_anytxt_search(
    query: &str,
    config: Option<AnyTxtConfig>,
    top_k: usize,
) -> Result<Vec<SearchReference>, String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let config = config.unwrap_or_default();
    if config.enabled == Some(false) {
        return Ok(Vec::new());
    }
    let endpoint = config
        .endpoint
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_ANYTXT_ENDPOINT)
        .trim()
        .trim_end_matches('/');
    let endpoint = normalize_anytxt_endpoint(endpoint);
    let limit = top_k
        .clamp(1, 100)
        .min(config.limit.unwrap_or(DEFAULT_ANYTXT_LIMIT).clamp(1, 100));
    // AnyTXT has its own query syntax. The caller may already have rewritten
    // natural language into keyword form, so do not run the source-search
    // tokenizer here; pass the pattern through unchanged.
    let pattern = query.to_string();
    let filter_dir = config.filter_dir.unwrap_or_default();
    let filter_ext = config
        .filter_ext
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "*".to_string());
    let client = crate::proxy::configure_http_client(reqwest::Client::builder())
        .timeout(std::time::Duration::from_secs(WEB_SEARCH_TIMEOUT_SECS))
        .build()
        .map_err(|err| format!("Failed to build AnyTXT client: {err}"))?;
    let mut input = json!({
        "pattern": pattern,
        "filterExt": filter_ext,
        "lastModifyBegin": 0,
        "lastModifyEnd": ANYTXT_LAST_MODIFY_END,
        "limit": limit.to_string(),
        "offset": 0,
        "order": 0
    });
    if !filter_dir.trim().is_empty() {
        input["filterDir"] = Value::String(filter_dir);
    }
    let response = client
        .post(&endpoint)
        .header("Accept", "application/json")
        .json(&json!({
            "id": 1,
            "jsonrpc": "2.0",
            "method": "ATRpcServer.Searcher.V1.GetResult",
            "params": { "input": input }
        }))
        .send()
        .await
        .map_err(|err| {
            format!("AnyTXT search failed. Check that ATGUI.exe or the AnyTXT service is running at {endpoint}: {err}")
        })?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed to read AnyTXT response: {err}"))?;
    if !status.is_success() {
        return Err(format!("AnyTXT HTTP {status}: {}", trim_text(&text, 300)));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| format!("AnyTXT returned invalid JSON: {}", trim_text(&text, 300)))?;
    if let Some(error) = value.get("error") {
        return Err(format!(
            "AnyTXT error: {}",
            trim_text(&error.to_string(), 300)
        ));
    }
    let mut references = Vec::new();
    for item in extract_anytxt_items(&value).into_iter().take(limit) {
        let fragment = if !item.fid.trim().is_empty() {
            get_anytxt_fragment(&client, &endpoint, &item.fid, &pattern)
                .await
                .unwrap_or_default()
        } else {
            String::new()
        };
        references.push(SearchReference {
            title: item.title,
            path: file_url_for_path(&item.path),
            kind: "anytxt".to_string(),
            snippet: Some(trim_text(
                if fragment.trim().is_empty() {
                    &item.snippet
                } else {
                    &fragment
                },
                1200,
            ))
            .filter(|s| !s.trim().is_empty()),
            score: None,
        });
    }
    Ok(references)
}

#[derive(Debug, Clone)]
struct AnyTxtItem {
    fid: String,
    title: String,
    path: String,
    snippet: String,
}

fn extract_anytxt_items(value: &Value) -> Vec<AnyTxtItem> {
    let result = value.get("result").unwrap_or(value);
    let candidates = first_anytxt_array(
        result,
        &[
            &[][..],
            &["items"],
            &["files"],
            &["results"],
            &["list"],
            &["value"],
            &["data"],
            &["output"],
            &["output", "items"],
            &["output", "files"],
            &["output", "results"],
            &["output", "list"],
            &["output", "value"],
            &["output", "data"],
            &["data", "items"],
            &["data", "files"],
            &["data", "results"],
            &["data", "list"],
            &["data", "value"],
            &["data", "output"],
            &["data", "output", "items"],
            &["data", "output", "files"],
            &["data", "output", "results"],
            &["data", "output", "list"],
            &["data", "output", "value"],
        ],
    )
    .unwrap_or_default();
    let fields = first_anytxt_fields(
        result,
        &[
            &["field"][..],
            &["fields"],
            &["output", "field"],
            &["output", "fields"],
            &["data", "field"],
            &["data", "fields"],
            &["data", "output", "field"],
            &["data", "output", "fields"],
        ],
    )
    .unwrap_or_default();
    candidates
        .into_iter()
        .filter_map(|item| {
            let record = normalize_anytxt_record(item, &fields);
            let fid = string_field(&record, &["fid", "id", "fileId", "file_id"]);
            let raw_path = string_field(
                &record,
                &[
                    "path",
                    "file",
                    "filePath",
                    "file_path",
                    "fullPath",
                    "full_path",
                    "filename",
                    "fileName",
                    "name",
                ],
            );
            let path = if raw_path.is_empty() && !fid.is_empty() {
                format!("anytxt://{fid}")
            } else {
                raw_path
            };
            let title = string_field(&record, &["title", "name", "fileName", "filename"])
                .trim()
                .to_string();
            let title = if title.is_empty() {
                Path::new(&path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("AnyTXT result")
                    .to_string()
            } else {
                title
            };
            let snippet = string_field(
                &record,
                &[
                    "snippet",
                    "fragment",
                    "content",
                    "contents",
                    "text",
                    "summary",
                    "highlight",
                    "hitText",
                    "hit_text",
                ],
            );
            if path.is_empty() && snippet.is_empty() {
                None
            } else {
                Some(AnyTxtItem {
                    fid,
                    title,
                    path,
                    snippet,
                })
            }
        })
        .collect()
}

fn first_anytxt_array(value: &Value, paths: &[&[&str]]) -> Option<Vec<Value>> {
    for path in paths {
        let Some(candidate) = value_at_path(value, path) else {
            continue;
        };
        if let Some(items) = candidate.as_array() {
            return Some(items.clone());
        }
    }
    None
}

fn first_anytxt_fields(value: &Value, paths: &[&[&str]]) -> Option<Vec<String>> {
    for path in paths {
        let Some(candidate) = value_at_path(value, path) else {
            continue;
        };
        let Some(items) = candidate.as_array() else {
            continue;
        };
        let fields = items
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if !fields.is_empty() {
            return Some(fields);
        }
    }
    None
}

fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    Some(current)
}

fn normalize_anytxt_record(item: Value, fields: &[String]) -> serde_json::Map<String, Value> {
    match item {
        Value::Object(object) => object,
        Value::Array(row) if !fields.is_empty() => fields
            .iter()
            .cloned()
            .zip(row)
            .collect::<serde_json::Map<String, Value>>(),
        other => {
            let mut object = serde_json::Map::new();
            object.insert("text".to_string(), other);
            object
        }
    }
}

fn string_field(record: &serde_json::Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        let Some(value) = record.get(*key) else {
            continue;
        };
        if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
            return text.trim().to_string();
        }
        if let Some(number) = value.as_i64() {
            return number.to_string();
        }
        if let Some(number) = value.as_u64() {
            return number.to_string();
        }
    }
    String::new()
}

async fn get_anytxt_fragment(
    client: &reqwest::Client,
    endpoint: &str,
    fid: &str,
    pattern: &str,
) -> Result<String, String> {
    let response = client
        .post(endpoint)
        .header("Accept", "application/json")
        .json(&json!({
            "id": 2,
            "jsonrpc": "2.0",
            "method": "ATRpcServer.Searcher.V1.GetFragment",
            "params": { "input": { "fid": fid, "pattern": pattern } }
        }))
        .send()
        .await
        .map_err(|err| format!("AnyTXT fragment failed: {err}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed to read AnyTXT fragment response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "AnyTXT fragment HTTP {status}: {}",
            trim_text(&text, 300)
        ));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "AnyTXT fragment returned invalid JSON: {}",
            trim_text(&text, 300)
        )
    })?;
    if let Some(error) = value.get("error") {
        return Err(format!(
            "AnyTXT fragment error: {}",
            trim_text(&error.to_string(), 300)
        ));
    }
    Ok(extract_anytxt_fragment_text(
        value.get("result").unwrap_or(&Value::Null),
    ))
}

fn extract_anytxt_fragment_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    if let Some(items) = value.as_array() {
        return items
            .iter()
            .map(extract_anytxt_fragment_text)
            .filter(|item| !item.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    let Some(object) = value.as_object() else {
        return String::new();
    };
    for key in ["text", "fragment", "content", "snippet", "html"] {
        if let Some(text) = object.get(key).and_then(Value::as_str) {
            return text.to_string();
        }
    }
    for key in ["output", "result", "data", "fragments", "items", "list"] {
        if let Some(next) = object.get(key) {
            let text = extract_anytxt_fragment_text(next);
            if !text.trim().is_empty() {
                return text;
            }
        }
    }
    String::new()
}

fn normalize_anytxt_endpoint(value: &str) -> String {
    if value.starts_with("http://") || value.starts_with("https://") {
        value.to_string()
    } else {
        format!("http://{value}")
    }
}

#[derive(Debug, Clone)]
struct WebSearchItem {
    title: String,
    url: String,
    snippet: String,
}

async fn firecrawl_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let override_cfg = config
        .provider_configs
        .as_ref()
        .and_then(|values| values.get("firecrawl"));
    let base = override_cfg
        .and_then(|value| value.base_url.as_deref())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://api.firecrawl.dev")
        .trim_end_matches('/');
    let mut request = client
        .post(format!("{base}/v2/search"))
        .header("Accept", "application/json");
    if let Some(key) = override_cfg
        .and_then(|value| value.api_key.as_deref())
        .filter(|value| !value.trim().is_empty())
    {
        request = request.bearer_auth(key.trim());
    }
    let response = request
        .json(&json!({ "query": query, "limit": max_results }))
        .send()
        .await
        .map_err(|err| format!("Network error reaching Firecrawl Search: {err}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed to read Firecrawl response: {err}"))?;
    let parsed: Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "Firecrawl search returned invalid JSON: {}",
            trim_text(&text, 300)
        )
    })?;
    if !status.is_success() || parsed.get("success").and_then(Value::as_bool) == Some(false) {
        let msg = parsed
            .get("error")
            .and_then(Value::as_str)
            .map(friendly_firecrawl_error)
            .unwrap_or_else(|| format!("Firecrawl search failed ({status})"));
        return Err(msg);
    }
    let items = extract_web_items(&parsed, &["data", "results"]);
    Ok(items.into_iter().map(normalize_web_result).collect())
}

async fn searxng_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let base = config
        .sear_xng_url
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| "SearXNG URL is required for web.search".to_string())?;
    let mut url = normalize_searxng_url(base)?;
    let categories = config
        .sear_xng_categories
        .clone()
        .unwrap_or_else(|| vec!["general".to_string()]);
    url.push_str(&format!(
        "?q={}&format=json&categories={}",
        url_encode(query),
        url_encode(&categories.join(","))
    ));
    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("Network error reaching SearXNG: {err}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed to read SearXNG response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "SearXNG search failed ({status}): {}",
            trim_text(&text, 300)
        ));
    }
    let parsed: Value = serde_json::from_str(&text)
        .map_err(|_| format!("SearXNG returned invalid JSON: {}", trim_text(&text, 300)))?;
    let items = parsed
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .into_iter()
        .take(max_results)
        .map(normalize_web_result)
        .collect())
}

async fn tavily_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let key = required_api_key(config, "Tavily")?;
    let response = client
        .post("https://api.tavily.com/search")
        .json(&json!({
            "api_key": key,
            "query": query,
            "max_results": max_results,
            "search_depth": "advanced",
            "include_answer": false
        }))
        .send()
        .await
        .map_err(|err| format!("Network error reaching Tavily: {err}"))?;
    parse_web_json_response(response, "Tavily", |value| {
        value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(normalize_web_result)
            .collect()
    })
    .await
}

async fn ollama_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let key = required_api_key(config, "Ollama")?;
    let base = config
        .ollama_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("https://ollama.com")
        .trim()
        .trim_end_matches('/');
    let url = format!("{base}/api/web_search");
    let response = client
        .post(url)
        .header("Accept", "application/json")
        .bearer_auth(key)
        .json(&json!({
            "query": query,
            "max_results": max_results
        }))
        .send()
        .await
        .map_err(|err| format!("Network error reaching Ollama Web Search: {err}"))?;
    parse_web_json_response(response, "Ollama Web Search", |value| {
        value
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(normalize_web_result)
            .collect()
    })
    .await
}

async fn brave_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let key = required_api_key(config, "Brave")?;
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        url_encode(query),
        max_results.min(20)
    );
    let response = client
        .get(url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", key)
        .send()
        .await
        .map_err(|err| format!("Network error reaching Brave Search: {err}"))?;
    parse_web_json_response(response, "Brave Search", |value| {
        value
            .get("web")
            .and_then(|web| web.get("results"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(normalize_web_result)
            .collect()
    })
    .await
}

async fn bocha_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let key = required_api_key(config, "Bocha")?;
    let response = client
        .post("https://api.bocha.cn/v1/web-search")
        .header("Accept", "application/json")
        .bearer_auth(key)
        .json(&json!({
            "query": query,
            "freshness": "noLimit",
            "summary": true,
            "count": max_results.clamp(1, 50)
        }))
        .send()
        .await
        .map_err(|err| format!("Network error reaching Bocha Search: {err}"))?;
    parse_web_json_response(response, "Bocha Search", parse_bocha_results).await
}

fn parse_bocha_results(value: Value) -> Vec<WebSearchItem> {
    value
        .get("data")
        .and_then(|data| data.get("webPages"))
        .and_then(|pages| pages.get("value"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|item| WebSearchItem {
            title: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Untitled")
                .to_string(),
            url: item
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            snippet: item
                .get("summary")
                .and_then(Value::as_str)
                .or_else(|| item.get("snippet").and_then(Value::as_str))
                .unwrap_or("")
                .to_string(),
        })
        .collect()
}

async fn serpapi_search(
    client: &reqwest::Client,
    query: &str,
    config: &WebSearchConfig,
    max_results: usize,
) -> Result<Vec<WebSearchItem>, String> {
    let key = required_api_key(config, "SerpApi")?;
    let engine = config.serp_api_engine.as_deref().unwrap_or("google");
    let url = format!(
        "https://serpapi.com/search?engine={}&q={}&api_key={}&num={}",
        url_encode(engine),
        url_encode(query),
        url_encode(key),
        max_results
    );
    let response = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("Network error reaching SerpApi: {err}"))?;
    parse_web_json_response(response, "SerpApi", |value| {
        for key in [
            "organic_results",
            "news_results",
            "images_results",
            "video_results",
            "videos_results",
            "shopping_results",
        ] {
            if let Some(items) = value.get(key).and_then(Value::as_array) {
                return items.iter().cloned().map(normalize_web_result).collect();
            }
        }
        Vec::new()
    })
    .await
}

async fn parse_web_json_response(
    response: reqwest::Response,
    provider: &str,
    parse: impl FnOnce(Value) -> Vec<WebSearchItem>,
) -> Result<Vec<WebSearchItem>, String> {
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| format!("Failed to read {provider} response: {err}"))?;
    if !status.is_success() {
        return Err(format!(
            "{provider} search failed ({status}): {}",
            trim_text(&text, 300)
        ));
    }
    let value: Value = serde_json::from_str(&text).map_err(|_| {
        format!(
            "{provider} returned invalid JSON: {}",
            trim_text(&text, 300)
        )
    })?;
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(format!("{provider} search failed: {error}"));
    }
    if let Some(message) = provider_payload_error(provider, &value) {
        return Err(message);
    }
    Ok(parse(value))
}

fn provider_payload_error(provider: &str, value: &Value) -> Option<String> {
    if provider == "Bocha Search" {
        let code = value.get("code").and_then(Value::as_i64);
        if code != Some(200) {
            let message = value
                .get("msg")
                .and_then(Value::as_str)
                .filter(|message| !message.trim().is_empty())
                .unwrap_or("unknown API error");
            return Some(format!(
                "{provider} failed (code {}): {message}",
                code.unwrap_or(0)
            ));
        }
    }
    if provider == "Brave Search" && value.get("web").is_none() {
        let message = value.get("message").and_then(Value::as_str)?;
        return Some(format!("{provider} search failed: {message}"));
    }
    None
}

fn web_items_to_references(raw: Vec<WebSearchItem>, max_results: usize) -> Vec<SearchReference> {
    raw.into_iter()
        .take(max_results)
        .filter(|item| !item.url.trim().is_empty())
        .map(|item| SearchReference {
            title: item.title,
            path: item.url,
            kind: "web".to_string(),
            snippet: Some(item.snippet).filter(|s| !s.trim().is_empty()),
            score: None,
        })
        .collect()
}

fn normalize_web_result(value: Value) -> WebSearchItem {
    let metadata = value.get("metadata");
    let title = value
        .get("title")
        .or_else(|| metadata.and_then(|m| m.get("title")))
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string();
    let url = value
        .get("url")
        .or_else(|| value.get("link"))
        .or_else(|| metadata.and_then(|m| m.get("sourceURL")))
        .or_else(|| metadata.and_then(|m| m.get("url")))
        .or_else(|| value.get("original"))
        .or_else(|| value.get("thumbnail"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let snippet = value
        .get("snippet")
        .or_else(|| value.get("content"))
        .or_else(|| value.get("description"))
        .or_else(|| metadata.and_then(|m| m.get("description")))
        .or_else(|| value.get("summary"))
        .or_else(|| value.get("markdown"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    WebSearchItem {
        title,
        url,
        snippet,
    }
}

fn extract_web_items(value: &Value, keys: &[&str]) -> Vec<Value> {
    for key in keys {
        let Some(candidate) = value.get(*key) else {
            continue;
        };
        if let Some(items) = candidate.as_array() {
            return items.clone();
        }
        if let Some(items) = extract_nested_web_items(candidate) {
            return items;
        }
    }
    Vec::new()
}

fn extract_nested_web_items(value: &Value) -> Option<Vec<Value>> {
    let object = value.as_object()?;
    for key in ["web", "results", "items"] {
        if let Some(items) = object.get(key).and_then(Value::as_array) {
            return Some(items.clone());
        }
    }
    None
}

fn required_api_key<'a>(config: &'a WebSearchConfig, provider: &str) -> Result<&'a str, String> {
    let key = config.api_key.trim();
    if key.is_empty() {
        Err(format!(
            "{provider} web.search requires an API key in Settings."
        ))
    } else {
        Ok(key)
    }
}

fn normalize_searxng_url(value: &str) -> Result<String, String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("SearXNG URL is required".to_string());
    }
    let mut url = if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };
    if !url.ends_with("/search") {
        url.push_str("/search");
    }
    Ok(url)
}

fn friendly_firecrawl_error(error: &str) -> String {
    if error
        .to_ascii_lowercase()
        .contains("ip address looks suspicious")
    {
        "Firecrawl Search rejected this IP for key-free access. Add a Firecrawl API key in Settings or choose another Web Search provider.".to_string()
    } else {
        format!("Firecrawl search failed: {error}")
    }
}

fn trim_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_string()
    } else {
        format!("{}...", value.chars().take(max_chars).collect::<String>())
    }
}

fn url_encode(value: &str) -> String {
    value
        .bytes()
        .flat_map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                vec![byte as char]
            }
            b' ' => vec!['+'],
            _ => format!("%{byte:02X}").chars().collect(),
        })
        .collect()
}
