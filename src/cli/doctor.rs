// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

//! `paqtra doctor`: is this installation healthy, and if not, what to do?
//! Goes beyond `status` (are pods up?) to the causes people actually hit:
//! crash loops, OOM kills, an agent missing from a node, Hubble ingest lag.
//! Every judgement is a pure function of what was observed.

use anyhow::Result;
use chrono::{DateTime, Utc};
use k8s_openapi::api::core::v1::{ConfigMap, Node, Pod};
use kube::api::ListParams;
use kube::Api;
use owo_colors::OwoColorize;

use super::global::Global;
use super::helm;
use super::preflight::{self, Check, Level, Report};
use super::status::{self, Component, State};
use super::version::{chart_version_of, parse_release, ReleaseInfo};

const CILIUM_NAMESPACE: &str = "kube-system";
/// A pod Pending longer than this is stuck, not starting.
const PENDING_TOO_LONG_SECS: i64 = 300;
const RESTARTS_WARN: i32 = 5;
/// Ingest lag beyond this means flows on screen are stale.
const INGEST_LAG_WARN_SECS: i64 = 60;

const BAD_WAITING: &[&str] = &[
    "CrashLoopBackOff",
    "ImagePullBackOff",
    "ErrImagePull",
    "CreateContainerConfigError",
    "CreateContainerError",
    "InvalidImageName",
    "RunContainerError",
];

fn waiting_hint(reason: &str) -> &'static str {
    match reason {
        "ImagePullBackOff" | "ErrImagePull" | "InvalidImageName" => {
            "check the image name and tag (a mirror? see `paqtra install --registry`) and that the node can pull it"
        }
        "CreateContainerConfigError" => "a referenced ConfigMap or Secret is missing; run `paqtra sysdump` and inspect the pod events",
        _ => "read the container's previous logs (`kubectl logs --previous`) or run `paqtra sysdump`",
    }
}

/// One pod's problem, if it has one.
pub fn pod_problem(pod: &Pod, now: DateTime<Utc>) -> Option<Check> {
    let name = pod.metadata.name.as_deref().unwrap_or("pod");
    let status = pod.status.as_ref()?;

    let all_containers = status
        .container_statuses
        .iter()
        .flatten()
        .chain(status.init_container_statuses.iter().flatten());
    for cs in all_containers {
        if let Some(reason) = cs
            .state
            .as_ref()
            .and_then(|s| s.waiting.as_ref())
            .and_then(|w| w.reason.as_deref())
            .filter(|r| BAD_WAITING.contains(r))
        {
            return Some(Check::fail(
                &format!("Pod {name}"),
                format!("container {}: {reason}", cs.name),
                waiting_hint(reason),
            ));
        }
        let oom = cs
            .last_state
            .as_ref()
            .and_then(|s| s.terminated.as_ref())
            .and_then(|t| t.reason.as_deref())
            == Some("OOMKilled");
        if oom {
            return Some(Check::warn(
                &format!("Pod {name}"),
                format!("container {} was OOMKilled", cs.name),
                "raise its memory limit (e.g. --set api.resources.limits.memory=3Gi)",
            ));
        }
        if cs.restart_count >= RESTARTS_WARN {
            return Some(Check::warn(
                &format!("Pod {name}"),
                format!("container {} restarted {} times", cs.name, cs.restart_count),
                "read its previous logs or run `paqtra sysdump`",
            ));
        }
    }

    match status.phase.as_deref() {
        Some("Failed") => Some(Check::fail(
            &format!("Pod {name}"),
            status.reason.clone().unwrap_or_else(|| "Failed".into()),
            "run `paqtra sysdump` and inspect the pod events",
        )),
        Some("Pending") => {
            let age = pod
                .metadata
                .creation_timestamp
                .as_ref()
                .map(|t| now.timestamp() - t.0.as_second())
                .unwrap_or(0);
            if age < PENDING_TOO_LONG_SECS {
                return None;
            }
            let why = status
                .conditions
                .iter()
                .flatten()
                .find(|c| c.status == "False")
                .and_then(|c| c.message.clone())
                .unwrap_or_else(|| "not scheduled".into());
            Some(Check::fail(
                &format!("Pod {name}"),
                format!("Pending for {}m: {why}", age / 60),
                "check node capacity, taints and PersistentVolumeClaims (`kubectl describe pod`)",
            ))
        }
        _ => None,
    }
}

/// Workload states -> checks. The API-health row is covered in more detail by
/// [`health_checks`], so it is skipped here.
pub fn component_checks(components: &[Component]) -> Vec<Check> {
    components
        .iter()
        .filter(|c| c.name != "API health")
        .map(|c| match c.state {
            State::Ready => Check::ok(&c.name, c.detail.clone()),
            State::Degraded => Check::warn(&c.name, c.detail.clone(), "some replicas are not ready; see the pod checks below"),
            State::Error => Check::fail(
                &c.name,
                c.detail.clone(),
                match c.name.as_str() {
                    "Cilium" => "Paqtra needs Cilium: `paqtra install --with-cilium` or https://docs.cilium.io",
                    "Hubble Relay" => "enable it: `paqtra hubble enable`",
                    _ => "see the pod checks below, or run `paqtra sysdump`",
                },
            ),
            State::NotDeployed => Check::info(&c.name, "not deployed (disabled in the chart values)"),
            State::Info | State::Warning => Check::info(&c.name, c.detail.clone()),
        })
        .collect()
}

pub fn eval_release(
    release: Option<&ReleaseInfo>,
    name: &str,
    namespace: &str,
    cli: &str,
) -> Check {
    let Some(r) = release else {
        return Check::fail(
            "Release",
            format!("no Helm release {name:?} in namespace {namespace:?}"),
            "install it: `paqtra install` (or pass --namespace/--release if it lives elsewhere)",
        );
    };
    if r.status != "deployed" {
        return Check::fail(
            "Release",
            format!("{} is {}", r.chart, r.status),
            "see `helm history` for the failure; `paqtra upgrade` or `paqtra uninstall` then install again",
        );
    }
    let chart = chart_version_of(&r.chart);
    if chart != cli {
        return Check::warn(
            "Release",
            format!("{} (this CLI is v{cli})", r.chart),
            "align them: `paqtra upgrade`",
        );
    }
    Check::ok("Release", format!("{} deployed", r.chart))
}

/// The agent should run on every node it can; fewer usually means a taint or
/// node selector, and flows from the missing nodes are silently absent.
pub fn agent_coverage(agents: Option<(i32, i32)>, nodes: usize) -> Option<Check> {
    let (_, desired) = agents?;
    if nodes == 0 {
        return None;
    }
    Some(if (desired as usize) < nodes {
        Check::warn(
            "Agent coverage",
            format!("scheduled on {desired} of {nodes} nodes"),
            "check node taints against agent.tolerations / agent.nodeSelector in the chart values",
        )
    } else {
        Check::ok("Agent coverage", format!("{desired} of {nodes} nodes"))
    })
}

/// `/health` -> one check per subsystem, with the numbers that matter for ingest.
pub fn health_checks(body: &str) -> Vec<Check> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else {
        return vec![Check::warn(
            "API health",
            "unexpected /health response",
            "the API may be starting; retry in a moment",
        )];
    };
    let Some(subs) = v.get("subsystems").and_then(|s| s.as_object()) else {
        return vec![Check::warn(
            "API health",
            "/health lists no subsystems",
            "an older API version? see `paqtra version`",
        )];
    };
    let mut out = Vec::new();
    for (name, val) in subs {
        let label = format!("API: {name}");
        let status = val
            .as_str()
            .or_else(|| val.get("status").and_then(|s| s.as_str()))
            .unwrap_or("unknown");
        if status != "ok" {
            out.push(Check::fail(
                &label,
                status.to_string(),
                match name.as_str() {
                    "hubble_relay" => "the API cannot reach Hubble Relay: check `paqtra status` and the HUBBLE_ADDRESS value (`paqtra config get api.env.hubbleAddress`)",
                    "kubernetes" => "the API cannot talk to Kubernetes: check its ServiceAccount RBAC",
                    "flow_ingest" => "flow ingest is not running: check the API logs (`paqtra sysdump`)",
                    _ => "check the API logs (`paqtra sysdump`)",
                },
            ));
            continue;
        }
        if name == "flow_ingest" {
            let lag = val.get("lag_secs").and_then(|x| x.as_i64()).unwrap_or(0);
            let gaps = val.get("gaps").and_then(|x| x.as_i64()).unwrap_or(0);
            let eps = val
                .get("events_per_sec")
                .and_then(|x| x.as_f64())
                .unwrap_or(0.0);
            let detail = format!("lag {lag}s, {eps:.1} flows/s, {gaps} gaps");
            if lag > INGEST_LAG_WARN_SECS {
                out.push(Check::warn(&label, detail, "flows on screen are stale; the API may be overloaded (see api.resources) or Hubble is slow"));
            } else if gaps > 0 {
                out.push(Check::warn(&label, detail, "Hubble dropped events (its ring buffer is full); see the Hubble page for buffer fill"));
            } else {
                out.push(Check::ok(&label, detail));
            }
            continue;
        }
        out.push(Check::ok(&label, "ok"));
    }
    out
}

fn prometheus_check(configured: Option<&str>) -> Check {
    match configured {
        Some(url) if !url.trim().is_empty() => Check::ok("Prometheus", "configured"),
        _ => Check::info(
            "Prometheus",
            "not configured: the Hubble and Cilium metrics pages will be empty",
        ),
    }
}

/// Run every check. Also used by `sysdump`, which embeds the report.
pub async fn collect(g: &Global) -> Report {
    let mut report = preflight::run(g, CILIUM_NAMESPACE, false, false).await;
    if report
        .checks
        .iter()
        .any(|c| c.name == "Cluster access" && c.level == Level::Fail)
    {
        return report;
    }
    let Ok(client) = super::kube::client(g).await else {
        return report;
    };

    match helm::find(g).await {
        Some(h) => {
            let args: Vec<String> = [
                "list",
                "--namespace",
                &g.namespace,
                "--filter",
                &format!("^{}$", g.release),
                "--output",
                "json",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect();
            let release = h
                .run_ok(&args, "helm list")
                .await
                .ok()
                .and_then(|o| parse_release(&o, &g.release));
            report.push(eval_release(
                release.as_ref(),
                &g.release,
                &g.namespace,
                super::chart::CLI_VERSION,
            ));
        }
        None => report.push(Check::info(
            "Release",
            "helm not found: release check skipped",
        )),
    }

    let components = status::gather(&client, g).await;
    report.extend(component_checks(&components));

    let pods: Api<Pod> = Api::namespaced(client.clone(), &g.namespace);
    let selector = format!("app.kubernetes.io/instance={}", g.release);
    let now = Utc::now();
    if let Ok(list) = pods.list(&ListParams::default().labels(&selector)).await {
        report.extend(list.items.iter().filter_map(|p| pod_problem(p, now)));
    }

    let node_count = Api::<Node>::all(client.clone())
        .list(&ListParams::default())
        .await
        .map(|l| l.items.len())
        .unwrap_or(0);
    let agent_counts = components.iter().find(|c| c.name == "Agent").and_then(|c| {
        let (ready, desired) = c.detail.split_whitespace().next()?.split_once('/')?;
        Some((ready.parse().ok()?, desired.parse().ok()?))
    });
    report.extend(agent_coverage(agent_counts, node_count));

    if let Some(res) = status::api_health_raw(&client, g).await {
        match res {
            Ok(body) => report.extend(health_checks(&body)),
            Err(e) => report.push(Check::warn(
                "API health",
                format!("/health not reachable: {e}"),
                "the API pod may be down; see the checks above",
            )),
        }
    }

    let cms: Api<ConfigMap> = Api::namespaced(client, &g.namespace);
    let prom = cms
        .get_opt(&format!("{}-config", g.release))
        .await
        .ok()
        .flatten()
        .and_then(|c| c.data)
        .and_then(|d| d.get("PROMETHEUS_URL").cloned());
    report.push(prometheus_check(prom.as_deref()));
    report
}

/// Returns true when there are no failures (warnings are fine).
pub async fn cmd_doctor(g: &Global, output: &str) -> Result<bool> {
    let report = collect(g).await;
    let ok = !report.has_failures();
    if output == "json" {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(ok);
    }
    println!("{}", "Paqtra doctor".bold());
    println!();
    print!("{}", report.render());
    println!();
    let fails = report
        .checks
        .iter()
        .filter(|c| c.level == Level::Fail)
        .count();
    let warns = report
        .checks
        .iter()
        .filter(|c| c.level == Level::Warn)
        .count();
    if fails > 0 {
        println!(
            "{} {fails} problem(s) need attention{}",
            "✖".red(),
            if warns > 0 {
                format!(", {warns} warning(s)")
            } else {
                String::new()
            }
        );
    } else if warns > 0 {
        println!("{} No problems, {warns} warning(s)", "⚠".yellow());
    } else {
        println!("{} No problems found", "✔".green());
    }
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::core::v1::{
        ContainerState, ContainerStateTerminated, ContainerStateWaiting, ContainerStatus,
        PodCondition, PodStatus,
    };
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, Time};

    fn pod(
        name: &str,
        phase: &str,
        age_secs: i64,
        containers: Vec<ContainerStatus>,
        conditions: Vec<PodCondition>,
    ) -> Pod {
        Pod {
            metadata: ObjectMeta {
                name: Some(name.into()),
                creation_timestamp: Some(Time(
                    k8s_openapi::jiff::Timestamp::from_second(Utc::now().timestamp() - age_secs)
                        .unwrap(),
                )),
                ..Default::default()
            },
            status: Some(PodStatus {
                phase: Some(phase.into()),
                container_statuses: Some(containers),
                conditions: Some(conditions),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    fn waiting(reason: &str) -> ContainerStatus {
        ContainerStatus {
            name: "api".into(),
            state: Some(ContainerState {
                waiting: Some(ContainerStateWaiting {
                    reason: Some(reason.into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    fn running(restarts: i32, last_terminated: Option<&str>) -> ContainerStatus {
        ContainerStatus {
            name: "api".into(),
            restart_count: restarts,
            last_state: last_terminated.map(|r| ContainerState {
                terminated: Some(ContainerStateTerminated {
                    reason: Some(r.into()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_healthy_pod_has_no_problem() {
        assert!(pod_problem(
            &pod("p", "Running", 3600, vec![running(0, None)], vec![]),
            Utc::now()
        )
        .is_none());
    }

    #[test]
    fn crash_loops_and_image_pull_failures_are_failures_with_the_right_hint() {
        let c = pod_problem(
            &pod(
                "p",
                "Running",
                60,
                vec![waiting("CrashLoopBackOff")],
                vec![],
            ),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(c.level, Level::Fail);
        assert!(c.detail.contains("CrashLoopBackOff") && c.hint.unwrap().contains("logs"));
        let c = pod_problem(
            &pod(
                "p",
                "Pending",
                60,
                vec![waiting("ImagePullBackOff")],
                vec![],
            ),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(c.level, Level::Fail);
        assert!(c.hint.unwrap().contains("--registry"));
        // ContainerCreating is normal, not a problem.
        assert!(pod_problem(
            &pod(
                "p",
                "Pending",
                60,
                vec![waiting("ContainerCreating")],
                vec![]
            ),
            Utc::now()
        )
        .is_none());
    }

    #[test]
    fn oom_kills_and_many_restarts_are_warnings() {
        let c = pod_problem(
            &pod(
                "p",
                "Running",
                600,
                vec![running(1, Some("OOMKilled"))],
                vec![],
            ),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(c.level, Level::Warn);
        assert!(c.hint.unwrap().contains("memory"));
        let c = pod_problem(
            &pod("p", "Running", 600, vec![running(7, None)], vec![]),
            Utc::now(),
        )
        .unwrap();
        assert_eq!(c.level, Level::Warn);
        assert!(pod_problem(
            &pod("p", "Running", 600, vec![running(4, None)], vec![]),
            Utc::now()
        )
        .is_none());
    }

    #[test]
    fn pending_is_a_problem_only_after_a_while_and_says_why() {
        let cond = PodCondition {
            type_: "PodScheduled".into(),
            status: "False".into(),
            message: Some("0/3 nodes are available: insufficient memory".into()),
            ..Default::default()
        };
        assert!(pod_problem(
            &pod("p", "Pending", 60, vec![], vec![cond.clone()]),
            Utc::now()
        )
        .is_none());
        let c = pod_problem(&pod("p", "Pending", 900, vec![], vec![cond]), Utc::now()).unwrap();
        assert_eq!(c.level, Level::Fail);
        assert!(
            c.detail.contains("15m") && c.detail.contains("insufficient memory"),
            "{}",
            c.detail
        );
        assert_eq!(
            pod_problem(&pod("p", "Failed", 10, vec![], vec![]), Utc::now())
                .unwrap()
                .level,
            Level::Fail
        );
    }

    fn comp(name: &str, state: State, detail: &str) -> Component {
        Component {
            name: name.into(),
            state,
            detail: detail.into(),
        }
    }

    #[test]
    fn components_map_to_checks() {
        let checks = component_checks(&[
            comp("API", State::Ready, "1/1 pods"),
            comp("Agent", State::Degraded, "2/3 nodes"),
            comp("UI", State::NotDeployed, "not deployed"),
            comp("Cilium", State::Error, "not found in kube-system"),
            comp("API health", State::Warning, "x"),
        ]);
        assert_eq!(
            checks.len(),
            4,
            "API health is reported by health_checks instead"
        );
        assert_eq!(checks[0].level, Level::Ok);
        assert_eq!(checks[1].level, Level::Warn);
        assert_eq!(checks[2].level, Level::Info);
        assert_eq!(checks[3].level, Level::Fail);
        assert!(checks[3].hint.as_ref().unwrap().contains("--with-cilium"));
    }

    fn rel(chart: &str, status: &str) -> ReleaseInfo {
        ReleaseInfo {
            name: "paqtra".into(),
            namespace: "paqtra".into(),
            chart: chart.into(),
            app_version: "x".into(),
            status: status.into(),
        }
    }

    #[test]
    fn release_verdicts() {
        assert_eq!(
            eval_release(None, "paqtra", "paqtra", "2.1.0").level,
            Level::Fail
        );
        assert_eq!(
            eval_release(Some(&rel("paqtra-2.1.0", "failed")), "p", "n", "2.1.0").level,
            Level::Fail
        );
        let skew = eval_release(Some(&rel("paqtra-2.0.0", "deployed")), "p", "n", "2.1.0");
        assert_eq!(skew.level, Level::Warn);
        assert!(skew.hint.unwrap().contains("paqtra upgrade"));
        assert_eq!(
            eval_release(Some(&rel("paqtra-2.1.0", "deployed")), "p", "n", "2.1.0").level,
            Level::Ok
        );
    }

    #[test]
    fn agent_coverage_compares_with_the_node_count() {
        assert_eq!(agent_coverage(Some((3, 3)), 3).unwrap().level, Level::Ok);
        assert_eq!(agent_coverage(Some((2, 2)), 3).unwrap().level, Level::Warn);
        assert!(
            agent_coverage(None, 3).is_none(),
            "no agent DaemonSet: nothing to compare"
        );
        assert!(agent_coverage(Some((1, 1)), 0).is_none());
    }

    #[test]
    fn health_subsystems_become_checks() {
        let ok = health_checks(
            r#"{"status":"healthy","subsystems":{"cache":"ok","hubble_relay":"ok","kubernetes":"ok","flow_ingest":{"status":"ok","lag_secs":8,"gaps":0,"events_per_sec":3.85}}}"#,
        );
        assert!(ok.iter().all(|c| c.level == Level::Ok));
        let ingest = ok.iter().find(|c| c.name == "API: flow_ingest").unwrap();
        assert!(
            ingest.detail.contains("lag 8s") && ingest.detail.contains("3.9 flows/s"),
            "{}",
            ingest.detail
        );

        let bad = health_checks(
            r#"{"subsystems":{"hubble_relay":"error","flow_ingest":{"status":"ok","lag_secs":300,"gaps":0}}}"#,
        );
        let relay = bad.iter().find(|c| c.name == "API: hubble_relay").unwrap();
        assert_eq!(relay.level, Level::Fail);
        assert!(
            relay.hint.as_ref().unwrap().contains("HUBBLE_ADDRESS")
                || relay.hint.as_ref().unwrap().contains("hubbleAddress")
        );
        assert_eq!(
            bad.iter()
                .find(|c| c.name == "API: flow_ingest")
                .unwrap()
                .level,
            Level::Warn,
            "300s lag is stale"
        );

        let gaps = health_checks(
            r#"{"subsystems":{"flow_ingest":{"status":"ok","lag_secs":2,"gaps":4}}}"#,
        );
        assert_eq!(gaps[0].level, Level::Warn);
        assert_eq!(health_checks("<html>")[0].level, Level::Warn);
        assert_eq!(health_checks("{}")[0].level, Level::Warn);
    }

    #[test]
    fn prometheus_is_informational() {
        assert_eq!(prometheus_check(Some("http://prom:80")).level, Level::Ok);
        assert_eq!(prometheus_check(None).level, Level::Info);
        assert_eq!(prometheus_check(Some(" ")).level, Level::Info);
    }
}
