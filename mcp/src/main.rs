//! paqtra-mcp: a stdio Model Context Protocol server for Paqtra.
//!
//! One JSON-RPC 2.0 message per line on stdin/stdout. Every tool is a read of
//! the Paqtra API (`web-api`) with the caller's own credentials; nothing here
//! applies, edits or deletes a policy. Enforcement stays with Cilium CRDs.
//! `paqtra_investigate_path` and `paqtra_investigate_flow` POST an analysis
//! request, which stores an evidence bundle but changes no cluster state.

use serde_json::{json, Map, Value};
use std::io::{BufRead, Write};
use std::sync::Mutex;
use std::time::Duration;

const SERVER_NAME: &str = "paqtra-mcp";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_PROTOCOL: &str = "2025-06-18";
const MAX_BODY: u64 = 4 * 1024 * 1024;

/// How a tool reaches the API.
#[derive(Clone, Copy, PartialEq)]
enum Method {
    Get,
    Post,
}

struct Tool {
    name: &'static str,
    description: &'static str,
    method: Method,
    /// API path; `{x}` segments are filled from the argument of the same name.
    path: &'static str,
    /// Documented query parameters (GET) or body fields (POST).
    params: &'static [(&'static str, &'static str)],
}

const NS: (&str, &str) = ("namespace", "Kubernetes namespace to filter on");
const LIMIT: (&str, &str) = ("limit", "Maximum rows to return");
const OFFSET: (&str, &str) = ("offset", "Rows to skip, for paging");

const TOOLS: &[Tool] = &[
    Tool { name: "paqtra_flows", description: "List recent Hubble flows, optionally filtered by namespace or verdict (FORWARDED, DROPPED).", method: Method::Get, path: "/api/v1/flows", params: &[NS, ("verdict", "FORWARDED or DROPPED"), LIMIT, OFFSET] },
    Tool { name: "paqtra_flow", description: "Get one flow by id.", method: Method::Get, path: "/api/v1/flows/{id}", params: &[("id", "Flow id")] },
    Tool { name: "paqtra_flow_stats", description: "Flow totals: forwarded, dropped, requests per second, average latency.", method: Method::Get, path: "/api/v1/flows/stats", params: &[] },
    Tool { name: "paqtra_flow_history", description: "Query stored flow history.", method: Method::Get, path: "/api/v1/flows/history", params: &[NS, LIMIT, OFFSET] },
    Tool { name: "paqtra_flow_timeline", description: "Flow counts over time from stored history.", method: Method::Get, path: "/api/v1/flows/history/timeline", params: &[NS] },
    Tool { name: "paqtra_flow_store", description: "Size, retention and health of the flow history store.", method: Method::Get, path: "/api/v1/flows/store", params: &[] },
    Tool { name: "paqtra_policies", description: "List network policies (Cilium CNP/CCNP) Paqtra can see.", method: Method::Get, path: "/api/v1/policies", params: &[NS] },
    Tool { name: "paqtra_policy", description: "Get one network policy by id.", method: Method::Get, path: "/api/v1/policies/{id}", params: &[("id", "Policy id")] },
    Tool { name: "paqtra_hubble_nodes", description: "Hubble relay node status.", method: Method::Get, path: "/api/v1/hubble/nodes", params: &[] },
    Tool { name: "paqtra_hubble_metrics", description: "Hubble metrics summary.", method: Method::Get, path: "/api/v1/hubble/metrics", params: &[] },
    Tool { name: "paqtra_cilium_metrics", description: "Cilium agent metrics summary.", method: Method::Get, path: "/api/v1/cilium/metrics", params: &[] },
    Tool { name: "paqtra_cilium_features", description: "Enabled Cilium features (kube-proxy replacement, encryption, Hubble, ...).", method: Method::Get, path: "/api/v1/cilium/features", params: &[] },
    Tool { name: "paqtra_cilium_resources", description: "List Cilium custom resources of a kind (for example ciliumnetworkpolicies, ciliumendpoints).", method: Method::Get, path: "/api/v1/cilium/resources/{kind}", params: &[("kind", "Resource kind"), NS] },
    Tool { name: "paqtra_cilium_agent", description: "Read-only Cilium agent query (status, endpoints, identities, ...).", method: Method::Get, path: "/api/v1/cilium/agent/{what}", params: &[("what", "Agent query name")] },
    Tool { name: "paqtra_investigate_path", description: "Answer why workload A cannot reach B: evidence from flows, drops and policy. Stores an evidence bundle; changes no cluster state.", method: Method::Post, path: "/api/v1/investigate/path", params: &[("source", "object {namespace, name|pod}"), ("destination", "object {namespace, name|pod}"), ("port", "destination port"), ("protocol", "TCP (default) or UDP"), ("time_window_minutes", "look-back, default 60")] },
    Tool { name: "paqtra_investigate_flow", description: "Explain one flow: which policy or condition allowed or dropped it. Stores an evidence bundle; changes no cluster state.", method: Method::Post, path: "/api/v1/investigate/flow", params: &[("flow_id", "Flow id to explain")] },
    Tool { name: "paqtra_investigation", description: "Fetch a stored investigation bundle by id.", method: Method::Get, path: "/api/v1/investigate/bundles/{id}", params: &[("id", "Bundle id")] },
    Tool { name: "paqtra_connectivity_paths", description: "Declared connectivity paths being watched.", method: Method::Get, path: "/api/v1/connectivity/paths", params: &[] },
    Tool { name: "paqtra_connectivity_path_status", description: "Current status of one declared connectivity path.", method: Method::Get, path: "/api/v1/connectivity/paths/{id}/status", params: &[("id", "Path id")] },
    Tool { name: "paqtra_connectivity_alerts", description: "Connectivity alerts raised by declared paths.", method: Method::Get, path: "/api/v1/connectivity/alerts", params: &[] },
    Tool { name: "paqtra_anomalies", description: "Detected network anomalies.", method: Method::Get, path: "/api/v1/anomalies", params: &[NS, LIMIT] },
    Tool { name: "paqtra_anomaly", description: "Get one anomaly by id.", method: Method::Get, path: "/api/v1/anomalies/{id}", params: &[("id", "Anomaly id")] },
    Tool { name: "paqtra_compliance_frameworks", description: "Supported compliance frameworks.", method: Method::Get, path: "/api/v1/compliance/frameworks", params: &[] },
    Tool { name: "paqtra_compliance_audits", description: "Past compliance audits.", method: Method::Get, path: "/api/v1/compliance/audits", params: &[] },
    Tool { name: "paqtra_compliance_audit", description: "Get one compliance audit by id.", method: Method::Get, path: "/api/v1/compliance/audits/{id}", params: &[("id", "Audit id")] },
    Tool { name: "paqtra_security_posture", description: "Overall security posture summary.", method: Method::Get, path: "/api/v1/security/posture", params: &[] },
    Tool { name: "paqtra_security_findings", description: "Security findings from the dashboard.", method: Method::Get, path: "/api/v1/security/findings", params: &[NS] },
    Tool { name: "paqtra_zero_trust", description: "Zero-trust score per namespace.", method: Method::Get, path: "/api/v1/security/zero-trust", params: &[] },
    Tool { name: "paqtra_rootcause_drops", description: "Recent packet drops with reasons.", method: Method::Get, path: "/api/v1/modules/rootcause/drops", params: &[NS, LIMIT] },
    Tool { name: "paqtra_healer_problems", description: "Problems the healer module has detected (listing only; fixes are not exposed).", method: Method::Get, path: "/api/v1/modules/healer/problems", params: &[] },
    Tool { name: "paqtra_events", description: "Recent Kubernetes and Cilium events.", method: Method::Get, path: "/api/v1/events", params: &[NS, LIMIT] },
    Tool { name: "paqtra_endpoints", description: "Cilium endpoints.", method: Method::Get, path: "/api/v1/endpoints", params: &[NS] },
    Tool { name: "paqtra_nodes", description: "Cluster nodes and their Cilium agent state.", method: Method::Get, path: "/api/v1/nodes", params: &[] },
    Tool { name: "paqtra_dependencies", description: "Observed service dependency graph.", method: Method::Get, path: "/api/v1/dependencies", params: &[NS] },
    Tool { name: "paqtra_heatmap", description: "Traffic heatmap data.", method: Method::Get, path: "/api/v1/heatmap", params: &[NS] },
    Tool { name: "paqtra_ebpf_attachments", description: "BPF program attachments classified as cil_*, netra_* or other (read-only).", method: Method::Get, path: "/api/v1/ebpf/attachments", params: &[] },
    Tool { name: "paqtra_ebpf_drift", description: "BPF attachment drift against the last known state.", method: Method::Get, path: "/api/v1/ebpf/drift", params: &[] },
    Tool { name: "paqtra_ebpf_programs", description: "Loaded eBPF programs and their runtime stats.", method: Method::Get, path: "/api/v1/ebpf/programs", params: &[] },
    Tool { name: "paqtra_health", description: "API health.", method: Method::Get, path: "/health", params: &[] },
];

struct Client {
    base: String,
    token: Mutex<Option<String>>,
    username: String,
    password: String,
    agent: ureq::Agent,
}

impl Client {
    fn from_env() -> Self {
        let env = |k: &str| std::env::var(k).unwrap_or_default();
        let base = std::env::var("PAQTRA_URL").unwrap_or_else(|_| "http://127.0.0.1:9191".into());
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .build()
            .into();
        let token = Some(env("PAQTRA_TOKEN")).filter(|t| !t.is_empty());
        Client {
            base: base.trim_end_matches('/').to_string(),
            token: Mutex::new(token),
            username: env("PAQTRA_USERNAME"),
            password: env("PAQTRA_PASSWORD"),
            agent,
        }
    }

    /// Bearer token: PAQTRA_TOKEN, or one login with PAQTRA_USERNAME/PASSWORD.
    fn bearer(&self, force_login: bool) -> Result<Option<String>, String> {
        let mut guard = self.token.lock().unwrap();
        if guard.is_some() && !force_login {
            return Ok(guard.clone());
        }
        if self.username.is_empty() || self.password.is_empty() {
            return Ok(guard.clone());
        }
        let body = json!({"username": self.username, "password": self.password});
        let mut resp = self
            .agent
            .post(&format!("{}/api/v1/auth/login", self.base))
            .send_json(&body)
            .map_err(|e| format!("login failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("login failed: HTTP {}", resp.status().as_u16()));
        }
        let v: Value = resp
            .body_mut()
            .read_json()
            .map_err(|e| format!("login reply: {e}"))?;
        let t = v["token"]
            .as_str()
            .ok_or("login reply carried no token")?
            .to_string();
        *guard = Some(t.clone());
        Ok(Some(t))
    }

    fn call(&self, tool: &Tool, args: &Map<String, Value>) -> Result<Value, String> {
        let (path, rest) = fill_path(tool.path, args)?;
        let url = format!("{}{}", self.base, path);
        let mut relogin = false;
        loop {
            let token = self.bearer(relogin)?;
            let mut resp = match tool.method {
                Method::Get => {
                    let mut req = self.agent.get(&url);
                    for (k, v) in &rest {
                        req = req.query(k, v);
                    }
                    if let Some(t) = &token {
                        req = req.header("Authorization", &format!("Bearer {t}"));
                    }
                    req.call()
                }
                Method::Post => {
                    let mut req = self.agent.post(&url);
                    if let Some(t) = &token {
                        req = req.header("Authorization", &format!("Bearer {t}"));
                    }
                    req.send_json(Value::Object(args.clone()))
                }
            }
            .map_err(|e| format!("request failed: {e}"))?;
            let status = resp.status().as_u16();
            if status == 401 && !relogin && !self.username.is_empty() {
                relogin = true;
                continue;
            }
            let text = resp
                .body_mut()
                .with_config()
                .limit(MAX_BODY)
                .read_to_string()
                .map_err(|e| format!("reading reply: {e}"))?;
            if !(200..300).contains(&status) {
                return Err(format!(
                    "HTTP {status}: {}",
                    text.chars().take(500).collect::<String>()
                ));
            }
            return Ok(serde_json::from_str(&text).unwrap_or(Value::String(text)));
        }
    }
}

/// Fill `{x}` segments from `args`; return the path plus the unused scalar
/// arguments as query pairs. Path values are limited to a safe character set.
fn fill_path(
    tmpl: &str,
    args: &Map<String, Value>,
) -> Result<(String, Vec<(String, String)>), String> {
    let mut out = String::new();
    let mut used: Vec<String> = Vec::new();
    let mut rest = tmpl;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let j = rest[i..].find('}').ok_or("bad path template")? + i;
        let key = &rest[i + 1..j];
        let val =
            scalar(args.get(key)).ok_or_else(|| format!("missing required argument '{key}'"))?;
        if val.is_empty()
            || !val
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.:".contains(c))
        {
            return Err(format!(
                "argument '{key}' has characters not allowed in a path"
            ));
        }
        out.push_str(&val);
        used.push(key.to_string());
        rest = &rest[j + 1..];
    }
    out.push_str(rest);
    let query = args
        .iter()
        .filter(|(k, _)| !used.contains(k))
        .filter_map(|(k, v)| scalar(Some(v)).map(|s| (k.clone(), s)))
        .collect();
    Ok((out, query))
}

fn scalar(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn tool_schema(t: &Tool) -> Value {
    let mut props = Map::new();
    let mut required = Vec::new();
    for (name, desc) in t.params {
        let ty = match *name {
            "limit" | "offset" | "port" | "time_window_minutes" => "integer",
            "source" | "destination" => "object",
            _ => "string",
        };
        props.insert((*name).into(), json!({"type": ty, "description": desc}));
        if t.path.contains(&format!("{{{name}}}")) {
            required.push(Value::String((*name).into()));
        }
    }
    if t.method == Method::Post && t.name == "paqtra_investigate_path" {
        required = vec!["source".into(), "destination".into(), "port".into()];
    }
    if t.name == "paqtra_investigate_flow" {
        required = vec!["flow_id".into()];
    }
    json!({
        "name": t.name,
        "description": t.description,
        "inputSchema": {"type": "object", "properties": props, "required": required},
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "openWorldHint": false,
            "title": t.name,
        },
    })
}

fn result(id: &Value, r: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": r})
}

fn error(id: &Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

fn handle(client: &Client, msg: &Value) -> Option<Value> {
    let method = msg["method"].as_str().unwrap_or("");
    // Notifications (no id) never get a reply.
    let id = msg.get("id").cloned()?;
    Some(match method {
        "initialize" => {
            let proto = msg["params"]["protocolVersion"]
                .as_str()
                .unwrap_or(DEFAULT_PROTOCOL);
            result(
                &id,
                json!({
                    "protocolVersion": proto,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": SERVER_NAME, "version": VERSION},
                    "instructions": "Read-only Paqtra (Cilium) network observability. Start with paqtra_flow_stats, paqtra_security_posture or paqtra_investigate_path. Policy changes go through Cilium CRDs, not this server.",
                }),
            )
        }
        "ping" => result(&id, json!({})),
        "tools/list" => result(
            &id,
            json!({"tools": TOOLS.iter().map(tool_schema).collect::<Vec<_>>()}),
        ),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let Some(tool) = TOOLS.iter().find(|t| t.name == name) else {
                return Some(error(&id, -32602, &format!("unknown tool '{name}'")));
            };
            let args = msg["params"]["arguments"]
                .as_object()
                .cloned()
                .unwrap_or_default();
            match client.call(tool, &args) {
                Ok(v) => result(
                    &id,
                    json!({
                        "content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}],
                        "isError": false,
                    }),
                ),
                Err(e) => result(
                    &id,
                    json!({"content": [{"type": "text", "text": e}], "isError": true}),
                ),
            }
        }
        "prompts/list" => result(&id, json!({"prompts": []})),
        "resources/list" => result(&id, json!({"resources": []})),
        _ => error(&id, -32601, &format!("method not found: {method}")),
    })
}

fn main() {
    let client = Client::from_env();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => handle(&client, &msg),
            Err(e) => Some(error(&Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(r) = reply {
            let _ = writeln!(out, "{r}");
            let _ = out.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn path_fills_and_rest_becomes_query() {
        let (p, q) = fill_path(
            "/api/v1/flows/{id}",
            &args(json!({"id": "abc-1", "limit": 5})),
        )
        .unwrap();
        assert_eq!(p, "/api/v1/flows/abc-1");
        assert_eq!(q, vec![("limit".to_string(), "5".to_string())]);
    }

    #[test]
    fn path_rejects_traversal_and_missing() {
        assert!(fill_path("/x/{id}", &args(json!({"id": "../etc"}))).is_err());
        assert!(fill_path("/x/{id}", &args(json!({"id": "a/b"}))).is_err());
        assert!(fill_path("/x/{id}", &args(json!({}))).is_err());
    }

    #[test]
    fn tool_names_are_unique_and_read_scoped() {
        let mut names: Vec<_> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        // Only the two investigations POST; everything else is a GET.
        let posts: Vec<_> = TOOLS
            .iter()
            .filter(|t| t.method == Method::Post)
            .map(|t| t.name)
            .collect();
        assert_eq!(
            posts,
            vec!["paqtra_investigate_path", "paqtra_investigate_flow"]
        );
        for t in TOOLS {
            assert!(
                !t.path.contains("/policies") || t.method == Method::Get,
                "{}",
                t.name
            );
        }
    }

    #[test]
    fn initialize_and_list() {
        let c = Client::from_env();
        let r = handle(
            &c,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
        )
        .unwrap();
        assert_eq!(r["result"]["serverInfo"]["name"], "paqtra-mcp");
        let l = handle(&c, &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"})).unwrap();
        assert_eq!(l["result"]["tools"].as_array().unwrap().len(), TOOLS.len());
        assert!(handle(
            &c,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .is_none());
    }

    /// Serve `n` canned HTTP exchanges on loopback; returns (base url, request log).
    fn stub(n: usize) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::Read;
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..n {
                let (mut c, _) = l.accept().unwrap();
                let mut buf = [0u8; 8192];
                let k = c.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..k]).to_string();
                let first = req.lines().next().unwrap_or("").to_string();
                let auth_ok = req.to_lowercase().contains("authorization: bearer t1");
                let (code, body) = if first.contains("/auth/login") {
                    ("200 OK", r#"{"token":"T1"}"#.to_string())
                } else if auth_ok {
                    ("200 OK", format!(r#"{{"seen":"{first}"}}"#))
                } else {
                    ("401 Unauthorized", r#"{"error":"no"}"#.to_string())
                };
                let _ = tx.send(first);
                let resp = format!(
                    "HTTP/1.1 {code}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                c.write_all(resp.as_bytes()).unwrap();
            }
        });
        (base, rx)
    }

    #[test]
    fn logs_in_once_then_sends_bearer_and_query() {
        let (base, rx) = stub(2);
        let c = Client {
            base,
            token: Mutex::new(None),
            username: "u".into(),
            password: "p".into(),
            agent: ureq::Agent::config_builder()
                .http_status_as_error(false)
                .build()
                .into(),
        };
        let t = TOOLS.iter().find(|t| t.name == "paqtra_flows").unwrap();
        let v = c
            .call(t, &args(json!({"namespace": "prod", "verdict": "DROPPED"})))
            .unwrap();
        assert_eq!(rx.recv().unwrap(), "POST /api/v1/auth/login HTTP/1.1");
        let second = rx.recv().unwrap();
        assert!(second.starts_with("GET /api/v1/flows?"), "{second}");
        assert!(
            second.contains("namespace=prod") && second.contains("verdict=DROPPED"),
            "{second}"
        );
        assert!(v["seen"].as_str().unwrap().contains("/api/v1/flows"));
    }
}
