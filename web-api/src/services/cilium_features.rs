// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

// Which Cilium features are switched on in this cluster.
//
// Cilium's runtime configuration lives in the `cilium-config` ConfigMap. This
// reads it (read-only) and reports a fixed set of features, each with the
// Paqtra view that can show it. A key that is missing is reported as
// `unknown`, not `disabled`: older Cilium versions simply do not have it.

use std::collections::BTreeMap;

use serde::Serialize;

/// What the ConfigMap says about one feature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureState {
    Enabled,
    Disabled,
    /// A setting with a value (a mode, a size) rather than an on/off switch.
    Set,
    /// No such key: an older Cilium, or a feature that was never configured.
    Unknown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Feature {
    pub key: &'static str,
    pub title: &'static str,
    pub category: &'static str,
    pub state: FeatureState,
    /// The raw ConfigMap value, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The `cilium-config` key that answered (features can have renamed keys).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_key: Option<&'static str>,
    /// Paqtra page that shows this feature, if there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view: Option<&'static str>,
}

struct Def {
    key: &'static str,
    title: &'static str,
    category: &'static str,
    /// Keys to try in order (newer name first).
    config_keys: &'static [&'static str],
    /// On/off switch (`true`) or a setting with a value (`false`).
    switch: bool,
    view: Option<&'static str>,
}

const DEFS: &[Def] = &[
    Def {
        key: "hubble",
        title: "Hubble observability",
        category: "Observability",
        config_keys: &["enable-hubble"],
        switch: true,
        view: Some("/flows"),
    },
    Def {
        key: "hubble-metrics",
        title: "Hubble metrics",
        category: "Observability",
        config_keys: &["hubble-metrics"],
        switch: false,
        view: Some("/cilium-insights"),
    },
    Def {
        key: "l7-proxy",
        title: "L7 proxy (HTTP/DNS visibility and policy)",
        category: "Observability",
        config_keys: &["enable-l7-proxy"],
        switch: true,
        view: Some("/dns"),
    },
    Def {
        key: "monitor-aggregation",
        title: "Monitor aggregation level",
        category: "Observability",
        config_keys: &["monitor-aggregation"],
        switch: false,
        view: None,
    },
    Def {
        key: "policy-enforcement",
        title: "Policy enforcement mode",
        category: "Policy",
        config_keys: &["enable-policy"],
        switch: false,
        view: Some("/policy-rules"),
    },
    Def {
        key: "policy-audit-mode",
        title: "Policy audit mode (verdicts logged, not enforced)",
        category: "Policy",
        config_keys: &["policy-audit-mode"],
        switch: true,
        view: Some("/flows"),
    },
    Def {
        key: "host-firewall",
        title: "Host firewall",
        category: "Policy",
        config_keys: &["enable-host-firewall"],
        switch: true,
        view: None,
    },
    Def {
        key: "mutual-auth",
        title: "Mutual authentication (SPIFFE)",
        category: "Policy",
        config_keys: &["mesh-auth-enabled"],
        switch: true,
        view: None,
    },
    Def {
        key: "wireguard",
        title: "WireGuard encryption",
        category: "Encryption",
        config_keys: &["enable-wireguard"],
        switch: true,
        view: Some("/wireguard"),
    },
    Def {
        key: "ipsec",
        title: "IPsec encryption",
        category: "Encryption",
        config_keys: &["enable-ipsec"],
        switch: true,
        view: Some("/encryption"),
    },
    Def {
        key: "kube-proxy-replacement",
        title: "Kube-proxy replacement",
        category: "Datapath",
        config_keys: &["kube-proxy-replacement"],
        switch: false,
        view: None,
    },
    Def {
        key: "routing-mode",
        title: "Routing mode",
        category: "Datapath",
        config_keys: &["routing-mode", "tunnel"],
        switch: false,
        view: None,
    },
    Def {
        key: "tunnel-protocol",
        title: "Tunnel protocol",
        category: "Datapath",
        config_keys: &["tunnel-protocol"],
        switch: false,
        view: None,
    },
    Def {
        key: "bandwidth-manager",
        title: "Bandwidth manager",
        category: "Datapath",
        config_keys: &["enable-bandwidth-manager"],
        switch: true,
        view: Some("/bandwidth"),
    },
    Def {
        key: "bbr",
        title: "BBR congestion control for pods",
        category: "Datapath",
        config_keys: &["enable-bbr"],
        switch: true,
        view: None,
    },
    Def {
        key: "xdp-prefilter",
        title: "XDP prefilter",
        category: "Datapath",
        config_keys: &["enable-xdp-prefilter"],
        switch: true,
        view: None,
    },
    Def {
        key: "lb-algorithm",
        title: "Load-balancing algorithm (random/maglev)",
        category: "Datapath",
        config_keys: &["bpf-lb-algorithm"],
        switch: false,
        view: Some("/lb-map"),
    },
    Def {
        key: "ipam",
        title: "IPAM mode",
        category: "Networking",
        config_keys: &["ipam"],
        switch: false,
        view: Some("/ipam"),
    },
    Def {
        key: "ipv4",
        title: "IPv4",
        category: "Networking",
        config_keys: &["enable-ipv4"],
        switch: true,
        view: None,
    },
    Def {
        key: "ipv6",
        title: "IPv6",
        category: "Networking",
        config_keys: &["enable-ipv6"],
        switch: true,
        view: None,
    },
    Def {
        key: "egress-gateway",
        title: "Egress gateway",
        category: "Networking",
        config_keys: &["enable-egress-gateway", "enable-ipv4-egress-gateway"],
        switch: true,
        view: Some("/egress"),
    },
    Def {
        key: "bgp",
        title: "BGP control plane",
        category: "Networking",
        config_keys: &["enable-bgp-control-plane"],
        switch: true,
        view: Some("/bgp"),
    },
    Def {
        key: "l2-announcements",
        title: "L2 announcements",
        category: "Networking",
        config_keys: &["enable-l2-announcements"],
        switch: true,
        view: None,
    },
    Def {
        key: "local-redirect-policy",
        title: "Local redirect policy",
        category: "Networking",
        config_keys: &["enable-local-redirect-policy"],
        switch: true,
        view: None,
    },
    Def {
        key: "gateway-api",
        title: "Gateway API",
        category: "Service mesh",
        config_keys: &["enable-gateway-api"],
        switch: true,
        view: None,
    },
    Def {
        key: "ingress-controller",
        title: "Ingress controller",
        category: "Service mesh",
        config_keys: &["enable-ingress-controller"],
        switch: true,
        view: None,
    },
    Def {
        key: "envoy-config",
        title: "CiliumEnvoyConfig",
        category: "Service mesh",
        config_keys: &["enable-envoy-config"],
        switch: true,
        view: None,
    },
    Def {
        key: "clustermesh",
        title: "Cluster name (ClusterMesh identity)",
        category: "Multi-cluster",
        config_keys: &["cluster-name"],
        switch: false,
        view: Some("/clustermesh"),
    },
    Def {
        key: "policy-map-max",
        title: "Policy map capacity (entries)",
        category: "Capacity",
        config_keys: &["bpf-policy-map-max"],
        switch: false,
        view: Some("/policy-map"),
    },
    Def {
        key: "ct-tcp-max",
        title: "Conntrack TCP capacity (entries)",
        category: "Capacity",
        config_keys: &["bpf-ct-global-tcp-max"],
        switch: false,
        view: Some("/conntrack"),
    },
    Def {
        key: "lb-map-max",
        title: "Load-balancer map capacity (entries)",
        category: "Capacity",
        config_keys: &["bpf-lb-map-max"],
        switch: false,
        view: Some("/lb-map"),
    },
];

fn parse_switch(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "enabled" => Some(true),
        "false" | "0" | "no" | "disabled" => Some(false),
        _ => None,
    }
}

/// Report every known feature from the ConfigMap's `data`.
pub fn features_from_config(data: &BTreeMap<String, String>) -> Vec<Feature> {
    DEFS.iter()
        .map(|d| {
            let found = d
                .config_keys
                .iter()
                .find_map(|k| data.get(*k).map(|v| (*k, v.clone())));
            let (state, value, config_key) = match found {
                None => (FeatureState::Unknown, None, None),
                Some((k, v)) if d.switch => match parse_switch(&v) {
                    Some(true) => (FeatureState::Enabled, None, Some(k)),
                    Some(false) => (FeatureState::Disabled, None, Some(k)),
                    // A switch key with a non-boolean value: show it, do not guess.
                    None => (FeatureState::Set, Some(v), Some(k)),
                },
                // An empty setting (e.g. no hubble metrics listed) is "not set".
                Some((k, v)) if v.trim().is_empty() => (FeatureState::Disabled, None, Some(k)),
                Some((k, v)) => (FeatureState::Set, Some(v), Some(k)),
            };
            Feature {
                key: d.key,
                title: d.title,
                category: d.category,
                state,
                value,
                config_key,
                view: d.view,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }
    fn get<'a>(fs: &'a [Feature], key: &str) -> &'a Feature {
        fs.iter().find(|f| f.key == key).unwrap()
    }

    #[test]
    fn switches_report_enabled_and_disabled() {
        let fs = features_from_config(&cfg(&[
            ("enable-hubble", "true"),
            ("enable-wireguard", "false"),
            ("enable-bgp-control-plane", "TRUE"),
        ]));
        assert_eq!(get(&fs, "hubble").state, FeatureState::Enabled);
        assert_eq!(get(&fs, "wireguard").state, FeatureState::Disabled);
        assert_eq!(get(&fs, "bgp").state, FeatureState::Enabled);
    }

    #[test]
    fn a_missing_key_is_unknown_not_disabled() {
        let fs = features_from_config(&cfg(&[]));
        assert!(fs.iter().all(|f| f.state == FeatureState::Unknown));
        assert!(fs
            .iter()
            .all(|f| f.value.is_none() && f.config_key.is_none()));
    }

    #[test]
    fn settings_carry_their_value() {
        let fs = features_from_config(&cfg(&[
            ("enable-policy", "default"),
            ("bpf-policy-map-max", "16384"),
            ("hubble-metrics", "dns drop tcp"),
        ]));
        let p = get(&fs, "policy-enforcement");
        assert_eq!(
            (p.state, p.value.as_deref()),
            (FeatureState::Set, Some("default"))
        );
        assert_eq!(get(&fs, "policy-map-max").value.as_deref(), Some("16384"));
        assert_eq!(
            get(&fs, "hubble-metrics").value.as_deref(),
            Some("dns drop tcp")
        );
    }

    #[test]
    fn an_empty_setting_is_disabled() {
        let fs = features_from_config(&cfg(&[("hubble-metrics", "")]));
        assert_eq!(get(&fs, "hubble-metrics").state, FeatureState::Disabled);
    }

    #[test]
    fn renamed_keys_fall_back_to_the_older_name() {
        let fs = features_from_config(&cfg(&[("enable-ipv4-egress-gateway", "true")]));
        let f = get(&fs, "egress-gateway");
        assert_eq!(f.state, FeatureState::Enabled);
        assert_eq!(f.config_key, Some("enable-ipv4-egress-gateway"));
        // Newer name wins when both exist.
        let fs = features_from_config(&cfg(&[
            ("enable-egress-gateway", "false"),
            ("enable-ipv4-egress-gateway", "true"),
        ]));
        assert_eq!(get(&fs, "egress-gateway").state, FeatureState::Disabled);
    }

    #[test]
    fn a_non_boolean_switch_value_is_shown_not_guessed() {
        let fs = features_from_config(&cfg(&[("enable-hubble", "maybe")]));
        let f = get(&fs, "hubble");
        assert_eq!(
            (f.state, f.value.as_deref()),
            (FeatureState::Set, Some("maybe"))
        );
    }

    #[test]
    fn keys_are_unique() {
        let mut keys: Vec<_> = DEFS.iter().map(|d| d.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), DEFS.len());
    }
}
