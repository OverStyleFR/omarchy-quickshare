use std::time::{Duration, Instant};

use crate::EndpointInfo;

pub const DISCOVERY_PEER_LIMIT: usize = 128;
pub const LAN_PEER_TTL: Duration = Duration::from_secs(120);

struct PeerLease {
    peer: EndpointInfo,
    seen: Instant,
}

#[derive(Default)]
pub(crate) struct PeerCache {
    leases: Vec<PeerLease>,
}

fn transport(peer: &EndpointInfo) -> &str {
    peer.transport.as_deref().unwrap_or_else(|| {
        if peer.id.starts_with("bt:") { "bluetooth" } else { "lan" }
    })
}

impl PeerCache {
    pub(crate) fn peers(&self) -> impl Iterator<Item = &EndpointInfo> {
        self.leases.iter().map(|lease| &lease.peer)
    }

    fn remove(&mut self, index: usize, emit: &mut impl FnMut(EndpointInfo)) {
        let mut peer = self.leases.swap_remove(index).peer;
        peer.present = Some(false);
        emit(peer);
    }

    pub(crate) fn apply(
        &mut self,
        mut peer: EndpointInfo,
        now: Instant,
        mut emit: impl FnMut(EndpointInfo),
    ) {
        if peer.id.is_empty() {
            return;
        }
        if peer.transport.is_none() {
            peer.transport = Some(transport(&peer).to_owned());
        }
        if peer.present != Some(true) {
            if let Some(index) = self.leases.iter().position(|lease| {
                lease.peer.id == peer.id
                    && transport(&lease.peer) == transport(&peer)
                    && (peer.fullname.is_empty() || lease.peer.fullname == peer.fullname)
            }) {
                self.remove(index, &mut emit);
            }
            return;
        }

        let mut index = 0;
        while index < self.leases.len() {
            let current = &self.leases[index].peer;
            let same_identity = !peer.fullname.is_empty()
                && current.fullname == peer.fullname
                && transport(current) == transport(&peer);
            let same_owner = current.fullname == peer.fullname
                && transport(current) == transport(&peer);
            if (same_identity && current.id != peer.id)
                || (current.id == peer.id && !same_owner)
            {
                self.remove(index, &mut emit);
            } else {
                index += 1;
            }
        }

        if let Some(lease) = self.leases.iter_mut().find(|lease| lease.peer.id == peer.id) {
            lease.seen = now;
            lease.peer = peer;
            emit(lease.peer.clone());
            return;
        }
        if self.leases.len() == DISCOVERY_PEER_LIMIT {
            let oldest = self.leases.iter().enumerate()
                .min_by_key(|(_, lease)| lease.seen)
                .map(|(index, _)| index)
                .expect("a full peer cache is nonempty");
            self.remove(oldest, &mut emit);
        }
        self.leases.push(PeerLease { peer, seen: now });
        emit(self.leases.last().expect("inserted peer").peer.clone());
    }

    pub(crate) fn replace_address(
        &mut self,
        transport_name: &str,
        fullname: &str,
        new_id: Option<&str>,
        mut emit: impl FnMut(EndpointInfo),
    ) {
        if fullname.is_empty() {
            return;
        }
        if let Some(index) = self.leases.iter().position(|lease| {
            transport(&lease.peer) == transport_name
                && lease.peer.fullname == fullname
                && new_id != Some(lease.peer.id.as_str())
        }) {
            self.remove(index, &mut emit);
        }
    }

    pub(crate) fn expire(&mut self, now: Instant, mut emit: impl FnMut(EndpointInfo)) {
        let mut index = 0;
        while index < self.leases.len() {
            let lease = &self.leases[index];
            // Bluetooth emits metadata changes, not every observation. Its own
            // discovery backend owns liveness; the LAN clock cannot expire it.
            if transport(&lease.peer) == "lan"
                && now.saturating_duration_since(lease.seen) >= LAN_PEER_TTL
            {
                self.remove(index, &mut emit);
            } else {
                index += 1;
            }
        }
    }

    pub(crate) fn clear(&mut self, mut emit: impl FnMut(EndpointInfo)) {
        while !self.leases.is_empty() {
            self.remove(self.leases.len() - 1, &mut emit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(fullname: &str, id: &str) -> EndpointInfo {
        EndpointInfo {
            fullname: fullname.into(),
            id: id.into(),
            name: Some("Phone".into()),
            ip: Some("192.0.2.1".into()),
            port: Some("1234".into()),
            rtype: Some(crate::DeviceType::Phone),
            present: Some(true),
            transport: None,
        }
    }

    #[test]
    fn address_and_port_changes_withdraw_full_old_identity_before_addition() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        let original = peer("phone.local.", "192.0.2.1:1234");
        cache.apply(original.clone(), now, |_| {});
        let mut events = Vec::new();
        cache.replace_address("lan", &original.fullname, Some("192.0.2.2:5678"), |p| events.push(p));
        assert_eq!(cache.peers().count(), 0);
        let mut replacement = peer(&original.fullname, "192.0.2.2:5678");
        replacement.ip = Some("192.0.2.2".into());
        replacement.port = Some("5678".into());
        cache.apply(replacement, now, |p| events.push(p));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].present, Some(false));
        assert_eq!(events[0].id, original.id);
        assert_eq!(events[0].fullname, original.fullname);
        assert_eq!(events[0].transport.as_deref(), Some("lan"));
        assert_eq!(events[1].present, Some(true));
        assert_eq!(events[1].id, "192.0.2.2:5678");
    }

    #[test]
    fn applying_a_new_route_retires_only_the_same_transport_identity() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        cache.apply(peer("same-endpoint", "bt:device"), now, |_| {});
        cache.apply(peer("same-endpoint", "192.0.2.1:1234"), now, |_| {});
        let mut events = Vec::new();
        cache.apply(peer("same-endpoint", "192.0.2.2:5678"), now, |p| events.push(p));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].id, "192.0.2.1:1234");
        assert_eq!(events[0].present, Some(false));
        assert_eq!(events[1].id, "192.0.2.2:5678");
        assert_eq!(events[1].present, Some(true));
        assert_eq!(cache.peers().count(), 2);
        assert!(cache.peers().any(|peer| peer.id == "bt:device"));
    }

    #[test]
    fn new_fullname_owns_route_despite_late_old_owner_removal() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        let mut old = peer("old.local.", "192.0.2.1:1234");
        cache.apply(old.clone(), now, |_| {});
        let mut events = Vec::new();
        cache.apply(peer("new.local.", &old.id), now, |p| events.push(p));
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].present, Some(false));
        old.present = Some(false);
        cache.apply(old, now, |p| events.push(p));
        cache.replace_address("lan", "old.local.", None, |p| events.push(p));
        assert_eq!(events.len(), 2);
        assert_eq!(cache.peers().next().unwrap().fullname, "new.local.");
    }

    #[test]
    fn count_limit_evicts_oldest_observation_across_transports() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        for index in 0..DISCOVERY_PEER_LIMIT {
            let id = if index % 2 == 0 { format!("bt:{index}") } else { format!("192.0.2.1:{index}") };
            cache.apply(peer(&format!("peer-{index}"), &id), now + Duration::from_secs(index as u64), |_| {});
        }
        cache.apply(peer("peer-0", "bt:0"), now + Duration::from_secs(200), |_| {});
        let mut events = Vec::new();
        for index in 0..10 {
            cache.apply(peer(&format!("extra-{index}"), &format!("192.0.2.2:{index}")), now + Duration::from_secs(201 + index), |p| events.push(p));
            assert_eq!(cache.peers().count(), DISCOVERY_PEER_LIMIT);
        }
        assert_eq!(events.len(), 20);
        assert_eq!(events[0].id, "192.0.2.1:1");
        assert_eq!(events[0].present, Some(false));
        assert!(cache.peers().any(|peer| peer.id == "bt:0"));
    }

    #[test]
    fn lan_lease_boundary_and_identical_refresh_are_monotonic() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        let endpoint = peer("phone.local.", "192.0.2.1:1234");
        cache.apply(endpoint.clone(), now, |_| {});
        cache.expire(now + LAN_PEER_TTL - Duration::from_nanos(1), |_| panic!("premature expiry"));
        cache.apply(endpoint.clone(), now + Duration::from_secs(60), |_| {});
        cache.expire(now + LAN_PEER_TTL, |_| panic!("refresh not honored"));
        let mut events = Vec::new();
        cache.expire(now + Duration::from_secs(60) + LAN_PEER_TTL, |p| events.push(p));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, endpoint.id);
        assert_eq!(events[0].present, Some(false));
        assert_eq!(cache.peers().count(), 0);
    }

    #[test]
    fn unchanged_bluetooth_does_not_expire_on_lan_clock() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        cache.apply(peer("bt:device", "bt:device"), now, |_| {});
        cache.expire(now + LAN_PEER_TTL * 10, |_| panic!("Bluetooth expired"));
        assert_eq!(cache.peers().count(), 1);
        assert_eq!(cache.peers().next().unwrap().transport.as_deref(), Some("bluetooth"));
    }

    #[test]
    fn invalid_or_unreachable_replacement_and_shutdown_retire_owned_peers() {
        let now = Instant::now();
        let mut cache = PeerCache::default();
        cache.apply(peer("phone.local.", "192.0.2.1:1234"), now, |_| {});
        let mut events = Vec::new();
        cache.replace_address("lan", "phone.local.", None, |p| events.push(p));
        assert_eq!(cache.peers().count(), 0);
        assert_eq!(events[0].fullname, "phone.local.");
        assert_eq!(events[0].present, Some(false));
        cache.apply(peer("other.local.", "192.0.2.2:1234"), now, |_| {});
        cache.apply(peer("bt:device", "bt:device"), now, |_| {});
        cache.clear(|p| events.push(p));
        assert_eq!(cache.peers().count(), 0);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|peer| peer.present == Some(false)));
        cache.clear(|_| panic!("duplicate withdrawal"));
    }
}
