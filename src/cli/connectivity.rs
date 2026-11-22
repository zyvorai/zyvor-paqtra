// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra connectivity test`: prove, end to end, that Cilium enforces policy
//! and that Paqtra sees it, like `cilium connectivity test`.
//!
//! It deploys a temporary echo server and short-lived probe pods in its own
//! namespace, applies a `CiliumNetworkPolicy` through the CRD (the only way
//! Paqtra enforces anything), checks who can and cannot connect, and, when
//! given an API token, that the resulting flows (FORWARDED / DROPPED) reach the
//! Paqtra API. Everything it creates is labelled and removed afterwards, also
//! on Ctrl-C.

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Namespace, Pod, Service};
use kube::api::{DeleteParams, DynamicObject, PostParams};
use kube::core::GroupVersionKind;
use kube::discovery::ApiResource;
use kube::{Api, Client};
use owo_colors::OwoColorize;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};

use super::global::Global;
use super::portforward;

pub const NAMESPACE: &str = "paqtra-connectivity-test";
const LABEL_KEY: &str = "paqtra.io/connectivity-test";
pub const DEFAULT_IMAGE: &str = "registry.k8s.io/e2e-test-images/agnhost:2.53";
const POLICY_NAME: &str = "paqtra-connectivity-allow-only-allowed-client";
/// One probe's own connect timeout: a dropped SYN never answers.
const PROBE_CONNECT_SECS: u64 = 5;

/// Independent scenarios, in the order `all` runs them.
pub const ALL_TESTS: &[&str] = &["baseline", "enforcement", "restore", "flows"];

#[derive(Debug, Clone)]
pub struct ConnOpts {
    pub tests: Vec<String>,
    pub timeout: Duration,
    pub cleanup: bool,
    pub image: String,
    pub registry: Option<String>,
    /// JWT for the Paqtra API; without it the `flows` scenario is skipped.
    pub api_token: Option<String>,
    pub output: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub skipped: bool,
    pub detail: String,
    pub seconds: u64,
}

// ─── Pure: manifests and judgements ────────────────────────────────────────

/// `--registry mirror.corp/k8s` -> `mirror.corp/k8s/agnhost:2.53`.
pub fn image_for(image: &str, registry: Option<&str>) -> String {
    match registry {
        Some(prefix) => {
            let leaf = image.rsplit('/').next().unwrap_or(image);
            format!("{}/{leaf}", prefix.trim_end_matches('/'))
        }
        None => image.to_string(),
    }
}

pub fn validate_tests(names: &[String]) -> Result<Vec<String>> {
    if names.is_empty() {
        return Ok(ALL_TESTS.iter().map(|s| s.to_string()).collect());
    }
    for n in names {
        if !ALL_TESTS.contains(&n.as_str()) {
            bail!("unknown test {n:?}; choose from {}", ALL_TESTS.join(", "));
        }
    }
    Ok(names.to_vec())
}

fn labels(extra: &[(&str, &str)]) -> Value {
    let mut m = serde_json::Map::new();
    m.insert(LABEL_KEY.into(), json!("true"));
    for (k, v) in extra {
        m.insert((*k).into(), json!(v));
    }
    Value::Object(m)
}

pub fn namespace_manifest() -> Value {
    json!({"apiVersion": "v1", "kind": "Namespace",
        "metadata": {"name": NAMESPACE, "labels": labels(&[])}})
}

pub fn server_deployment(image: &str) -> Value {
    json!({"apiVersion": "apps/v1", "kind": "Deployment",
    "metadata": {"name": "echo-server", "namespace": NAMESPACE, "labels": labels(&[("app", "echo-server")])},
    "spec": {"replicas": 1,
        "selector": {"matchLabels": {"app": "echo-server"}},
        "template": {
            "metadata": {"labels": labels(&[("app", "echo-server")])},
            "spec": {"containers": [{
                "name": "echo", "image": image,
                "args": ["netexec", "--http-port=8080"],
                "ports": [{"containerPort": 8080}],
                "readinessProbe": {"httpGet": {"path": "/hostname", "port": 8080}, "periodSeconds": 2},
                "securityContext": {"allowPrivilegeEscalation": false, "runAsNonRoot": true, "runAsUser": 1000,
                    "capabilities": {"drop": ["ALL"]}, "seccompProfile": {"type": "RuntimeDefault"}},
            }]}}}})
}

pub fn server_service() -> Value {
    json!({"apiVersion": "v1", "kind": "Service",
        "metadata": {"name": "echo-server", "namespace": NAMESPACE, "labels": labels(&[])},
        "spec": {"selector": {"app": "echo-server"}, "ports": [{"port": 80, "targetPort": 8080}]}})
}

/// A pod that tries one TCP connection to the server and exits 0 on success.
pub fn probe_pod(name: &str, client_label: &str, image: &str) -> Value {
    json!({"apiVersion": "v1", "kind": "Pod",
    "metadata": {"name": name, "namespace": NAMESPACE, "labels": labels(&[("app", client_label)])},
    "spec": {"restartPolicy": "Never",
        "containers": [{
            "name": "probe", "image": image,
            "args": ["connect", &format!("echo-server.{NAMESPACE}.svc:80"), &format!("--timeout={PROBE_CONNECT_SECS}s")],
            "securityContext": {"allowPrivilegeEscalation": false, "runAsNonRoot": true, "runAsUser": 1000,
                "capabilities": {"drop": ["ALL"]}, "seccompProfile": {"type": "RuntimeDefault"}},
        }]}})
}

/// Only pods labelled `app=allowed-client` may reach the server.
pub fn allow_policy() -> Value {
    json!({"apiVersion": "cilium.io/v2", "kind": "CiliumNetworkPolicy",
        "metadata": {"name": POLICY_NAME, "namespace": NAMESPACE, "labels": labels(&[])},
        "spec": {
            "endpointSelector": {"matchLabels": {"app": "echo-server"}},
            "ingress": [{"fromEndpoints": [{"matchLabels": {"app": "allowed-client"}}]}]}})
}

/// What the Paqtra API saw of the probes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Evidence {
    pub dropped: bool,
    pub forwarded: bool,
}

/// From `GET /api/v1/flows`: did it record the denied client's drop and the
/// allowed client's forwarded flow toward the echo server?
pub fn flows_evidence(body: &Value, denied_prefix: &str, allowed_prefix: &str) -> Evidence {
    let mut e = Evidence::default();
    let Some(flows) = body.get("flows").and_then(Value::as_array) else {
        return e;
    };
    for f in flows {
        let src = f
            .pointer("/source/pod")
            .and_then(Value::as_str)
            .unwrap_or("");
        let dst = f
            .pointer("/destination/pod")
            .and_then(Value::as_str)
            .unwrap_or("");
        let verdict = f.get("verdict").and_then(Value::as_str).unwrap_or("");
        if !dst.starts_with("echo-server") {
            continue;
        }
        if src.starts_with(denied_prefix) && verdict == "DROPPED" {
            e.dropped = true;
        }
        if src.starts_with(allowed_prefix) && verdict == "FORWARDED" {
            e.forwarded = true;
        }
    }
    e
}

/// The API request for flows in the test namespace since `since`. It uses the
/// time-bounded history endpoint on purpose: `/flows?namespace=` filters the
/// whole store, and on a cluster with millions of stored flows that is a full
/// scan. A time range is an index lookup, so this stays cheap however big the
/// store is (and however often it is polled).
pub fn flows_url(port: u16, since: DateTime<Utc>) -> String {
    format!(
        "http://127.0.0.1:{port}/api/v1/flows/history?namespace={NAMESPACE}&from={}&limit=500",
        since.format("%Y-%m-%dT%H:%M:%SZ")
    )
}

pub fn render_results(results: &[TestResult]) -> String {
    let mut out = String::new();
    for r in results {
        let mark = if r.skipped {
            "○".dimmed().to_string()
        } else if r.passed {
            "✔".green().to_string()
        } else {
            "✘".red().to_string()
        };
        out.push_str(&format!(
            "  {mark} {:14} {} {}\n",
            r.name,
            r.detail,
            format!("({}s)", r.seconds).dimmed()
        ));
    }
    out
}

pub fn all_passed(results: &[TestResult]) -> bool {
    results.iter().all(|r| r.passed || r.skipped)
}

// ─── Cluster operations ────────────────────────────────────────────────────

fn policy_api(client: &Client) -> Api<DynamicObject> {
    let ar = ApiResource::from_gvk_with_plural(
        &GroupVersionKind::gvk("cilium.io", "v2", "CiliumNetworkPolicy"),
        "ciliumnetworkpolicies",
    );
    Api::namespaced_with(client.clone(), NAMESPACE, &ar)
}

async fn create<K>(api: &Api<K>, manifest: Value) -> Result<()>
where
    K: Clone + serde::de::DeserializeOwned + std::fmt::Debug + Serialize,
{
    let obj: K = serde_json::from_value(manifest)?;
    match api.create(&PostParams::default(), &obj).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(e)) if e.code == 409 => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn wait_until<F, Fut>(what: &str, timeout: Duration, mut check: F) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    let start = Instant::now();
    loop {
        if check().await {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!("timed out after {}s waiting for {what}", timeout.as_secs());
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn setup(client: &Client, image: &str, timeout: Duration) -> Result<()> {
    create(&Api::<Namespace>::all(client.clone()), namespace_manifest())
        .await
        .context("cannot create the test namespace")?;
    create(
        &Api::<Deployment>::namespaced(client.clone(), NAMESPACE),
        server_deployment(image),
    )
    .await?;
    create(
        &Api::<Service>::namespaced(client.clone(), NAMESPACE),
        server_service(),
    )
    .await?;
    let deployments: Api<Deployment> = Api::namespaced(client.clone(), NAMESPACE);
    wait_until(
        "the echo server to become ready (image pull?)",
        timeout,
        || async {
            deployments
                .get_opt("echo-server")
                .await
                .ok()
                .flatten()
                .and_then(|d| d.status)
                .and_then(|s| s.ready_replicas)
                .unwrap_or(0)
                >= 1
        },
    )
    .await
}

/// One connection attempt from a fresh pod. `Ok(true)` = connected.
async fn probe(
    client: &Client,
    name: &str,
    client_label: &str,
    image: &str,
    timeout: Duration,
) -> Result<bool> {
    let pods: Api<Pod> = Api::namespaced(client.clone(), NAMESPACE);
    let _ = pods.delete(name, &DeleteParams::default()).await;
    create(&pods, probe_pod(name, client_label, image)).await?;
    let start = Instant::now();
    let outcome = loop {
        let phase = pods
            .get_opt(name)
            .await?
            .and_then(|p| p.status)
            .and_then(|s| s.phase)
            .unwrap_or_default();
        match phase.as_str() {
            "Succeeded" => break Ok(true),
            "Failed" => break Ok(false),
            _ if start.elapsed() > timeout => {
                break Err(anyhow::anyhow!(
                    "probe pod {name} did not finish in {}s",
                    timeout.as_secs()
                ))
            }
            _ => tokio::time::sleep(Duration::from_secs(1)).await,
        }
    };
    let _ = pods
        .delete(
            name,
            &DeleteParams {
                grace_period_seconds: Some(0),
                ..Default::default()
            },
        )
        .await;
    outcome
}

/// Keep probing until the result matches `want` (a policy takes a moment to be
/// realised, and a probe can race it).
async fn probe_until(
    client: &Client,
    name: &str,
    label: &str,
    image: &str,
    want: bool,
    timeout: Duration,
) -> Result<()> {
    let start = Instant::now();
    loop {
        let got = probe(
            client,
            name,
            label,
            image,
            Duration::from_secs(PROBE_CONNECT_SECS + 60),
        )
        .await?;
        if got == want {
            return Ok(());
        }
        if start.elapsed() > timeout {
            bail!(
                "{label} {} the echo server, expected {}",
                if got {
                    "could reach"
                } else {
                    "could not reach"
                },
                if want {
                    "it to connect"
                } else {
                    "it to be blocked"
                }
            );
        }
    }
}

async fn apply_policy(client: &Client) -> Result<()> {
    let api = policy_api(client);
    let obj: DynamicObject = serde_json::from_value(allow_policy())?;
    match api.create(&PostParams::default(), &obj).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(e)) if e.code == 409 => Ok(()),
        Err(e) => Err(anyhow::anyhow!(
            "cannot create the CiliumNetworkPolicy: {e} (is Cilium installed?)"
        )),
    }
}

async fn remove_policy(client: &Client) {
    let _ = policy_api(client)
        .delete(POLICY_NAME, &DeleteParams::default())
        .await;
}

async fn run_one(
    name: &str,
    client: &Client,
    o: &ConnOpts,
    image: &str,
    g: &Global,
    since: DateTime<Utc>,
) -> Result<String> {
    match name {
        "baseline" => {
            remove_policy(client).await;
            probe_until(
                client,
                "probe-denied",
                "denied-client",
                image,
                true,
                o.timeout,
            )
            .await?;
            probe_until(
                client,
                "probe-allowed",
                "allowed-client",
                image,
                true,
                o.timeout,
            )
            .await?;
            Ok("both clients connect with no policy".into())
        }
        "enforcement" => {
            apply_policy(client).await?;
            probe_until(
                client,
                "probe-denied",
                "denied-client",
                image,
                false,
                o.timeout,
            )
            .await?;
            probe_until(
                client,
                "probe-allowed",
                "allowed-client",
                image,
                true,
                o.timeout,
            )
            .await?;
            Ok("policy blocks the other client and admits the allowed one".into())
        }
        "restore" => {
            apply_policy(client).await?;
            remove_policy(client).await;
            probe_until(
                client,
                "probe-denied",
                "denied-client",
                image,
                true,
                o.timeout,
            )
            .await?;
            Ok("deleting the policy restores connectivity".into())
        }
        "flows" => {
            let token = o.api_token.as_deref().context("no API token")?;
            apply_policy(client).await?;
            probe_until(
                client,
                "probe-denied",
                "denied-client",
                image,
                false,
                o.timeout,
            )
            .await?;
            probe_until(
                client,
                "probe-allowed",
                "allowed-client",
                image,
                true,
                o.timeout,
            )
            .await?;
            check_flows(client, g, token, o.timeout, since).await
        }
        other => bail!("unknown test {other}"),
    }
}

/// Ask the Paqtra API (through a temporary port-forward: the Kubernetes API
/// proxy would not pass our bearer token on) until both flows are indexed.
async fn check_flows(
    client: &Client,
    g: &Global,
    token: &str,
    timeout: Duration,
    since: DateTime<Utc>,
) -> Result<String> {
    let selector = format!(
        "app.kubernetes.io/instance={},app.kubernetes.io/component=api",
        g.release
    );
    let pod = portforward::find_pod(client, &g.namespace, &selector).await?;
    let pods: Api<Pod> = Api::namespaced(client.clone(), &g.namespace);
    let port_remote = pods
        .get(&pod)
        .await?
        .spec
        .and_then(|s| s.containers.into_iter().next())
        .and_then(|c| c.ports)
        .and_then(|p| p.first().map(|p| p.container_port as u16))
        .unwrap_or(9191);
    let (local, handle) =
        portforward::spawn(client.clone(), &g.namespace, &pod, port_remote).await?;
    let url = flows_url(local, since);
    let token = token.to_string();

    let start = Instant::now();
    let result = loop {
        let (url, token) = (url.clone(), token.clone());
        let body = tokio::task::spawn_blocking(move || -> Result<Value> {
            let mut resp = ureq::get(&url)
                .header("Authorization", format!("Bearer {token}"))
                .call()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            let text = resp.body_mut().read_to_string()?;
            Ok(serde_json::from_str(&text)?)
        })
        .await?;
        match body {
            Ok(v) => {
                let e = flows_evidence(&v, "probe-denied", "probe-allowed");
                if e.dropped && e.forwarded {
                    break Ok("Paqtra recorded the DROPPED and the FORWARDED flow".to_string());
                }
                if start.elapsed() > timeout {
                    break Err(anyhow::anyhow!(
                        "flows did not show up in Paqtra within {}s (dropped seen: {}, forwarded seen: {}); check `paqtra doctor` for Hubble ingest",
                        timeout.as_secs(), e.dropped, e.forwarded
                    ));
                }
            }
            Err(e) => {
                break Err(anyhow::anyhow!(
                    "cannot query the Paqtra API: {e} (is the token valid?)"
                ))
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    };
    handle.abort();
    result
}

async fn cleanup(client: &Client) {
    let _ = Api::<Namespace>::all(client.clone())
        .delete(NAMESPACE, &DeleteParams::default())
        .await;
}

/// Runs the selected scenarios; returns whether all passed.
pub async fn cmd_connectivity_test(g: &Global, o: ConnOpts) -> Result<bool> {
    let tests = validate_tests(&o.tests)?;
    // Flows older than this run are of no interest (and cost the API to find).
    let since = Utc::now() - chrono::Duration::seconds(30);
    let client = super::kube::client(g).await?;
    let image = image_for(&o.image, o.registry.as_deref());
    let json = o.output == "json";

    if !json {
        println!(
            "{} Connectivity test in namespace {} (image {})",
            "→".cyan(),
            NAMESPACE.bold(),
            image.dimmed()
        );
    }
    if let Err(e) = setup(&client, &image, o.timeout.max(Duration::from_secs(120))).await {
        cleanup(&client).await;
        bail!("setup failed: {e:#}");
    }

    let mut results: Vec<TestResult> = Vec::new();
    let body = async {
        for name in &tests {
            let start = Instant::now();
            if name == "flows" && o.api_token.is_none() {
                results.push(TestResult {
                    name: name.clone(),
                    passed: false,
                    skipped: true,
                    detail: "skipped: pass --api-token (or PAQTRA_API_TOKEN) to check flows reach Paqtra".into(),
                    seconds: 0,
                });
                continue;
            }
            let outcome = run_one(name, &client, &o, &image, g, since).await;
            let (passed, detail) = match outcome {
                Ok(d) => (true, d),
                Err(e) => (false, format!("{e:#}")),
            };
            results.push(TestResult {
                name: name.clone(),
                passed,
                skipped: false,
                detail,
                seconds: start.elapsed().as_secs(),
            });
        }
    };
    // Ctrl-C must not leave a namespace behind.
    tokio::select! {
        _ = body => {}
        _ = tokio::signal::ctrl_c() => { eprintln!("interrupted; cleaning up..."); }
    }

    if o.cleanup {
        cleanup(&client).await;
    } else if !json {
        println!("  namespace {NAMESPACE} kept (--no-cleanup); remove it with `kubectl delete ns {NAMESPACE}`");
    }

    let ok = !results.is_empty() && all_passed(&results);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"ok": ok, "results": results}))?
        );
    } else {
        print!("{}", render_results(&results));
        println!();
        if ok {
            println!("{} All connectivity tests passed", "✔".green());
        } else {
            println!("{} Connectivity tests failed", "✖".red());
        }
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_mirrors_replace_the_prefix() {
        assert_eq!(image_for(DEFAULT_IMAGE, None), DEFAULT_IMAGE);
        assert_eq!(
            image_for(DEFAULT_IMAGE, Some("mirror.corp/k8s")),
            "mirror.corp/k8s/agnhost:2.53"
        );
        assert_eq!(
            image_for(DEFAULT_IMAGE, Some("mirror.corp/k8s/")),
            "mirror.corp/k8s/agnhost:2.53"
        );
    }

    #[test]
    fn test_selection() {
        assert_eq!(validate_tests(&[]).unwrap(), ALL_TESTS);
        assert_eq!(validate_tests(&["restore".into()]).unwrap(), ["restore"]);
        assert!(validate_tests(&["nope".into()])
            .unwrap_err()
            .to_string()
            .contains("choose from"));
    }

    #[test]
    fn everything_created_is_labelled_for_cleanup_and_confined_to_one_namespace() {
        for m in [
            namespace_manifest(),
            server_deployment("i"),
            server_service(),
            probe_pod("p", "denied-client", "i"),
            allow_policy(),
        ] {
            assert_eq!(
                m.pointer("/metadata/labels").and_then(|l| l.get(LABEL_KEY)),
                Some(&json!("true")),
                "{m}"
            );
            if m["kind"] != "Namespace" {
                assert_eq!(m.pointer("/metadata/namespace"), Some(&json!(NAMESPACE)));
            }
        }
    }

    #[test]
    fn manifests_deserialize_into_the_typed_objects_the_cluster_expects() {
        serde_json::from_value::<Namespace>(namespace_manifest()).unwrap();
        serde_json::from_value::<Deployment>(server_deployment("img")).unwrap();
        serde_json::from_value::<Service>(server_service()).unwrap();
        let pod: Pod =
            serde_json::from_value(probe_pod("probe-x", "allowed-client", "img")).unwrap();
        let args = pod.spec.unwrap().containers[0].args.clone().unwrap();
        assert_eq!(args[0], "connect");
        assert!(args[1].contains("echo-server") && args[1].ends_with(":80"));
        assert_eq!(args[2], "--timeout=5s");
        serde_json::from_value::<DynamicObject>(allow_policy()).unwrap();
    }

    #[test]
    fn probes_and_server_run_unprivileged() {
        for m in [server_deployment("i"), probe_pod("p", "x", "i")] {
            let sc = m
                .pointer("/spec/template/spec/containers/0/securityContext")
                .or_else(|| m.pointer("/spec/containers/0/securityContext"))
                .unwrap();
            assert_eq!(sc["allowPrivilegeEscalation"], false);
            assert_eq!(sc["runAsNonRoot"], true);
        }
    }

    #[test]
    fn the_policy_admits_only_the_allowed_client() {
        let p = allow_policy();
        assert_eq!(
            p.pointer("/spec/endpointSelector/matchLabels/app"),
            Some(&json!("echo-server"))
        );
        assert_eq!(
            p.pointer("/spec/ingress/0/fromEndpoints/0/matchLabels/app"),
            Some(&json!("allowed-client"))
        );
        assert_eq!(p["spec"]["ingress"].as_array().unwrap().len(), 1);
    }

    fn flow(src: &str, dst: &str, verdict: &str) -> Value {
        json!({"source": {"pod": src}, "destination": {"pod": dst}, "verdict": verdict})
    }

    #[test]
    fn flow_evidence_needs_the_right_verdict_for_the_right_client() {
        let both = json!({"flows": [
            flow("probe-denied", "echo-server-abc", "DROPPED"),
            flow("probe-allowed", "echo-server-abc", "FORWARDED"),
        ]});
        assert_eq!(
            flows_evidence(&both, "probe-denied", "probe-allowed"),
            Evidence {
                dropped: true,
                forwarded: true
            }
        );

        // Wrong way round: the denied client forwarded, the allowed one dropped.
        let swapped = json!({"flows": [
            flow("probe-denied", "echo-server-abc", "FORWARDED"),
            flow("probe-allowed", "echo-server-abc", "DROPPED"),
        ]});
        assert_eq!(
            flows_evidence(&swapped, "probe-denied", "probe-allowed"),
            Evidence::default()
        );

        // Flows to anything else do not count.
        let other = json!({"flows": [flow("probe-denied", "coredns-1", "DROPPED")]});
        assert_eq!(
            flows_evidence(&other, "probe-denied", "probe-allowed"),
            Evidence::default()
        );
        assert_eq!(flows_evidence(&json!({}), "a", "b"), Evidence::default());
        assert_eq!(
            flows_evidence(&json!({"flows": "no"}), "a", "b"),
            Evidence::default()
        );
    }

    #[test]
    fn the_flows_request_is_time_bounded() {
        let since = DateTime::parse_from_rfc3339("2026-09-26T04:26:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let url = flows_url(19191, since);
        assert_eq!(
            url,
            "http://127.0.0.1:19191/api/v1/flows/history?namespace=paqtra-connectivity-test&from=2026-09-26T04:26:00Z&limit=500"
        );
        // Never the unbounded list: that one scans the whole store.
        assert!(url.contains("/flows/history") && url.contains("from="));
    }

    fn res(name: &str, passed: bool, skipped: bool) -> TestResult {
        TestResult {
            name: name.into(),
            passed,
            skipped,
            detail: "d".into(),
            seconds: 1,
        }
    }

    #[test]
    fn skipped_tests_do_not_fail_the_run_but_failures_do() {
        assert!(all_passed(&[
            res("a", true, false),
            res("flows", false, true)
        ]));
        assert!(!all_passed(&[
            res("a", true, false),
            res("b", false, false)
        ]));
        assert!(
            all_passed(&[]),
            "the caller separately requires at least one result"
        );
        let text = render_results(&[res("baseline", true, false), res("flows", false, true)]);
        assert!(text.contains("baseline") && text.contains("flows"));
    }
}
