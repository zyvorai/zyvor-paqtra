// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// Integration tests for eBPF module types and MockMapReader.
///
/// Validates that:
/// - MockMapReader returns expected data structures
/// - DropReasonType covers all documented Cilium codes
/// - DropReasonType::description() returns non-empty strings for all variants
/// - PolicyVerdict serialization/deserialization round-trips correctly
/// - ConntrackState equality works as expected
/// - All map types can be read from the mock reader
use paqtra::ebpf::{
    ConntrackState, DropReasonType, EbpfMetrics, MapReader, MockMapReader, PolicyVerdict,
};

// ---------------------------------------------------------------------------
// Tests: MockMapReader returns expected data structures
// ---------------------------------------------------------------------------

#[test]
fn test_mock_reader_policy_map_returns_one_entry() {
    let reader = MockMapReader;
    let decisions = reader.read_policy_map().unwrap();
    assert_eq!(
        decisions.len(),
        1,
        "MockMapReader should return exactly 1 policy decision"
    );
}

#[test]
fn test_mock_reader_policy_map_entry_has_correct_fields() {
    let reader = MockMapReader;
    let decisions = reader.read_policy_map().unwrap();
    let d = &decisions[0];
    assert_eq!(d.src_identity, 100);
    assert_eq!(d.dst_identity, 200);
    assert_eq!(d.port, 80);
    assert_eq!(d.protocol, 6, "Protocol 6 is TCP");
    assert_eq!(d.verdict, PolicyVerdict::Allow);
}

#[test]
fn test_mock_reader_conntrack_map_has_data() {
    let reader = MockMapReader;
    let entries = reader.read_conntrack_map().unwrap();
    assert!(
        !entries.is_empty(),
        "MockMapReader conntrack map should have realistic test data"
    );
}

#[test]
fn test_mock_reader_lb_map_has_data() {
    let reader = MockMapReader;
    let entries = reader.read_lb_map().unwrap();
    assert!(
        !entries.is_empty(),
        "MockMapReader LB map should have realistic test data"
    );
}

#[test]
fn test_mock_reader_ipcache_map_has_data() {
    let reader = MockMapReader;
    let entries = reader.read_ipcache_map().unwrap();
    assert!(
        !entries.is_empty(),
        "MockMapReader IP cache map should have realistic test data"
    );
}

#[test]
fn test_mock_reader_drop_map_has_data() {
    let reader = MockMapReader;
    let entries = reader.read_drop_map().unwrap();
    assert!(
        !entries.is_empty(),
        "MockMapReader drop map should have realistic test data"
    );
}

// ---------------------------------------------------------------------------
// Tests: DropReasonType covers all documented Cilium codes
// ---------------------------------------------------------------------------

/// All documented Cilium drop reason codes that are explicitly mapped
/// in DropReasonType::from_code.
const DOCUMENTED_CODES: &[(u32, &str)] = &[
    // Code 0 is now Other(0) — not a documented drop reason
    (1, "PolicyDenied"),
    (2, "InvalidPacket"),
    (3, "NoRoute"),
    (4, "UnknownL4Protocol"),
    (5, "FragmentationNeeded"),
    (6, "CTMapFull"),
    (7, "NATMapFull"),
    (130, "InvalidSourceIP"),
    (131, "InvalidDestIP"),
    (132, "UnsupportedL3Protocol"),
    (133, "MissedTailCall"),
    (134, "ErrorWritingToPacket"),
    (135, "UnknownL4ICMPType"),
    (136, "UnknownICMPv6Type"),
    (137, "UnknownICMPv6Code"),
    (140, "ServiceBackendNotFound"),
    (141, "NoTunnelEndpoint"),
    (148, "HostUnreachable"),
    (152, "StaleOrUnroutable"),
    (153, "ConnectionTrackingInvalid"),
    (181, "AuthRequired"),
    (184, "NATNotNeeded"),
    (185, "IsClusterIP"),
];

#[test]
fn test_documented_codes_do_not_fall_through_to_other() {
    for (code, expected_name) in DOCUMENTED_CODES {
        let reason = DropReasonType::from_code(*code);
        // Ensure documented codes never produce DropReasonType::Other
        assert!(
            !matches!(reason, DropReasonType::Other(_)),
            "Code {} ({}) should not produce DropReasonType::Other, got {:?}",
            code,
            expected_name,
            reason
        );
    }
}

#[test]
fn test_undocumented_codes_produce_other() {
    let undocumented = [8, 9, 50, 99, 200, 255, 1000, u32::MAX];
    for code in undocumented {
        let reason = DropReasonType::from_code(code);
        assert!(
            matches!(reason, DropReasonType::Other(c) if c == code),
            "Undocumented code {} should produce Other({}), got {:?}",
            code,
            code,
            reason
        );
    }
}

#[test]
fn test_specific_code_mappings() {
    assert_eq!(DropReasonType::from_code(1), DropReasonType::PolicyDenied);
    assert_eq!(DropReasonType::from_code(2), DropReasonType::InvalidPacket);
    assert_eq!(DropReasonType::from_code(3), DropReasonType::NoRoute);
    assert_eq!(
        DropReasonType::from_code(4),
        DropReasonType::UnknownL4Protocol
    );
    assert_eq!(
        DropReasonType::from_code(5),
        DropReasonType::FragmentationNeeded
    );
    assert_eq!(DropReasonType::from_code(6), DropReasonType::CTMapFull);
    assert_eq!(DropReasonType::from_code(7), DropReasonType::NATMapFull);
    assert_eq!(
        DropReasonType::from_code(130),
        DropReasonType::InvalidSourceIP
    );
    assert_eq!(
        DropReasonType::from_code(131),
        DropReasonType::InvalidDestIP
    );
    assert_eq!(
        DropReasonType::from_code(132),
        DropReasonType::UnsupportedL3Protocol
    );
    assert_eq!(
        DropReasonType::from_code(133),
        DropReasonType::MissedTailCall
    );
    assert_eq!(
        DropReasonType::from_code(134),
        DropReasonType::ErrorWritingToPacket
    );
    assert_eq!(
        DropReasonType::from_code(135),
        DropReasonType::UnknownL4ICMPType
    );
    assert_eq!(
        DropReasonType::from_code(136),
        DropReasonType::UnknownICMPv6Type
    );
    assert_eq!(
        DropReasonType::from_code(137),
        DropReasonType::UnknownICMPv6Code
    );
    assert_eq!(
        DropReasonType::from_code(140),
        DropReasonType::ServiceBackendNotFound
    );
    assert_eq!(
        DropReasonType::from_code(141),
        DropReasonType::NoTunnelEndpoint
    );
    assert_eq!(
        DropReasonType::from_code(148),
        DropReasonType::HostUnreachable
    );
    assert_eq!(
        DropReasonType::from_code(152),
        DropReasonType::StaleOrUnroutable
    );
    assert_eq!(
        DropReasonType::from_code(153),
        DropReasonType::ConnectionTrackingInvalid
    );
    assert_eq!(DropReasonType::from_code(181), DropReasonType::AuthRequired);
    assert_eq!(DropReasonType::from_code(184), DropReasonType::NATNotNeeded);
    assert_eq!(DropReasonType::from_code(185), DropReasonType::IsClusterIP);
}

// ---------------------------------------------------------------------------
// Tests: DropReasonType::description() returns non-empty strings
// ---------------------------------------------------------------------------

#[test]
fn test_all_drop_reason_descriptions_are_nonempty() {
    let all_variants: Vec<DropReasonType> = vec![
        DropReasonType::PolicyDenied,
        DropReasonType::InvalidPacket,
        DropReasonType::NoRoute,
        DropReasonType::UnknownL4Protocol,
        DropReasonType::FragmentationNeeded,
        DropReasonType::CTMapFull,
        DropReasonType::NATMapFull,
        DropReasonType::InvalidSourceIP,
        DropReasonType::InvalidDestIP,
        DropReasonType::UnsupportedL3Protocol,
        DropReasonType::MissedTailCall,
        DropReasonType::ErrorWritingToPacket,
        DropReasonType::UnknownL4ICMPType,
        DropReasonType::UnknownICMPv6Type,
        DropReasonType::UnknownICMPv6Code,
        DropReasonType::ServiceBackendNotFound,
        DropReasonType::NoTunnelEndpoint,
        DropReasonType::HostUnreachable,
        DropReasonType::StaleOrUnroutable,
        DropReasonType::ConnectionTrackingInvalid,
        DropReasonType::AuthRequired,
        DropReasonType::NATNotNeeded,
        DropReasonType::IsClusterIP,
        DropReasonType::Other(99),
        DropReasonType::Other(0),
        DropReasonType::Other(u32::MAX),
    ];

    for variant in all_variants {
        let desc = variant.description();
        assert!(
            !desc.is_empty(),
            "Description for {:?} must not be empty",
            variant
        );
    }
}

#[test]
fn test_known_descriptions_match_expected_text() {
    assert_eq!(DropReasonType::PolicyDenied.description(), "Policy denied");
    assert_eq!(DropReasonType::NoRoute.description(), "No route");
    assert_eq!(
        DropReasonType::AuthRequired.description(),
        "Authentication required"
    );
    assert_eq!(
        DropReasonType::CTMapFull.description(),
        "Connection tracking map full"
    );
    assert_eq!(
        DropReasonType::FragmentationNeeded.description(),
        "Fragmentation needed"
    );
    assert_eq!(DropReasonType::IsClusterIP.description(), "Is ClusterIP");
}

#[test]
fn test_other_variant_description_is_meaningful() {
    let desc = DropReasonType::Other(42).description();
    assert_eq!(desc, "Unknown drop reason");
}

// ---------------------------------------------------------------------------
// Tests: PolicyVerdict serialization/deserialization round-trip
// ---------------------------------------------------------------------------

#[test]
fn test_policy_verdict_serde_roundtrip() {
    let verdicts = [
        PolicyVerdict::Allow,
        PolicyVerdict::Deny,
        PolicyVerdict::Redirect,
        PolicyVerdict::Audit,
    ];

    for verdict in verdicts {
        let serialized = serde_json::to_string(&verdict).unwrap();
        let deserialized: PolicyVerdict = serde_json::from_str(&serialized).unwrap();
        assert_eq!(
            verdict, deserialized,
            "PolicyVerdict::{:?} must round-trip through JSON",
            verdict
        );
    }
}

#[test]
fn test_policy_verdict_yaml_roundtrip() {
    let verdicts = [
        PolicyVerdict::Allow,
        PolicyVerdict::Deny,
        PolicyVerdict::Redirect,
        PolicyVerdict::Audit,
    ];

    for verdict in verdicts {
        let serialized = serde_yaml::to_string(&verdict).unwrap();
        let deserialized: PolicyVerdict = serde_yaml::from_str(&serialized).unwrap();
        assert_eq!(
            verdict, deserialized,
            "PolicyVerdict::{:?} must round-trip through YAML",
            verdict
        );
    }
}

#[test]
fn test_policy_verdict_json_values() {
    // Verify the JSON representation is human-readable
    let json = serde_json::to_string(&PolicyVerdict::Allow).unwrap();
    assert!(
        json.contains("Allow"),
        "PolicyVerdict::Allow should serialize as readable string, got: {}",
        json
    );
}

// ---------------------------------------------------------------------------
// Tests: ConntrackState equality
// ---------------------------------------------------------------------------

#[test]
fn test_conntrack_state_equality_same_variant() {
    assert_eq!(ConntrackState::New, ConntrackState::New);
    assert_eq!(ConntrackState::Established, ConntrackState::Established);
    assert_eq!(ConntrackState::Related, ConntrackState::Related);
    assert_eq!(ConntrackState::Invalid, ConntrackState::Invalid);
}

#[test]
fn test_conntrack_state_inequality_different_variants() {
    assert_ne!(ConntrackState::New, ConntrackState::Established);
    assert_ne!(ConntrackState::New, ConntrackState::Related);
    assert_ne!(ConntrackState::New, ConntrackState::Invalid);
    assert_ne!(ConntrackState::Established, ConntrackState::Related);
    assert_ne!(ConntrackState::Established, ConntrackState::Invalid);
    assert_ne!(ConntrackState::Related, ConntrackState::Invalid);
}

// ---------------------------------------------------------------------------
// Tests: All map types can be read from mock reader (no panics / errors)
// ---------------------------------------------------------------------------

#[test]
fn test_all_map_reads_succeed_on_mock_reader() {
    let reader = MockMapReader;

    // Each read must return Ok, regardless of contents
    assert!(reader.read_policy_map().is_ok(), "read_policy_map failed");
    assert!(
        reader.read_conntrack_map().is_ok(),
        "read_conntrack_map failed"
    );
    assert!(reader.read_lb_map().is_ok(), "read_lb_map failed");
    assert!(reader.read_ipcache_map().is_ok(), "read_ipcache_map failed");
    assert!(reader.read_drop_map().is_ok(), "read_drop_map failed");
}

// ---------------------------------------------------------------------------
// Tests: PolicyVerdict equality
// ---------------------------------------------------------------------------

#[test]
fn test_policy_verdict_equality() {
    assert_eq!(PolicyVerdict::Allow, PolicyVerdict::Allow);
    assert_eq!(PolicyVerdict::Deny, PolicyVerdict::Deny);
    assert_eq!(PolicyVerdict::Redirect, PolicyVerdict::Redirect);
    assert_eq!(PolicyVerdict::Audit, PolicyVerdict::Audit);
}

#[test]
fn test_policy_verdict_inequality() {
    assert_ne!(PolicyVerdict::Allow, PolicyVerdict::Deny);
    assert_ne!(PolicyVerdict::Allow, PolicyVerdict::Redirect);
    assert_ne!(PolicyVerdict::Allow, PolicyVerdict::Audit);
    assert_ne!(PolicyVerdict::Deny, PolicyVerdict::Redirect);
    assert_ne!(PolicyVerdict::Deny, PolicyVerdict::Audit);
    assert_ne!(PolicyVerdict::Redirect, PolicyVerdict::Audit);
}

// ---------------------------------------------------------------------------
// Tests: EbpfMetrics default values
// ---------------------------------------------------------------------------

#[test]
fn test_ebpf_metrics_default_is_all_zeros() {
    let m = EbpfMetrics::default();
    assert_eq!(m.total_packets, 0);
    assert_eq!(m.dropped_packets, 0);
    assert_eq!(m.forwarded_packets, 0);
    assert_eq!(m.policy_drops, 0);
    assert_eq!(m.nat_lookups, 0);
    assert_eq!(m.ct_lookups, 0);
}
