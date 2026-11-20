// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

// Real eBPF handler module — queries kernel state via bpftool.

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::{check_admin, paginate_json, PaginationQuery};
use crate::AppState;

/// Classify BPF program name into brotherhood owner (cilium | netra | other).
fn classify_owner(name: &str) -> &'static str {
    let n = name.trim();
    if n.starts_with("cil_")
        || n.starts_with("cilium")
        || n.contains("cilium")
        || n.starts_with("from-")
        || n.starts_with("to-")
        || n.starts_with("tail_")
    {
        "cilium"
    } else if n.starts_with("netra_") || n.starts_with("netra") {
        "netra"
    } else {
        "other"
    }
}

struct BpfCacheEntry {
    at: Instant,
    value: Value,
}

fn bpf_cache() -> &'static Mutex<HashMap<String, BpfCacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, BpfCacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

const BPF_CACHE_TTL: Duration = Duration::from_secs(30);
const BPFTOOL_TIMEOUT: Duration = Duration::from_secs(8);
const BPFTOOL_MAX_CONCURRENT: usize = 2;

fn bpf_semaphore() -> &'static tokio::sync::Semaphore {
    static SEM: OnceLock<tokio::sync::Semaphore> = OnceLock::new();
    SEM.get_or_init(|| tokio::sync::Semaphore::new(BPFTOOL_MAX_CONCURRENT))
}

// ---------------------------------------------------------------------------
// Helper: run bpftool with a short timeout + 30s result cache + concurrency cap
// ---------------------------------------------------------------------------

async fn run_bpftool(args: &[&str]) -> Result<Value, String> {
    let key = args.join(" ");
    if let Ok(cache) = bpf_cache().lock() {
        if let Some(hit) = cache.get(&key) {
            if hit.at.elapsed() < BPF_CACHE_TTL {
                return Ok(hit.value.clone());
            }
        }
    }

    let _permit = bpf_semaphore()
        .acquire()
        .await
        .map_err(|e| format!("bpftool semaphore: {e}"))?;
    let value = run_bpftool_uncached(args).await?;
    if let Ok(mut cache) = bpf_cache().lock() {
        cache.insert(
            key,
            BpfCacheEntry {
                at: Instant::now(),
                value: value.clone(),
            },
        );
    }
    Ok(value)
}

async fn run_bpftool_uncached(args: &[&str]) -> Result<Value, String> {
    // Try local bpftool first
    let fut = tokio::process::Command::new("bpftool").args(args).output();
    match tokio::time::timeout(BPFTOOL_TIMEOUT, fut).await {
        Ok(Ok(output)) if output.status.success() => {
            return serde_json::from_slice(&output.stdout)
                .map_err(|e| format!("Parse error: {}", e));
        }
        _ => {}
    }

    // Resolve a Cilium agent pod once, then exec (daemonset/ target can hang on some clusters).
    let pod = {
        let fut = tokio::process::Command::new("kubectl")
            .args([
                "get",
                "pod",
                "-n",
                "kube-system",
                "-l",
                "k8s-app=cilium",
                "-o",
                "jsonpath={.items[0].metadata.name}",
            ])
            .output();
        match tokio::time::timeout(Duration::from_secs(5), fut).await {
            Ok(Ok(o)) if o.status.success() => {
                String::from_utf8_lossy(&o.stdout).trim().to_string()
            }
            _ => String::new(),
        }
    };
    if pod.is_empty() {
        return Err("no cilium agent pod found for bpftool".into());
    }

    let mut kubectl_args: Vec<String> = vec![
        "exec".into(),
        "-n".into(),
        "kube-system".into(),
        pod,
        "-c".into(),
        "cilium-agent".into(),
        "--".into(),
        "bpftool".into(),
    ];
    kubectl_args.extend(args.iter().map(|a| (*a).to_string()));

    let fut = tokio::process::Command::new("kubectl")
        .args(&kubectl_args)
        .kill_on_drop(true)
        .output();

    let output = tokio::time::timeout(BPFTOOL_TIMEOUT, fut)
        .await
        .map_err(|_| "bpftool timed out via cilium agent".to_string())?
        .map_err(|e| format!("bpftool kubectl exec failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "bpftool unavailable via cilium agent: {}",
            stderr.trim()
        ));
    }

    serde_json::from_slice(&output.stdout).map_err(|e| format!("Parse error: {}", e))
}

fn error_response(status: StatusCode, msg: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({ "error": msg })))
}

// ---------------------------------------------------------------------------
// Hex-array helpers
// ---------------------------------------------------------------------------

/// Parse a bpftool hex-string array (e.g. ["0x0a","0x00",...]) into bytes.
fn hex_array_to_bytes(arr: &[Value]) -> Vec<u8> {
    arr.iter()
        .filter_map(|v| {
            let s = v.as_str()?;
            u8::from_str_radix(s.trim_start_matches("0x").trim_start_matches("0X"), 16).ok()
        })
        .collect()
}

fn bytes_to_ipv4(b: &[u8]) -> String {
    if b.len() >= 4 {
        format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
    } else {
        "0.0.0.0".into()
    }
}

fn bytes_to_u16_be(b: &[u8]) -> u16 {
    if b.len() >= 2 {
        u16::from_be_bytes([b[0], b[1]])
    } else {
        0
    }
}

fn bytes_to_u64_le(b: &[u8]) -> u64 {
    b.get(..8)
        .and_then(|s| <[u8; 8]>::try_from(s).ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

fn bytes_to_u32_le(b: &[u8]) -> u32 {
    b.get(..4)
        .and_then(|s| <[u8; 4]>::try_from(s).ok())
        .map(u32::from_le_bytes)
        .unwrap_or(0)
}

fn protocol_name(p: u8) -> &'static str {
    match p {
        1 => "ICMP",
        6 => "TCP",
        17 => "UDP",
        _ => "OTHER",
    }
}

// ---------------------------------------------------------------------------
// Map lookup helpers
// ---------------------------------------------------------------------------

/// Find the first map whose name contains `substr`.
fn find_map_by_name<'a>(maps: &'a [Value], substr: &str) -> Option<&'a Value> {
    maps.iter().find(|m| {
        m.get("name")
            .and_then(|n| n.as_str())
            .map(|n| n.contains(substr))
            .unwrap_or(false)
    })
}

fn map_id(m: &Value) -> Option<u64> {
    m.get("id").and_then(|v| v.as_u64())
}

/// List all Cilium map IDs for validation.
async fn list_cilium_map_ids() -> Result<Vec<u64>, String> {
    let maps = run_bpftool(&["map", "list", "-j"]).await?;
    let arr = maps.as_array().ok_or("unexpected bpftool output")?;
    Ok(arr
        .iter()
        .filter(|m| {
            m.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.contains("cilium"))
                .unwrap_or(false)
        })
        .filter_map(map_id)
        .collect())
}

// ---------------------------------------------------------------------------
// GET /api/v1/ebpf/attachments — read-only inventory with owner classification
// ---------------------------------------------------------------------------

pub async fn list_attachments(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let progs = match run_bpftool(&["prog", "list", "-j"]).await {
        Ok(v) => v,
        Err(_) => {
            return Ok(Json(json!({
                "source": "unavailable",
                "attachments": [],
                "total": 0,
                "cilium": 0,
                "netra": 0,
                "other": 0,
            })));
        }
    };

    let empty = vec![];
    let arr = progs.as_array().unwrap_or(&empty);
    let mut cilium = 0usize;
    let mut netra = 0usize;
    let mut other = 0usize;
    let items: Vec<Value> = arr
        .iter()
        .filter_map(|p| {
            let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name.is_empty() {
                return None;
            }
            let owner = classify_owner(name);
            match owner {
                "cilium" => cilium += 1,
                "netra" => netra += 1,
                _ => other += 1,
            }
            Some(json!({
                "id": p.get("id"),
                "name": name,
                "owner": owner,
                "type": p.get("type"),
                "tag": p.get("tag"),
            }))
        })
        .collect();
    let total = items.len();

    Ok(Json(json!({
        "source": "bpftool",
        "attachments": items,
        "total": total,
        "cilium": cilium,
        "netra": netra,
        "other": other,
    })))
}

// ---------------------------------------------------------------------------
// GET /api/v1/ebpf/drift — warn-only brotherhood drift findings
// ---------------------------------------------------------------------------

pub async fn get_drift(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let mut findings = Vec::new();
    match run_bpftool(&["prog", "list", "-j"]).await {
        Err(_) => {
            findings.push(json!({
                "kind": "bpf-inventory-unavailable",
                "severity": "info",
                "message": "BPF program inventory unavailable (bpftool missing or failed).",
            }));
        }
        Ok(progs) => {
            let empty = vec![];
            let arr = progs.as_array().unwrap_or(&empty);
            let cilium = arr
                .iter()
                .filter(|p| {
                    let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    !name.is_empty() && classify_owner(name) == "cilium"
                })
                .count();
            if cilium == 0 {
                findings.push(json!({
                    "kind": "bpf-cilium-missing",
                    "severity": "warning",
                    "message": "No cil_* programs visible — Cilium datapath may be absent.",
                }));
            }
        }
    }

    Ok(Json(json!({ "findings": findings })))
}

// ---------------------------------------------------------------------------
// (a) GET /api/v1/ebpf/programs — list real Cilium eBPF programs
// ---------------------------------------------------------------------------

pub async fn list_real_programs(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let progs = match run_bpftool(&["prog", "list", "-j"]).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("ebpf programs inventory unavailable: {}", e);
            return Ok(Json(json!({
                "programs": [],
                "total": 0,
                "limit": params.limit.unwrap_or(50),
                "offset": params.offset.unwrap_or(0),
                "warning": e,
            })));
        }
    };

    let arr = match progs.as_array() {
        Some(a) => a,
        None => {
            return Ok(Json(paginate_json(vec![], &params, "programs")));
        }
    };

    let items: Vec<Value> = arr
        .iter()
        .filter_map(|p| {
            let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
            let ptype = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if name.is_empty() && ptype.is_empty() {
                return None;
            }
            let owner = classify_owner(name);
            // Keep Cilium/Netra and common datapath types; drop opaque noise.
            let keep = owner != "other"
                || matches!(
                    ptype,
                    "sched_cls"
                        | "xdp"
                        | "cgroup_skb"
                        | "cgroup_sock"
                        | "cgroup_device"
                        | "cgroup_sysctl"
                        | "lwt_in"
                        | "lwt_out"
                        | "lwt_xmit"
                        | "sched_act"
                );
            if !keep {
                return None;
            }
            Some(json!({
                "id": p.get("id"),
                "name": name,
                "type": ptype,
                "owner": owner,
                "tag": p.get("tag"),
                "run_cnt": p.get("run_cnt").or_else(|| p.get("run_count")),
                "run_time_ns": p.get("run_time_ns"),
                "bytes_xlated": p.get("bytes_xlated"),
                "bytes_jited": p.get("bytes_jited"),
                "loaded_at": p.get("loaded_at"),
            }))
        })
        .collect();

    Ok(Json(paginate_json(items, &params, "programs")))
}

// ---------------------------------------------------------------------------
// (b) GET /api/v1/ebpf/maps — list real Cilium eBPF maps
// ---------------------------------------------------------------------------

pub async fn list_real_maps(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let maps = match run_bpftool(&["map", "list", "-j"]).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("ebpf maps inventory unavailable: {}", e);
            return Ok(Json(json!({
                "maps": [],
                "total": 0,
                "limit": params.limit.unwrap_or(50),
                "offset": params.offset.unwrap_or(0),
                "warning": e,
            })));
        }
    };

    let empty = vec![];
    let arr = maps.as_array().unwrap_or(&empty);

    let items: Vec<Value> = arr
        .iter()
        .filter_map(|m| {
            let name = m.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name.is_empty() {
                return None;
            }
            let keep = name.contains("cilium")
                || name.contains("cil_")
                || name.starts_with("cil")
                || name.contains("netra");
            if !keep {
                return None;
            }
            Some(json!({
                "id": m.get("id"),
                "name": name,
                "type": m.get("type"),
                "owner": classify_owner(name),
                "bytes_key": m.get("bytes_key"),
                "bytes_value": m.get("bytes_value"),
                "max_entries": m.get("max_entries"),
            }))
        })
        .collect();

    Ok(Json(paginate_json(items, &params, "maps")))
}

// ---------------------------------------------------------------------------
// (c) GET /api/v1/ebpf/programs/:id — full program details
// ---------------------------------------------------------------------------

pub async fn get_program_stats(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<u32>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let id_str = id.to_string();
    match run_bpftool(&["prog", "show", "id", &id_str, "-j"]).await {
        Ok(v) => Ok(Json(v)),
        Err(e) => Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    }
}

// ---------------------------------------------------------------------------
// (d) GET /api/v1/ebpf/maps/:id/entries?limit=50
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct MapEntriesQuery {
    pub limit: Option<usize>,
}

pub async fn dump_map_entries(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Path(id): Path<u32>,
    Query(params): Query<MapEntriesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    // Validate the requested map ID is a known Cilium map
    let valid_ids = match list_cilium_map_ids().await {
        Ok(ids) => ids,
        Err(e) => return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };
    if !valid_ids.contains(&(id as u64)) {
        return Err(error_response(
            StatusCode::NOT_FOUND,
            "Map ID is not a known Cilium map",
        ));
    }

    let id_str = id.to_string();
    let data = match run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
        Ok(v) => v,
        Err(e) => return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    let limit = params.limit.unwrap_or(50).min(200);

    let entries: Vec<Value> = data
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .take(limit)
        .map(|entry| {
            json!({
                "key": entry.get("key"),
                "value": entry.get("value"),
            })
        })
        .collect();

    Ok(Json(json!({
        "entries": entries,
        "total": data.as_array().map(|a| a.len()).unwrap_or(0),
        "limit": limit,
    })))
}

// ---------------------------------------------------------------------------
// (e) GET /api/v1/ebpf/conntrack — parsed conntrack entries
// ---------------------------------------------------------------------------

pub async fn get_conntrack(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let maps = match run_bpftool(&["map", "list", "-j"]).await {
        Ok(v) => v,
        Err(_) => {
            return Ok(Json(json!({
                "entries": [],
                "total": 0,
                "message": "eBPF maps not accessible"
            })));
        }
    };

    let map_arr = maps.as_array().cloned().unwrap_or_default();

    let mut all_entries: Vec<Value> = Vec::new();

    // Look for cilium_ct4_glob* and cilium_ct_any4_ maps
    for m in &map_arr {
        let name = m.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if !name.contains("cilium_ct4_glob") && !name.contains("cilium_ct_any4_") {
            continue;
        }
        let mid = match map_id(m) {
            Some(id) => id,
            None => continue,
        };
        let id_str = mid.to_string();
        let dump = match run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        let entries = dump.as_array().cloned().unwrap_or_default();
        for entry in entries {
            let key_arr = entry
                .get("key")
                .and_then(|k| k.as_array())
                .cloned()
                .unwrap_or_default();
            let val_arr = entry
                .get("value")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            let key_bytes = hex_array_to_bytes(&key_arr);
            let val_bytes = hex_array_to_bytes(&val_arr);

            if key_bytes.len() < 13 {
                continue;
            }

            let dst_ip = bytes_to_ipv4(&key_bytes[0..4]);
            let src_ip = bytes_to_ipv4(&key_bytes[4..8]);
            let dst_port = bytes_to_u16_be(&key_bytes[8..10]);
            let src_port = bytes_to_u16_be(&key_bytes[10..12]);
            let proto = key_bytes[12];

            let rx_packets = if val_bytes.len() >= 8 {
                bytes_to_u64_le(&val_bytes[0..8])
            } else {
                0
            };
            let rx_bytes = if val_bytes.len() >= 16 {
                bytes_to_u64_le(&val_bytes[8..16])
            } else {
                0
            };
            let tx_packets = if val_bytes.len() >= 24 {
                bytes_to_u64_le(&val_bytes[16..24])
            } else {
                0
            };
            let tx_bytes = if val_bytes.len() >= 32 {
                bytes_to_u64_le(&val_bytes[24..32])
            } else {
                0
            };

            all_entries.push(json!({
                "src_ip": src_ip,
                "dst_ip": dst_ip,
                "src_port": src_port,
                "dst_port": dst_port,
                "protocol": protocol_name(proto),
                "protocol_number": proto,
                "rx_packets": rx_packets,
                "tx_packets": tx_packets,
                "rx_bytes": rx_bytes,
                "tx_bytes": tx_bytes,
                "total_packets": rx_packets + tx_packets,
                "total_bytes": rx_bytes + tx_bytes,
            }));
        }
    }

    let limit = params.limit.unwrap_or(100).min(1000);
    let offset = params.offset.unwrap_or(0);
    let total = all_entries.len();
    let page: Vec<Value> = all_entries.into_iter().skip(offset).take(limit).collect();

    Ok(Json(json!({
        "entries": page,
        "total": total,
        "limit": limit,
        "offset": offset,
    })))
}

// ---------------------------------------------------------------------------
// (f) GET /api/v1/ebpf/ipcache — parsed ipcache entries
// ---------------------------------------------------------------------------

pub async fn get_ipcache(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let maps = match run_bpftool(&["map", "list", "-j"]).await {
        Ok(v) => v,
        Err(_) => {
            // bpftool not available — return empty result instead of error
            return Ok(Json(json!({
                "entries": [],
                "total": 0,
                "message": "eBPF maps not accessible (bpftool unavailable or no Cilium agent found)"
            })));
        }
    };
    let map_arr = maps.as_array().cloned().unwrap_or_default();

    let ipcache_map = match find_map_by_name(&map_arr, "cilium_ipcache") {
        Some(m) => m,
        None => {
            return Ok(Json(json!({
                "entries": [],
                "total": 0,
                "message": "cilium_ipcache map not found — Cilium may not be installed"
            })));
        }
    };
    let mid = match map_id(ipcache_map) {
        Some(id) => id,
        None => {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cannot read map id",
            ))
        }
    };

    let id_str = mid.to_string();
    let dump = match run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
        Ok(v) => v,
        Err(e) => return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    let entries_arr = dump.as_array().cloned().unwrap_or_default();
    let mut items: Vec<Value> = Vec::new();

    for entry in &entries_arr {
        let key_arr = entry
            .get("key")
            .and_then(|k| k.as_array())
            .cloned()
            .unwrap_or_default();
        let val_arr = entry
            .get("value")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let key_bytes = hex_array_to_bytes(&key_arr);
        let val_bytes = hex_array_to_bytes(&val_arr);

        let ip = if key_bytes.len() >= 4 {
            bytes_to_ipv4(&key_bytes[0..4])
        } else {
            continue;
        };
        let prefix_len = if key_bytes.len() >= 8 {
            bytes_to_u32_le(&key_bytes[4..8])
        } else {
            32
        };
        let identity = if val_bytes.len() >= 4 {
            bytes_to_u32_le(&val_bytes[0..4])
        } else {
            0
        };

        items.push(json!({
            "ip": ip,
            "prefix_len": prefix_len,
            "identity": identity,
        }));
    }

    Ok(Json(paginate_json(items, &params, "entries")))
}

// ---------------------------------------------------------------------------
// (g) GET /api/v1/ebpf/lb — load-balancer backends
// ---------------------------------------------------------------------------

pub async fn get_lb_backends(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let maps = match run_bpftool(&["map", "list", "-j"]).await {
        Ok(v) => v,
        Err(_) => {
            return Ok(Json(paginate_json(vec![], &params, "services")));
        }
    };
    let map_arr = maps.as_array().cloned().unwrap_or_default();

    let lb_map = match find_map_by_name(&map_arr, "cilium_lb4_serv") {
        Some(m) => m,
        None => {
            return Ok(Json(paginate_json(vec![], &params, "services")));
        }
    };
    let mid = match map_id(lb_map) {
        Some(id) => id,
        None => {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cannot read map id",
            ))
        }
    };

    let id_str = mid.to_string();
    let dump = match run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
        Ok(v) => v,
        Err(e) => return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    let entries_arr = dump.as_array().cloned().unwrap_or_default();
    let mut items: Vec<Value> = Vec::new();

    for entry in &entries_arr {
        let key_arr = entry
            .get("key")
            .and_then(|k| k.as_array())
            .cloned()
            .unwrap_or_default();

        let key_bytes = hex_array_to_bytes(&key_arr);

        let service_ip = if key_bytes.len() >= 4 {
            bytes_to_ipv4(&key_bytes[0..4])
        } else {
            continue;
        };
        let service_port = if key_bytes.len() >= 6 {
            bytes_to_u16_be(&key_bytes[4..6])
        } else {
            0
        };
        let backend_slot = if key_bytes.len() >= 8 {
            bytes_to_u16_be(&key_bytes[6..8])
        } else {
            0
        };
        let proto = if key_bytes.len() >= 9 {
            key_bytes[8]
        } else {
            0
        };

        items.push(json!({
            "service_ip": service_ip,
            "service_port": service_port,
            "backend_slot": backend_slot,
            "protocol": protocol_name(proto),
            "protocol_number": proto,
        }));
    }

    Ok(Json(paginate_json(items, &params, "services")))
}

// ---------------------------------------------------------------------------
// (h) GET /api/v1/ebpf/drops — drop statistics
// ---------------------------------------------------------------------------

/// Cilium's drop reason for a `cilium_metrics` reason code. The codes are the
/// datapath's `DROP_*` values, the same numbers Hubble reports in
/// `drop_reason_desc`, so its enum is the one authority (`POLICY_DENIED` = 133).
fn drop_reason_name(code: u32) -> String {
    use crate::services::hubble_grpc::pb::flow::DropReason;
    i32::try_from(code)
        .ok()
        .and_then(|c| DropReason::try_from(c).ok())
        .filter(|r| *r != DropReason::Unknown)
        .map(|r| r.as_str_name().to_string())
        .unwrap_or_else(|| format!("DROP_{code}"))
}

/// Smallest `cilium_metrics` reason that is a drop. Codes below it count
/// forwarded traffic by how it was handled (0 forwarded, 3 plaintext, 4 decrypt,
/// LB and fragment reasons, ...): they are not drops. Hubble's `DropReason`
/// numbering starts here as well.
const FIRST_DROP_REASON: u32 = 130;

/// One decoded `cilium_metrics` entry.
#[derive(Debug, PartialEq, Eq)]
struct MetricsSample {
    /// Below [`FIRST_DROP_REASON`] this is forwarded traffic, not a drop.
    reason: u32,
    /// `ingress`, `egress` or `unknown`.
    direction: &'static str,
    count: u64,
    bytes: u64,
}

fn metrics_direction(dir: u64) -> &'static str {
    match dir & 0x3 {
        1 => "ingress",
        2 => "egress",
        _ => "unknown",
    }
}

/// `{count, bytes}` from one value: raw bytes (`struct metrics_value`, two
/// little-endian u64) or a BTF-decoded object.
fn metrics_value(v: &Value) -> Option<(u64, u64)> {
    match v {
        Value::Array(a) => {
            let b = hex_array_to_bytes(a);
            (b.len() >= 8).then(|| {
                (
                    bytes_to_u64_le(&b[0..8]),
                    b.get(8..).map_or(0, bytes_to_u64_le),
                )
            })
        }
        Value::Object(o) => Some((
            o.get("count").and_then(Value::as_u64).unwrap_or(0),
            o.get("bytes").and_then(Value::as_u64).unwrap_or(0),
        )),
        _ => None,
    }
}

/// Decode one entry of `bpftool map dump id <cilium_metrics> -j`.
///
/// The key is `{ u8 reason; u8 dir:2; ... }`: the reason is the first byte only
/// (reading four bytes would fold the direction into it). The map is per-CPU,
/// so bpftool lists one value per CPU under `values`; they are summed.
fn parse_metrics_entry(entry: &Value) -> Option<MetricsSample> {
    let (reason, dir) = match entry.get("key")? {
        Value::Array(a) => {
            let b = hex_array_to_bytes(a);
            (
                u32::from(*b.first()?),
                u64::from(b.get(1).copied().unwrap_or(0)),
            )
        }
        Value::Object(o) => (
            u32::try_from(o.get("reason")?.as_u64()?).ok()?,
            o.get("dir").and_then(Value::as_u64).unwrap_or(0),
        ),
        _ => return None,
    };

    let (count, bytes) = if let Some(per_cpu) = entry.get("values").and_then(Value::as_array) {
        per_cpu
            .iter()
            .filter_map(|c| metrics_value(c.get("value")?))
            .fold((0u64, 0u64), |(n, b), (cn, cb)| {
                (n.saturating_add(cn), b.saturating_add(cb))
            })
    } else {
        metrics_value(entry.get("value")?)?
    };

    Some(MetricsSample {
        reason,
        direction: metrics_direction(dir),
        count,
        bytes,
    })
}

pub async fn get_drop_stats(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(params): Query<PaginationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let maps = match run_bpftool(&["map", "list", "-j"]).await {
        Ok(v) => v,
        Err(_) => {
            let mut r = paginate_json(vec![], &params, "drops");
            r.as_object_mut()
                .map(|o| o.insert("total_drops".to_string(), json!(0)));
            return Ok(Json(r));
        }
    };
    let map_arr = maps.as_array().cloned().unwrap_or_default();

    let metrics_map = match find_map_by_name(&map_arr, "cilium_metrics") {
        Some(m) => m,
        None => {
            let mut r = paginate_json(vec![], &params, "drops");
            r.as_object_mut()
                .map(|o| o.insert("total_drops".to_string(), json!(0)));
            return Ok(Json(r));
        }
    };
    let mid = match map_id(metrics_map) {
        Some(id) => id,
        None => {
            return Err(error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "cannot read map id",
            ))
        }
    };

    let id_str = mid.to_string();
    let dump = match run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
        Ok(v) => v,
        Err(e) => return Err(error_response(StatusCode::INTERNAL_SERVER_ERROR, &e)),
    };

    let entries_arr = dump.as_array().cloned().unwrap_or_default();
    let mut items: Vec<Value> = Vec::new();

    // Only reasons from FIRST_DROP_REASON up are drops.
    for sample in entries_arr
        .iter()
        .filter_map(parse_metrics_entry)
        .filter(|m| m.reason >= FIRST_DROP_REASON)
    {
        items.push(json!({
            "reason": drop_reason_name(sample.reason),
            "reason_code": sample.reason,
            "direction": sample.direction,
            "count": sample.count,
            "bytes": sample.bytes,
        }));
    }

    let mut result = paginate_json(items, &params, "drops");
    // Add total_drops alias — the frontend expects this field alongside "total"
    if let Some(total) = result.get("total").cloned() {
        result
            .as_object_mut()
            .unwrap()
            .insert("total_drops".to_string(), total);
    }
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// (i) GET /api/v1/ebpf/summary — aggregate overview
//
// Default is counts-only (fast). Pass ?detail=full for CT dumps + drop metrics
// (expensive kubectl/bpftool map dumps — not needed on Overview).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct EbpfSummaryQuery {
    /// `full` enables CT/map dumps; anything else stays slim.
    pub detail: Option<String>,
}

pub async fn get_ebpf_summary(
    State(state): State<Arc<AppState>>,
    claims: Option<axum::Extension<crate::middleware::auth::Claims>>,
    Query(q): Query<EbpfSummaryQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    check_admin(&state, &claims)?;

    let detail_full = q
        .detail
        .as_deref()
        .map(|d| d.eq_ignore_ascii_case("full"))
        .unwrap_or(false);

    // Programs + maps in parallel (cached bpftool).
    let (progs_res, maps_res) = tokio::join!(
        run_bpftool(&["prog", "list", "-j"]),
        run_bpftool(&["map", "list", "-j"]),
    );

    let total_programs = progs_res
        .ok()
        .and_then(|v| v.as_array().cloned())
        .map(|a| {
            a.iter()
                .filter(|p| {
                    let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("");
                    let ptype = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    let owner = classify_owner(name);
                    owner != "other"
                        || matches!(
                            ptype,
                            "sched_cls"
                                | "xdp"
                                | "cgroup_skb"
                                | "cgroup_sock"
                                | "cgroup_device"
                                | "cgroup_sysctl"
                                | "lwt_in"
                                | "lwt_out"
                                | "lwt_xmit"
                                | "sched_act"
                        )
                })
                .count()
        })
        .unwrap_or(0);

    let map_arr = maps_res
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();

    let total_maps = map_arr
        .iter()
        .filter(|m| {
            m.get("name")
                .and_then(|n| n.as_str())
                .map(|n| n.contains("cilium") || n.contains("cil_") || n.starts_with("cil"))
                .unwrap_or(false)
        })
        .count();

    if !detail_full {
        return Ok(Json(json!({
            "total_programs": total_programs,
            "total_maps": total_maps,
            "total_ct_entries": 0,
            "total_drops": 0,
            "top_drop_reason": "n/a",
            "detail": "slim",
        })));
    }

    // Full detail: CT dumps + metrics map (slow — Diagnostics only).
    let mut total_ct_entries: usize = 0;
    for m in &map_arr {
        let name = m.get("name").and_then(|n| n.as_str()).unwrap_or("");
        if name.contains("cilium_ct4_glob") || name.contains("cilium_ct_any4_") {
            if let Some(id) = map_id(m) {
                let id_str = id.to_string();
                if let Ok(dump) = run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
                    total_ct_entries += dump.as_array().map(|a| a.len()).unwrap_or(0);
                }
            }
        }
    }

    let mut total_drops: u64 = 0;
    let mut top_drop_reason = "None".to_string();
    let mut top_drop_count: u64 = 0;

    if let Some(metrics_map) = find_map_by_name(&map_arr, "cilium_metrics") {
        if let Some(mid) = map_id(metrics_map) {
            let id_str = mid.to_string();
            if let Ok(dump) = run_bpftool(&["map", "dump", "id", &id_str, "-j"]).await {
                for sample in dump
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .filter_map(parse_metrics_entry)
                    .filter(|m| m.reason >= FIRST_DROP_REASON)
                {
                    total_drops += sample.count;
                    if sample.count > top_drop_count {
                        top_drop_count = sample.count;
                        top_drop_reason = drop_reason_name(sample.reason);
                    }
                }
            }
        }
    }

    Ok(Json(json!({
        "total_programs": total_programs,
        "total_maps": total_maps,
        "total_ct_entries": total_ct_entries,
        "total_drops": total_drops,
        "top_drop_reason": top_drop_reason,
        "detail": "full",
    })))
}

#[cfg(test)]
mod tests {
    use super::classify_owner;

    #[test]
    fn classify_brotherhood_owners() {
        assert_eq!(classify_owner("cil_from_container"), "cilium");
        assert_eq!(classify_owner("cilium_host"), "cilium");
        assert_eq!(classify_owner("netra_tcx_ingress"), "netra");
        assert_eq!(classify_owner("custom_xdp"), "other");
    }
}

#[cfg(test)]
mod metrics_tests {
    use super::*;
    use serde_json::json;

    fn hex(bytes: &[u8]) -> Value {
        Value::Array(bytes.iter().map(|b| json!(format!("0x{b:02x}"))).collect())
    }
    fn value_bytes(count: u64, bytes: u64) -> Vec<u8> {
        [count.to_le_bytes(), bytes.to_le_bytes()].concat()
    }

    #[test]
    fn the_reason_is_the_first_key_byte_not_four_bytes() {
        // reason 133 (policy denied), dir 1 (ingress): as u32 this would be 389.
        let entry = json!({
            "key": hex(&[133, 1, 0, 0, 0, 0, 0, 0]),
            "value": hex(&value_bytes(5, 700)),
        });
        assert_eq!(
            parse_metrics_entry(&entry),
            Some(MetricsSample {
                reason: 133,
                direction: "ingress",
                count: 5,
                bytes: 700
            })
        );
    }

    #[test]
    fn direction_uses_only_the_low_two_bits() {
        let entry = json!({ "key": hex(&[133, 0b1111_1110]), "value": hex(&value_bytes(1, 1)) });
        assert_eq!(parse_metrics_entry(&entry).unwrap().direction, "egress");
        let entry = json!({ "key": hex(&[133, 0]), "value": hex(&value_bytes(1, 1)) });
        assert_eq!(parse_metrics_entry(&entry).unwrap().direction, "unknown");
    }

    #[test]
    fn per_cpu_values_are_summed() {
        let entry = json!({
            "key": hex(&[181, 2, 0, 0, 0, 0, 0, 0]),
            "values": [
                { "cpu": 0, "value": hex(&value_bytes(3, 30)) },
                { "cpu": 1, "value": hex(&value_bytes(4, 40)) },
                { "cpu": 2, "value": hex(&value_bytes(0, 0)) },
            ],
        });
        let s = parse_metrics_entry(&entry).unwrap();
        assert_eq!(
            (s.reason, s.direction, s.count, s.bytes),
            (181, "egress", 7, 70)
        );
    }

    #[test]
    fn btf_decoded_entries_are_understood() {
        let entry = json!({
            "key": { "reason": 133, "dir": 1 },
            "value": { "count": 9, "bytes": 90 },
        });
        assert_eq!(
            parse_metrics_entry(&entry),
            Some(MetricsSample {
                reason: 133,
                direction: "ingress",
                count: 9,
                bytes: 90
            })
        );
    }

    #[test]
    fn forwarded_traffic_decodes_but_is_not_a_drop() {
        // Seen on a live node: reason 3 (plaintext) carried 66M packets and was
        // reported as a "drop" until only reasons >= 130 counted.
        for reason in [0u8, 3, 4, 129] {
            let entry = json!({ "key": hex(&[reason, 1]), "value": hex(&value_bytes(100, 1)) });
            let s = parse_metrics_entry(&entry).unwrap();
            assert_eq!(s.reason, u32::from(reason));
            assert!(s.reason < FIRST_DROP_REASON, "{reason}");
        }
        let drop = json!({ "key": hex(&[133, 1]), "value": hex(&value_bytes(1, 1)) });
        assert!(parse_metrics_entry(&drop).unwrap().reason >= FIRST_DROP_REASON);
    }

    #[test]
    fn malformed_entries_are_skipped_not_guessed() {
        assert!(parse_metrics_entry(&json!({})).is_none());
        assert!(
            parse_metrics_entry(&json!({ "key": hex(&[]), "value": hex(&value_bytes(1, 1)) }))
                .is_none()
        );
        assert!(
            parse_metrics_entry(&json!({ "key": hex(&[133, 1]), "value": hex(&[1, 2]) })).is_none()
        );
        assert!(parse_metrics_entry(&json!({ "key": "nope", "value": [] })).is_none());
    }

    #[test]
    fn drop_reasons_use_ciliums_names() {
        assert_eq!(drop_reason_name(133), "POLICY_DENIED");
        assert_eq!(drop_reason_name(132), "INVALID_SOURCE_IP");
        assert_eq!(drop_reason_name(9999), "DROP_9999");
        // Not a drop reason: no Hubble name, so a code, never an invented label.
        assert_eq!(drop_reason_name(3), "DROP_3");
    }
}
