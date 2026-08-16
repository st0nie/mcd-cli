use anyhow::{Result, anyhow};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MCP_PROTOCOL_VERSION: &str = "2025-06-18";

#[derive(Debug)]
pub struct McpClient {
    client: reqwest::Client,
    url: String,
    token: String,
    request_id: std::sync::atomic::AtomicU64,
}

impl Clone for McpClient {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            url: self.url.clone(),
            token: self.token.clone(),
            request_id: std::sync::atomic::AtomicU64::new(
                self.request_id.load(std::sync::atomic::Ordering::SeqCst),
            ),
        }
    }
}

#[derive(Serialize, Debug)]
struct JsonRpcRequest<T> {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    params: T,
}

#[derive(Deserialize, Debug)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
}

#[derive(Deserialize, Debug)]
struct JsonRpcError {
    code: i32,
    message: String,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: String,
    pub capabilities: Value,
    pub client_info: ClientInfo,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Serialize, Debug)]
pub struct CallToolParams {
    pub name: String,
    pub arguments: Value,
}

#[derive(Deserialize, Debug)]
pub struct ToolResult {
    pub content: Vec<ToolContent>,
    #[serde(rename = "structuredContent")]
    pub structured_content: Option<Value>,
}

#[derive(Deserialize, Debug)]
pub struct ToolContent {
    #[serde(default)]
    pub text: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: String,
    pub server_info: ClientInfo,
}

impl McpClient {
    pub fn with_url(url: impl Into<String>, token: impl Into<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()?;
        Ok(McpClient {
            client,
            url: url.into(),
            token: token.into(),
            request_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    fn next_id(&self) -> u64 {
        self.request_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    fn headers(&self) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        let auth = format!("Bearer {}", self.token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth).map_err(|e| anyhow!("Invalid token: {}", e))?,
        );
        Ok(headers)
    }

    async fn request<T: Serialize, R: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: T,
    ) -> Result<R> {
        let id = self.next_id();
        let req_body = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let resp = self
            .client
            .post(&self.url)
            .headers(self.headers()?)
            .json(&req_body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, text));
        }

        let resp_body: JsonRpcResponse<R> = resp.json().await?;

        if let Some(err) = resp_body.error {
            return Err(anyhow!("MCP error {}: {}", err.code, err.message));
        }

        resp_body
            .result
            .ok_or_else(|| anyhow!("Empty result from MCP server"))
    }

    pub async fn initialize(&self) -> Result<InitializeResult> {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: serde_json::json!({}),
            client_info: ClientInfo {
                name: "mcd-cli".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
        };
        self.request("initialize", params).await
    }

    pub async fn call_tool(&self, name: impl Into<String>, arguments: Value) -> Result<ToolResult> {
        let params = CallToolParams {
            name: name.into(),
            arguments,
        };
        self.request("tools/call", params).await
    }
}
