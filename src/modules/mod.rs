/// Escape a string for safe interpolation into YAML values.
/// Wraps the value in double quotes and escapes embedded backslashes, quotes,
/// newlines, carriage returns, and tabs.
pub fn yaml_escape(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

/// Sanitize a string for use as a Kubernetes resource name.
/// Lowercases, replaces non-alphanumeric characters (except hyphens and dots)
/// with hyphens, and trims leading/trailing hyphens and dots (so ".." cannot
/// survive as a name).
pub fn sanitize_k8s_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(|c| c == '-' || c == '.')
        .to_string()
}

pub mod autopolicy;
pub mod canary;
pub mod chaos;
/// Intelligent modules for paqtra
///
/// Each module provides a specific capability:
/// - healer: Automatic problem detection and fixing
/// - autopolicy: Zero-trust policy learning and generation
/// - rootcause: Drop analysis and explanation
/// - replay: Traffic recording and replay
/// - simulator: What-if policy simulation
/// - packet_explainer: Interactive packet analysis and explanation
/// - chaos: Chaos engineering and fault injection
/// - canary: Progressive traffic shifting and canary deployments
/// - multicluster: Multi-cluster orchestration
/// - anomaly_detection: AI/ML-powered anomaly detection (experimental)
/// - security_compliance: Zero-trust and compliance frameworks (experimental)
/// - dev_tools: Developer experience tools (experimental)
pub mod healer;
pub mod multicluster;
pub mod packet_explainer;
pub mod replay;
pub mod rootcause;
pub mod simulator;

// Experimental cutting-edge features
pub mod anomaly_detection;
pub mod dev_tools;
pub mod security_compliance;

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_k8s_name;

    #[test]
    fn strips_dot_only_and_edge_dots() {
        assert_eq!(sanitize_k8s_name(".."), "");
        assert_eq!(sanitize_k8s_name("."), "");
        assert_eq!(sanitize_k8s_name(".Foo_Bar."), "foo-bar");
        assert_eq!(sanitize_k8s_name("a.b-c"), "a.b-c");
    }
}
