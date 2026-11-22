// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-2.0

/// BPF Data Structure Parsers
///
/// Parses raw binary data from Cilium BPF maps into Rust structures
use anyhow::Result;
use byteorder::{ByteOrder, LittleEndian, NetworkEndian};
use std::net::{Ipv4Addr, Ipv6Addr};

use super::{
    ConntrackEntry, ConntrackState, DropReasonType, IPCacheEntry, LoadBalancerEntry,
    PolicyDecision, PolicyVerdict,
};

/// Parse Cilium policy map entry
///
/// Key format: struct policy_key {
///   u32 sec_label;     // Source security identity
///   u16 dport;         // Destination port (network byte order)
///   u8  protocol;      // IP protocol
///   u8  egress;        // Direction (1=egress, 0=ingress)
///   u8  pad[4];
/// }
///
/// Value format: struct policy_entry {
///   u64 packets;
///   u64 bytes;
///   u16 proxy_port;
///   u16 _pad;
///   u32 _pad2;
/// }
pub fn parse_policy_entry(key: &[u8], _value: &[u8]) -> Result<PolicyDecision> {
    if key.len() < 12 {
        anyhow::bail!("Policy key too short: {} bytes", key.len());
    }

    let src_identity = LittleEndian::read_u32(&key[0..4]);
    let dst_port = NetworkEndian::read_u16(&key[4..6]);
    let protocol = key[6];
    let _egress = key[7];

    // Entries in the Cilium policy map represent allowed traffic.
    // The presence of an entry means the policy allows this traffic,
    // regardless of the packet count.
    let verdict = PolicyVerdict::Allow;

    Ok(PolicyDecision {
        src_identity,
        dst_identity: 0, // Not in policy map key
        port: dst_port,
        protocol,
        verdict,
    })
}

/// Parse Cilium connection tracking entry
///
/// CT4 Key format: struct ipv4_ct_tuple {
///   __be32 daddr;      // Destination IPv4
///   __be32 saddr;      // Source IPv4
///   __be16 dport;      // Destination port
///   __be16 sport;      // Source port
///   __u8   nexthdr;    // Protocol
///   __u8   flags;
/// }
///
/// Value format: struct ct_entry {
///   u64 rx_packets;
///   u64 rx_bytes;
///   u64 tx_packets;
///   u64 tx_bytes;
///   u32 lifetime;
///   u16 rx_closing:1;
///   u16 tx_closing:1;
///   ...
/// }
pub fn parse_ct_entry(key: &[u8], value: &[u8]) -> Result<ConntrackEntry> {
    if key.len() < 14 {
        anyhow::bail!("CT key too short: {} bytes", key.len());
    }

    // Parse IPv4 addresses (network byte order)
    let dst_ip = Ipv4Addr::new(key[0], key[1], key[2], key[3]);
    let src_ip = Ipv4Addr::new(key[4], key[5], key[6], key[7]);

    // Parse ports (network byte order)
    let dst_port = NetworkEndian::read_u16(&key[8..10]);
    let src_port = NetworkEndian::read_u16(&key[10..12]);

    // Protocol
    let protocol = key[12];

    // Flags byte at key offset 13
    let flags = key[13];

    // Parse value
    let (packets, bytes, lifetime, state) = if value.len() >= 36 {
        let rx_packets = LittleEndian::read_u64(&value[0..8]);
        let rx_bytes = LittleEndian::read_u64(&value[8..16]);
        let tx_packets = LittleEndian::read_u64(&value[16..24]);
        let tx_bytes = LittleEndian::read_u64(&value[24..32]);
        let lifetime = LittleEndian::read_u32(&value[32..36]);

        // Total packets and bytes (use saturating_add to prevent overflow)
        let total_packets = rx_packets.saturating_add(tx_packets);
        let total_bytes = rx_bytes.saturating_add(tx_bytes);

        // Derive state from flags
        // rx_closing = bit 0, tx_closing = bit 1
        let rx_closing = flags & 0x01 != 0;
        let tx_closing = flags & 0x02 != 0;
        let state = if rx_closing && tx_closing {
            ConntrackState::Invalid // closing
        } else if total_packets > 0 {
            ConntrackState::Established
        } else {
            ConntrackState::New
        };

        (total_packets, total_bytes, u64::from(lifetime), state)
    } else if value.len() >= 32 {
        let rx_packets = LittleEndian::read_u64(&value[0..8]);
        let rx_bytes = LittleEndian::read_u64(&value[8..16]);
        let tx_packets = LittleEndian::read_u64(&value[16..24]);
        let tx_bytes = LittleEndian::read_u64(&value[24..32]);

        let total_packets = rx_packets.saturating_add(tx_packets);
        let total_bytes = rx_bytes.saturating_add(tx_bytes);

        let rx_closing = flags & 0x01 != 0;
        let tx_closing = flags & 0x02 != 0;
        let state = if rx_closing && tx_closing {
            ConntrackState::Invalid
        } else if total_packets > 0 {
            ConntrackState::Established
        } else {
            ConntrackState::New
        };

        (total_packets, total_bytes, 0u64, state)
    } else {
        (0, 0, 0u64, ConntrackState::New)
    };

    Ok(ConntrackEntry {
        src_ip: src_ip.to_string(),
        dst_ip: dst_ip.to_string(),
        src_port,
        dst_port,
        protocol,
        state,
        packets,
        bytes,
        last_seen: lifetime,
        src_namespace: None,
        src_pod: None,
        src_labels: None,
        dst_namespace: None,
        dst_pod: None,
        dst_labels: None,
    })
}

/// Parse IPv6 connection tracking entry
pub fn parse_ct6_entry(key: &[u8], value: &[u8]) -> Result<ConntrackEntry> {
    if key.len() < 38 {
        anyhow::bail!("CT6 key too short: {} bytes", key.len());
    }

    // Parse IPv6 addresses (16 bytes each)
    let dst_ip = Ipv6Addr::from([
        key[0], key[1], key[2], key[3], key[4], key[5], key[6], key[7], key[8], key[9], key[10],
        key[11], key[12], key[13], key[14], key[15],
    ]);

    let src_ip = Ipv6Addr::from([
        key[16], key[17], key[18], key[19], key[20], key[21], key[22], key[23], key[24], key[25],
        key[26], key[27], key[28], key[29], key[30], key[31],
    ]);

    // Parse ports
    let dst_port = NetworkEndian::read_u16(&key[32..34]);
    let src_port = NetworkEndian::read_u16(&key[34..36]);

    // Protocol
    let protocol = key[36];

    // Flags byte at key offset 37
    let flags = key[37];

    // Parse value
    let (packets, bytes, lifetime, state) = if value.len() >= 36 {
        let rx_packets = LittleEndian::read_u64(&value[0..8]);
        let rx_bytes = LittleEndian::read_u64(&value[8..16]);
        let tx_packets = LittleEndian::read_u64(&value[16..24]);
        let tx_bytes = LittleEndian::read_u64(&value[24..32]);
        let lifetime = LittleEndian::read_u32(&value[32..36]);

        let total_packets = rx_packets.saturating_add(tx_packets);
        let total_bytes = rx_bytes.saturating_add(tx_bytes);

        // Derive state from flags
        let rx_closing = flags & 0x01 != 0;
        let tx_closing = flags & 0x02 != 0;
        let state = if rx_closing && tx_closing {
            ConntrackState::Invalid // closing
        } else if total_packets > 0 {
            ConntrackState::Established
        } else {
            ConntrackState::New
        };

        (total_packets, total_bytes, u64::from(lifetime), state)
    } else if value.len() >= 32 {
        let rx_packets = LittleEndian::read_u64(&value[0..8]);
        let rx_bytes = LittleEndian::read_u64(&value[8..16]);
        let tx_packets = LittleEndian::read_u64(&value[16..24]);
        let tx_bytes = LittleEndian::read_u64(&value[24..32]);

        let total_packets = rx_packets.saturating_add(tx_packets);
        let total_bytes = rx_bytes.saturating_add(tx_bytes);

        let rx_closing = flags & 0x01 != 0;
        let tx_closing = flags & 0x02 != 0;
        let state = if rx_closing && tx_closing {
            ConntrackState::Invalid
        } else if total_packets > 0 {
            ConntrackState::Established
        } else {
            ConntrackState::New
        };

        (total_packets, total_bytes, 0u64, state)
    } else {
        (0, 0, 0u64, ConntrackState::New)
    };

    Ok(ConntrackEntry {
        src_ip: src_ip.to_string(),
        dst_ip: dst_ip.to_string(),
        src_port,
        dst_port,
        protocol,
        state,
        packets,
        bytes,
        last_seen: lifetime,
        src_namespace: None,
        src_pod: None,
        src_labels: None,
        dst_namespace: None,
        dst_pod: None,
        dst_labels: None,
    })
}

/// Parse IP cache entry
///
/// Key format (IPv4): struct ipcache_key {
///   struct bpf_lpm_trie_key lpm_key;
///   u16 cluster_id;
///   __u8 family;
///   __u8 pad;
///   union {
///     struct {
///       __be32 ip4;
///       __u32 pad1;
///       __u32 pad2;
///       __u32 pad3;
///     };
///     __be32 ip6[4];
///   } ip_data;
/// }
///
/// Value format: struct remote_endpoint_info {
///   __u32 sec_label;
///   ...
/// }
pub fn parse_ipcache_entry(key: &[u8], value: &[u8]) -> Result<IPCacheEntry> {
    if key.len() < 8 {
        anyhow::bail!("IPCache key too short: {} bytes", key.len());
    }

    // Skip LPM trie prefix (4 bytes)
    // cluster_id (2 bytes), family (1 byte), pad (1 byte)
    let family = if key.len() > 6 { key[6] } else { 2 }; // 2 = AF_INET

    // Parse IP based on family
    let ip = if family == 2 && key.len() >= 12 {
        // IPv4
        let ip4 = Ipv4Addr::new(key[8], key[9], key[10], key[11]);
        ip4.to_string()
    } else if family == 10 && key.len() >= 24 {
        // IPv6
        let ip6 = Ipv6Addr::from([
            key[8], key[9], key[10], key[11], key[12], key[13], key[14], key[15], key[16], key[17],
            key[18], key[19], key[20], key[21], key[22], key[23],
        ]);
        ip6.to_string()
    } else {
        "0.0.0.0".to_string()
    };

    // Parse identity from value
    let identity = if value.len() >= 4 {
        LittleEndian::read_u32(&value[0..4])
    } else {
        0
    };

    Ok(IPCacheEntry {
        ip,
        identity,
        namespace: String::new(), // Needs K8s resolution
        labels: Vec::new(),       // Needs K8s resolution
    })
}

/// Parse load balancer service entry
///
/// Key format: struct lb4_key {
///   __be32 address;    // Service VIP
///   __be16 dport;      // Service port
///   __u16  backend_slot;
///   __u8   proto;
///   __u8   scope;
///   __u8   pad;
/// }
///
/// Value format: struct lb4_service {
///   __be32 target;     // Backend IP
///   __be16 port;       // Backend port
///   __u16  count;      // Number of backends
///   ...
/// }
pub fn parse_lb_entry(key: &[u8], value: &[u8]) -> Result<LoadBalancerEntry> {
    if key.len() < 11 {
        anyhow::bail!("LB key too short: {} bytes", key.len());
    }

    // Parse service VIP (network byte order)
    let service_ip = Ipv4Addr::new(key[0], key[1], key[2], key[3]);

    // Parse service port (network byte order)
    let service_port = NetworkEndian::read_u16(&key[4..6]);

    // Parse backend from value
    let (backend_ip, backend_port) = if value.len() >= 6 {
        let backend_ip = Ipv4Addr::new(value[0], value[1], value[2], value[3]);
        let backend_port = NetworkEndian::read_u16(&value[4..6]);
        (backend_ip.to_string(), backend_port)
    } else {
        ("0.0.0.0".to_string(), 0)
    };

    Ok(LoadBalancerEntry {
        service_ip: service_ip.to_string(),
        service_port,
        backend_ip,
        backend_port,
        weight: 100,     // Default weight
        active_conns: 0, // Would need separate metrics
    })
}

/// Parse drop/metrics entry
///
/// Key format: struct metrics_key {
///   __u8  reason;      // Drop reason code
///   __u8  dir;         // Direction
///   __u16 reserved;
/// }
///
/// Value format: struct metrics_value {
///   __u64 count;       // Drop count
///   __u64 bytes;       // Bytes dropped
/// }
pub fn parse_metrics_entry(key: &[u8], value: &[u8]) -> Result<(DropReasonType, u64)> {
    if key.is_empty() {
        anyhow::bail!("Metrics key too short");
    }

    let reason_code = key[0] as u32;
    let reason = DropReasonType::from_code(reason_code);

    let count = if value.len() >= 8 {
        LittleEndian::read_u64(&value[0..8])
    } else {
        0
    };

    Ok((reason, count))
}

/// Helper: Convert protocol number to name
pub fn protocol_to_name(proto: u8) -> &'static str {
    match proto {
        1 => "ICMP",
        6 => "TCP",
        17 => "UDP",
        47 => "GRE",
        58 => "ICMPv6",
        132 => "SCTP",
        _ => "Unknown",
    }
}

/// Helper: Convert port to service name (well-known ports)
pub fn port_to_service(port: u16) -> Option<&'static str> {
    match port {
        20 => Some("FTP-DATA"),
        21 => Some("FTP"),
        22 => Some("SSH"),
        23 => Some("Telnet"),
        25 => Some("SMTP"),
        53 => Some("DNS"),
        80 => Some("HTTP"),
        110 => Some("POP3"),
        143 => Some("IMAP"),
        443 => Some("HTTPS"),
        465 => Some("SMTPS"),
        587 => Some("SMTP-Submission"),
        993 => Some("IMAPS"),
        995 => Some("POP3S"),
        3306 => Some("MySQL"),
        5432 => Some("PostgreSQL"),
        6379 => Some("Redis"),
        8080 => Some("HTTP-Alt"),
        8443 => Some("HTTPS-Alt"),
        9090 => Some("Prometheus"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ct_entry() {
        // Mock CT4 entry: 10.0.0.1:12345 -> 10.0.0.2:80 (TCP)
        let key = vec![
            10, 0, 0, 2, // dst_ip
            10, 0, 0, 1, // src_ip
            0x00, 0x50, // dst_port (80)
            0x30, 0x39, // src_port (12345)
            6,    // protocol (TCP)
            0,    // flags
        ];

        let value = vec![
            100, 0, 0, 0, 0, 0, 0, 0, // rx_packets
            0, 0, 50, 0, 0, 0, 0, 0, // rx_bytes
            50, 0, 0, 0, 0, 0, 0, 0, // tx_packets
            0, 0, 25, 0, 0, 0, 0, 0, // tx_bytes
        ];

        let entry = parse_ct_entry(&key, &value).unwrap();

        assert_eq!(entry.src_ip, "10.0.0.1");
        assert_eq!(entry.dst_ip, "10.0.0.2");
        assert_eq!(entry.src_port, 12345);
        assert_eq!(entry.dst_port, 80);
        assert_eq!(entry.protocol, 6);
        assert_eq!(entry.packets, 150);
    }

    #[test]
    fn test_parse_ipcache_entry() {
        // Mock ipcache entry
        let key = vec![
            0, 0, 0, 0, // LPM prefix
            0, 0, // cluster_id
            2, // family (AF_INET)
            0, // pad
            10, 0, 0, 1, // IPv4: 10.0.0.1
        ];

        let value = vec![
            100, 0, 0, 0, // identity: 100
        ];

        let entry = parse_ipcache_entry(&key, &value).unwrap();

        assert_eq!(entry.ip, "10.0.0.1");
        assert_eq!(entry.identity, 100);
    }

    #[test]
    fn test_parse_lb_entry() {
        // Mock LB entry: 10.0.1.100:80 -> 10.0.2.10:8080
        let key = vec![
            10, 0, 1, 100, // service VIP
            0x00, 0x50, // service port (80)
            0, 0, // backend_slot
            6, // proto (TCP)
            0, // scope
            0, // pad
        ];

        let value = vec![
            10, 0, 2, 10, // backend IP
            0x1F, 0x90, // backend port (8080)
        ];

        let entry = parse_lb_entry(&key, &value).unwrap();

        assert_eq!(entry.service_ip, "10.0.1.100");
        assert_eq!(entry.service_port, 80);
        assert_eq!(entry.backend_ip, "10.0.2.10");
        assert_eq!(entry.backend_port, 8080);
    }

    #[test]
    fn test_protocol_to_name() {
        assert_eq!(protocol_to_name(1), "ICMP");
        assert_eq!(protocol_to_name(6), "TCP");
        assert_eq!(protocol_to_name(17), "UDP");
        assert_eq!(protocol_to_name(255), "Unknown");
    }

    #[test]
    fn test_port_to_service() {
        assert_eq!(port_to_service(80), Some("HTTP"));
        assert_eq!(port_to_service(443), Some("HTTPS"));
        assert_eq!(port_to_service(5432), Some("PostgreSQL"));
        assert_eq!(port_to_service(12345), None);
    }

    #[test]
    fn test_parse_metrics_entry() {
        let key = vec![1, 0, 0, 0]; // reason: PolicyDenied
        let value = vec![42, 0, 0, 0, 0, 0, 0, 0]; // count: 42

        let (reason, count) = parse_metrics_entry(&key, &value).unwrap();

        assert_eq!(reason, DropReasonType::PolicyDenied);
        assert_eq!(count, 42);
    }
}
