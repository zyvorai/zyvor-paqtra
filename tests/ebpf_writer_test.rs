// Copyright 2026 Zyvor AI Labs · https://zyvor.dev
// SPDX-License-Identifier: LicenseRef-Zyvor-Production-1.0

/// Integration tests for the MapWriter trait using a MockMapWriter.
///
/// These tests exercise the full MapWriter contract (policy, LB, ipcache,
/// metrics) against an in-memory mock, verifying CRUD semantics, protocol
/// isolation, edge cases, and IPv6-style addressing.
use anyhow::Result;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Mutex;

use paqtra::ebpf::MapWriter;

// ---------------------------------------------------------------------------
// Mock implementation
// ---------------------------------------------------------------------------

/// Key for the policy map: (src_identity, dst_port, protocol).
type PolicyKey = (u32, u16, u8);

/// Key for the LB map: (service_ip, service_port, slot, protocol).
type LbKey = (Ipv4Addr, u16, u16, u8);

/// Value stored for each LB backend: (backend_ip, backend_port).
type LbValue = (Ipv4Addr, u16);

/// Key for the ipcache map: (ip, prefix_len).
type IpcacheKey = (Ipv4Addr, u32);

struct MockMapWriter {
    /// (src_identity, dst_port, protocol) -> allow flag
    policy_entries: Mutex<HashMap<PolicyKey, bool>>,
    /// (service_ip, service_port, slot, protocol) -> (backend_ip, backend_port)
    lb_entries: Mutex<HashMap<LbKey, LbValue>>,
    /// (ip, prefix_len) -> identity
    ipcache_entries: Mutex<HashMap<IpcacheKey, u32>>,
    /// Whether clear_metrics has been called at least once.
    metrics_cleared: Mutex<bool>,
}

impl MockMapWriter {
    fn new() -> Self {
        Self {
            policy_entries: Mutex::new(HashMap::new()),
            lb_entries: Mutex::new(HashMap::new()),
            ipcache_entries: Mutex::new(HashMap::new()),
            metrics_cleared: Mutex::new(false),
        }
    }
}

impl MapWriter for MockMapWriter {
    fn write_policy_entry(
        &self,
        src_identity: u32,
        dst_port: u16,
        protocol: u8,
        allow: bool,
    ) -> Result<()> {
        self.policy_entries
            .lock()
            .unwrap()
            .insert((src_identity, dst_port, protocol), allow);
        Ok(())
    }

    fn delete_policy_entry(&self, src_identity: u32, dst_port: u16, protocol: u8) -> Result<()> {
        self.policy_entries
            .lock()
            .unwrap()
            .remove(&(src_identity, dst_port, protocol));
        Ok(())
    }

    fn write_lb_entry(
        &self,
        service_ip: Ipv4Addr,
        service_port: u16,
        backend_ip: Ipv4Addr,
        backend_port: u16,
        slot: u16,
        protocol: u8,
    ) -> Result<()> {
        self.lb_entries.lock().unwrap().insert(
            (service_ip, service_port, slot, protocol),
            (backend_ip, backend_port),
        );
        Ok(())
    }

    fn delete_lb_entry(
        &self,
        service_ip: Ipv4Addr,
        service_port: u16,
        slot: u16,
        protocol: u8,
    ) -> Result<()> {
        self.lb_entries
            .lock()
            .unwrap()
            .remove(&(service_ip, service_port, slot, protocol));
        Ok(())
    }

    fn write_ipcache_entry(&self, ip: Ipv4Addr, identity: u32, prefix_len: u32) -> Result<()> {
        self.ipcache_entries
            .lock()
            .unwrap()
            .insert((ip, prefix_len), identity);
        Ok(())
    }

    fn clear_metrics(&self) -> Result<()> {
        *self.metrics_cleared.lock().unwrap() = true;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 1. Policy entry CRUD
// ---------------------------------------------------------------------------

#[test]
fn test_write_policy_entry() {
    let writer = MockMapWriter::new();
    writer.write_policy_entry(100, 80, 6, true).unwrap();

    let map = writer.policy_entries.lock().unwrap();
    assert_eq!(map.get(&(100, 80, 6)), Some(&true));
}

#[test]
fn test_write_policy_entry_deny() {
    let writer = MockMapWriter::new();
    writer.write_policy_entry(100, 80, 6, false).unwrap();

    let map = writer.policy_entries.lock().unwrap();
    assert_eq!(map.get(&(100, 80, 6)), Some(&false));
}

#[test]
fn test_delete_policy_entry() {
    let writer = MockMapWriter::new();
    writer.write_policy_entry(100, 80, 6, true).unwrap();
    assert!(writer
        .policy_entries
        .lock()
        .unwrap()
        .contains_key(&(100, 80, 6)));

    writer.delete_policy_entry(100, 80, 6).unwrap();
    assert!(!writer
        .policy_entries
        .lock()
        .unwrap()
        .contains_key(&(100, 80, 6)));
}

#[test]
fn test_write_policy_entry_different_protocols() {
    let writer = MockMapWriter::new();
    // TCP (protocol 6) and UDP (protocol 17) for the same identity+port
    writer.write_policy_entry(100, 80, 6, true).unwrap();
    writer.write_policy_entry(100, 80, 17, false).unwrap();

    let map = writer.policy_entries.lock().unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(map.get(&(100, 80, 6)), Some(&true));
    assert_eq!(map.get(&(100, 80, 17)), Some(&false));
}

// ---------------------------------------------------------------------------
// 2. LB entry CRUD with protocol parameter
// ---------------------------------------------------------------------------

#[test]
fn test_write_lb_entry_tcp() {
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 1);
    let be_ip = Ipv4Addr::new(10, 0, 1, 10);

    writer
        .write_lb_entry(svc_ip, 443, be_ip, 8443, 1, 6)
        .unwrap();

    let map = writer.lb_entries.lock().unwrap();
    assert_eq!(map.get(&(svc_ip, 443, 1, 6)), Some(&(be_ip, 8443)));
}

#[test]
fn test_write_lb_entry_udp() {
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 10);
    let be_ip = Ipv4Addr::new(10, 0, 2, 20);

    writer
        .write_lb_entry(svc_ip, 53, be_ip, 5353, 1, 17)
        .unwrap();

    let map = writer.lb_entries.lock().unwrap();
    assert_eq!(map.get(&(svc_ip, 53, 1, 17)), Some(&(be_ip, 5353)));
}

#[test]
fn test_delete_lb_entry() {
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 1);
    let be_ip = Ipv4Addr::new(10, 0, 1, 10);

    writer
        .write_lb_entry(svc_ip, 443, be_ip, 8443, 1, 6)
        .unwrap();
    assert!(writer
        .lb_entries
        .lock()
        .unwrap()
        .contains_key(&(svc_ip, 443, 1, 6)));

    writer.delete_lb_entry(svc_ip, 443, 1, 6).unwrap();
    assert!(!writer
        .lb_entries
        .lock()
        .unwrap()
        .contains_key(&(svc_ip, 443, 1, 6)));
}

#[test]
fn test_lb_entry_protocol_isolation() {
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 1);
    let be_tcp = Ipv4Addr::new(10, 0, 1, 10);
    let be_udp = Ipv4Addr::new(10, 0, 1, 20);

    // Same service IP:port and slot, but different protocols
    writer
        .write_lb_entry(svc_ip, 443, be_tcp, 8443, 1, 6)
        .unwrap();
    writer
        .write_lb_entry(svc_ip, 443, be_udp, 9443, 1, 17)
        .unwrap();

    let map = writer.lb_entries.lock().unwrap();
    assert_eq!(map.len(), 2);
    assert_eq!(map.get(&(svc_ip, 443, 1, 6)), Some(&(be_tcp, 8443)));
    assert_eq!(map.get(&(svc_ip, 443, 1, 17)), Some(&(be_udp, 9443)));

    // Deleting the TCP entry must not affect the UDP entry
    drop(map);
    writer.delete_lb_entry(svc_ip, 443, 1, 6).unwrap();
    let map = writer.lb_entries.lock().unwrap();
    assert!(!map.contains_key(&(svc_ip, 443, 1, 6)));
    assert!(map.contains_key(&(svc_ip, 443, 1, 17)));
}

// ---------------------------------------------------------------------------
// 3. IPCache CRUD
// ---------------------------------------------------------------------------

#[test]
fn test_write_ipcache_entry() {
    let writer = MockMapWriter::new();
    let ip = Ipv4Addr::new(10, 0, 0, 1);

    writer.write_ipcache_entry(ip, 100, 32).unwrap();

    let map = writer.ipcache_entries.lock().unwrap();
    assert_eq!(map.get(&(ip, 32)), Some(&100));
}

#[test]
fn test_delete_ipcache_entry_by_overwrite() {
    // The MapWriter trait does not expose a delete_ipcache_entry method, so
    // the only way to "remove" an entry in practice is to overwrite it with a
    // sentinel identity (0 = world identity in Cilium).  Verify that
    // overwriting works correctly.
    let writer = MockMapWriter::new();
    let ip = Ipv4Addr::new(10, 0, 0, 1);

    writer.write_ipcache_entry(ip, 100, 32).unwrap();
    assert_eq!(
        *writer
            .ipcache_entries
            .lock()
            .unwrap()
            .get(&(ip, 32))
            .unwrap(),
        100
    );

    // Overwrite with identity 0 (world)
    writer.write_ipcache_entry(ip, 0, 32).unwrap();
    assert_eq!(
        *writer
            .ipcache_entries
            .lock()
            .unwrap()
            .get(&(ip, 32))
            .unwrap(),
        0
    );
}

#[test]
fn test_ipcache_overwrite() {
    let writer = MockMapWriter::new();
    let ip = Ipv4Addr::new(10, 0, 0, 1);

    writer.write_ipcache_entry(ip, 100, 32).unwrap();
    writer.write_ipcache_entry(ip, 200, 32).unwrap();

    let map = writer.ipcache_entries.lock().unwrap();
    assert_eq!(map.len(), 1, "overwrite should not create a second entry");
    assert_eq!(map.get(&(ip, 32)), Some(&200));
}

// ---------------------------------------------------------------------------
// 4. Metrics
// ---------------------------------------------------------------------------

#[test]
fn test_clear_metrics() {
    let writer = MockMapWriter::new();
    assert!(!*writer.metrics_cleared.lock().unwrap());

    writer.clear_metrics().unwrap();
    assert!(*writer.metrics_cleared.lock().unwrap());
}

#[test]
fn test_clear_metrics_idempotent() {
    let writer = MockMapWriter::new();
    writer.clear_metrics().unwrap();
    writer.clear_metrics().unwrap();
    assert!(*writer.metrics_cleared.lock().unwrap());
}

// ---------------------------------------------------------------------------
// 5. Edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_write_lb_entry_zero_slot() {
    // Slot 0 is used by Cilium for the service header entry.
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 1);
    let be_ip = Ipv4Addr::new(10, 0, 1, 10);

    writer
        .write_lb_entry(svc_ip, 443, be_ip, 8443, 0, 6)
        .unwrap();

    let map = writer.lb_entries.lock().unwrap();
    assert_eq!(map.get(&(svc_ip, 443, 0, 6)), Some(&(be_ip, 8443)));
}

#[test]
fn test_policy_entry_all_ports() {
    // Port 0 is the Cilium wildcard meaning "all ports".
    let writer = MockMapWriter::new();
    writer.write_policy_entry(100, 0, 6, true).unwrap();

    let map = writer.policy_entries.lock().unwrap();
    assert_eq!(map.get(&(100, 0, 6)), Some(&true));
}

#[test]
fn test_ipcache_ipv6_mapped_ipv4() {
    // The MapWriter trait accepts Ipv4Addr, but we can still verify that the
    // IPv4-mapped IPv6 range (::ffff:0:0/96) round-trips correctly by using
    // the equivalent IPv4 address.
    let writer = MockMapWriter::new();
    let ip = Ipv4Addr::new(192, 168, 1, 1);

    writer.write_ipcache_entry(ip, 500, 128).unwrap();

    let map = writer.ipcache_entries.lock().unwrap();
    assert_eq!(map.get(&(ip, 128)), Some(&500));
}

#[test]
fn test_delete_nonexistent_policy_entry() {
    // Deleting a key that was never written should succeed silently (HashMap::remove is a no-op).
    let writer = MockMapWriter::new();
    writer.delete_policy_entry(999, 443, 6).unwrap();
    assert!(writer.policy_entries.lock().unwrap().is_empty());
}

#[test]
fn test_delete_nonexistent_lb_entry() {
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 1);
    writer.delete_lb_entry(svc_ip, 443, 1, 6).unwrap();
    assert!(writer.lb_entries.lock().unwrap().is_empty());
}

#[test]
fn test_policy_entry_overwrite_verdict() {
    // Writing the same key twice should update the allow flag.
    let writer = MockMapWriter::new();
    writer.write_policy_entry(100, 80, 6, true).unwrap();
    writer.write_policy_entry(100, 80, 6, false).unwrap();

    let map = writer.policy_entries.lock().unwrap();
    assert_eq!(map.len(), 1);
    assert_eq!(map.get(&(100, 80, 6)), Some(&false));
}

#[test]
fn test_lb_multiple_slots() {
    // Multiple backends for the same service are distinguished by slot number.
    let writer = MockMapWriter::new();
    let svc_ip = Ipv4Addr::new(10, 96, 0, 1);
    let be1 = Ipv4Addr::new(10, 0, 1, 10);
    let be2 = Ipv4Addr::new(10, 0, 1, 11);
    let be3 = Ipv4Addr::new(10, 0, 1, 12);

    writer.write_lb_entry(svc_ip, 80, be1, 8080, 1, 6).unwrap();
    writer.write_lb_entry(svc_ip, 80, be2, 8080, 2, 6).unwrap();
    writer.write_lb_entry(svc_ip, 80, be3, 8080, 3, 6).unwrap();

    let map = writer.lb_entries.lock().unwrap();
    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&(svc_ip, 80, 1, 6)), Some(&(be1, 8080)));
    assert_eq!(map.get(&(svc_ip, 80, 2, 6)), Some(&(be2, 8080)));
    assert_eq!(map.get(&(svc_ip, 80, 3, 6)), Some(&(be3, 8080)));
}

#[test]
fn test_ipcache_different_prefix_lengths() {
    // The same IP with different prefix lengths should be stored as separate entries.
    let writer = MockMapWriter::new();
    let ip = Ipv4Addr::new(10, 0, 0, 0);

    writer.write_ipcache_entry(ip, 100, 8).unwrap();
    writer.write_ipcache_entry(ip, 200, 16).unwrap();
    writer.write_ipcache_entry(ip, 300, 24).unwrap();

    let map = writer.ipcache_entries.lock().unwrap();
    assert_eq!(map.len(), 3);
    assert_eq!(map.get(&(ip, 8)), Some(&100));
    assert_eq!(map.get(&(ip, 16)), Some(&200));
    assert_eq!(map.get(&(ip, 24)), Some(&300));
}

// ---------------------------------------------------------------------------
// 6. Trait-object usage (dyn MapWriter)
// ---------------------------------------------------------------------------

#[test]
fn test_map_writer_as_trait_object() {
    // Verify that MockMapWriter can be used behind a dyn MapWriter reference,
    // ensuring the trait is object-safe.
    let writer = MockMapWriter::new();
    let dyn_writer: &dyn MapWriter = &writer;

    dyn_writer.write_policy_entry(1, 80, 6, true).unwrap();
    dyn_writer
        .write_lb_entry(
            Ipv4Addr::new(10, 96, 0, 1),
            443,
            Ipv4Addr::new(10, 0, 1, 10),
            8443,
            1,
            6,
        )
        .unwrap();
    dyn_writer
        .write_ipcache_entry(Ipv4Addr::new(10, 0, 0, 1), 100, 32)
        .unwrap();
    dyn_writer.clear_metrics().unwrap();

    // Verify the writes landed in the backing store
    assert_eq!(writer.policy_entries.lock().unwrap().len(), 1);
    assert_eq!(writer.lb_entries.lock().unwrap().len(), 1);
    assert_eq!(writer.ipcache_entries.lock().unwrap().len(), 1);
    assert!(*writer.metrics_cleared.lock().unwrap());
}
