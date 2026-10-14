use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::peer_cache::{PeerCache, LAN_PEER_TTL};
use crate::utils::{is_not_self_ip, parse_mdns_endpoint_info};
use crate::DeviceType;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct EndpointInfo {
    pub fullname: String,
    pub id: String,
    pub name: Option<String>,
    pub ip: Option<String>,
    pub port: Option<String>,
    pub rtype: Option<DeviceType>,
    pub present: Option<bool>,
    #[serde(default)]
    pub transport: Option<String>,
}

pub struct MDnsDiscovery {
    daemon: ServiceDaemon,
    sender: broadcast::Sender<EndpointInfo>,
}

impl MDnsDiscovery {
    pub fn new(sender: broadcast::Sender<EndpointInfo>) -> Result<Self, anyhow::Error> {
        let daemon = ServiceDaemon::new()?;

        Ok(Self { daemon, sender })
    }

    pub async fn run(mut self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!("MDnsDiscovery: service starting");

        let service_type = "_FC9F5ED42C8A._tcp.local.";
        let mut cache = PeerCache::default();
        let mut receiver = None;
        let result = async {
            receiver = Some(self.daemon.browse(service_type)?);
            let mut expiry = tokio::time::interval(Duration::from_secs(1));
            expiry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            // The pinned mdns-sd suppresses ServiceResolved for unchanged
            // answers even when it renews their DNS TTL. Start a fresh browse
            // before our lease expires, rather than renewing from its stale
            // cached snapshot. This also retires the daemon's old DNS cache.
            let refresh_period = LAN_PEER_TTL / 2;
            let mut refresh = tokio::time::interval_at(
                tokio::time::Instant::now() + refresh_period,
                refresh_period,
            );
            refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = ctk.cancelled() => {
                        info!("MDnsDiscovery: tracker cancelled, breaking");
                        break;
                    }
                    _ = expiry.tick() => {
                        cache.expire(Instant::now(), |peer| { let _ = self.sender.send(peer); });
                    }
                    _ = refresh.tick() => {
                        self.daemon.stop_browse(service_type)?;
                        let stopped = self.daemon.shutdown()?;
                        let mut events_open = true;
                        // Drain and discard this generation until shutdown is
                        // acknowledged. Its late removals cannot reach a new
                        // generation, and its bounded channel cannot block exit.
                        tokio::select! {
                            _ = ctk.cancelled() => break,
                            result = tokio::time::timeout(Duration::from_secs(3), async {
                                loop {
                                    tokio::select! {
                                        result = stopped.recv_async() => break result,
                                        event = receiver.as_ref().expect("active browse").recv_async(), if events_open => {
                                            events_open = event.is_ok();
                                        }
                                        _ = expiry.tick() => {
                                            cache.expire(Instant::now(), |peer| { let _ = self.sender.send(peer); });
                                        }
                                    }
                                }
                            }) => { result??; }
                        }
                        self.daemon = ServiceDaemon::new()?;
                        receiver = Some(self.daemon.browse(service_type)?);
                    }
                    event = receiver.as_ref().expect("active browse").recv_async() => {
                        match event {
                            Ok(ServiceEvent::ServiceResolved(info)) => {
                                let fullname = info.get_fullname();
                                let endpoint = resolved_endpoint(&info);
                                // Retire the old route before probing a changed
                                // address, including invalid/no-IPv4 metadata.
                                cache.replace_address(
                                    "lan", fullname, endpoint.as_ref().map(|peer| peer.id.as_str()),
                                    |peer| { let _ = self.sender.send(peer); },
                                );
                                let Some(endpoint) = endpoint else { continue; };
                                let address = endpoint.id.parse::<std::net::SocketAddr>()
                                    .expect("resolved IPv4 endpoint");
                                let probe = tokio::time::timeout(
                                    Duration::from_secs(3), TcpStream::connect(address),
                                );
                                tokio::pin!(probe);
                                loop {
                                    tokio::select! {
                                        _ = ctk.cancelled() => break,
                                        _ = expiry.tick() => {
                                            cache.expire(Instant::now(), |peer| { let _ = self.sender.send(peer); });
                                        }
                                        result = &mut probe => {
                                            if matches!(result, Ok(Ok(_))) {
                                                cache.apply(endpoint, Instant::now(), |peer| { let _ = self.sender.send(peer); });
                                            } else {
                                                cache.replace_address("lan", fullname, None, |peer| { let _ = self.sender.send(peer); });
                                            }
                                            break;
                                        }
                                    }
                                }
                                if ctk.is_cancelled() { break; }
                            }
                            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                                cache.replace_address("lan", &fullname, None, |peer| { let _ = self.sender.send(peer); });
                            }
                            Ok(_) => {}
                            Err(error) => {
                                error!("MDnsDiscovery: receiver closed: {error}");
                                break;
                            }
                        }
                    }
                }
            }
            Ok(())
        }.await;

        let _ = self.daemon.stop_browse(service_type);
        if let Ok(stopped) = self.daemon.shutdown() {
            // Cleanup also drains without applying old-generation events.
            let mut events_open = receiver.is_some();
            let _ = tokio::time::timeout(Duration::from_secs(3), async {
                loop {
                    tokio::select! {
                        _ = stopped.recv_async() => break,
                        event = async {
                            receiver.as_ref().expect("active browse").recv_async().await
                        }, if events_open => {
                            events_open = event.is_ok();
                        }
                    }
                }
            }).await;
        }
        cache.clear(|peer| { let _ = self.sender.send(peer); });
        result
    }
}

fn resolved_endpoint(info: &mdns_sd::ServiceInfo) -> Option<EndpointInfo> {
    // DNS names occupy at most 255 wire octets including the root byte.
    // mdns-sd renders each label with a trailing dot but does not check this
    // overall bound while decoding remote names.
    let fullname = info.get_fullname();
    if fullname.is_empty() || fullname.len() > 254 {
        return None;
    }
    let addresses = info.get_addresses_v4();
    let ip = addresses.iter().filter(|ip| is_not_self_ip(ip)).min()?;
    let (device_type, name) = parse_mdns_endpoint_info(info.get_property("n")?.val_str()).ok()?;
    let port = info.get_port();
    Some(EndpointInfo {
        fullname: fullname.to_owned(),
        id: format!("{ip}:{port}"),
        name: Some(name),
        ip: Some(ip.to_string()),
        port: Some(port.to_string()),
        rtype: Some(device_type),
        present: Some(true),
        transport: Some("lan".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::gen_mdns_endpoint_info;

    fn service(instance: &str, ip: &str, port: u16, metadata: &str) -> mdns_sd::ServiceInfo {
        mdns_sd::ServiceInfo::new(
            "_FC9F5ED42C8A._tcp.local.", instance, "host.local.", ip, port,
            &[("n", metadata)][..],
        ).unwrap()
    }

    #[test]
    fn resolution_selects_a_stable_ipv4() {
        let metadata = gen_mdns_endpoint_info(1, &"p".repeat(256));
        let info = service("phone", "192.0.2.2,192.0.2.1", 1234, &metadata);
        let peer = resolved_endpoint(&info).unwrap();
        assert_eq!(peer.id, "192.0.2.1:1234");
    }

    #[test]
    fn invalid_metadata_and_ipv6_only_updates_withdraw_previous_route() {
        let metadata = gen_mdns_endpoint_info(1, "Phone");
        let original = service("phone", "192.0.2.1", 1234, &metadata);
        for updated in [
            service("phone", "192.0.2.2", 5678, "invalid"),
            service("phone", "2001:db8::1", 5678, &metadata),
        ] {
            let mut cache = PeerCache::default();
            cache.apply(resolved_endpoint(&original).unwrap(), Instant::now(), |_| {});
            let replacement = resolved_endpoint(&updated);
            assert!(replacement.is_none());
            let mut events = Vec::new();
            cache.replace_address(
                "lan", updated.get_fullname(),
                replacement.as_ref().map(|peer| peer.id.as_str()),
                |peer| events.push(peer),
            );
            assert_eq!(cache.peers().count(), 0);
            assert_eq!(events.len(), 1);
            assert_eq!(events[0].id, "192.0.2.1:1234");
            assert_eq!(events[0].fullname, original.get_fullname());
            assert_eq!(events[0].present, Some(false));
        }
    }

    #[test]
    fn oversized_decoded_dns_name_is_not_retained() {
        let metadata = gen_mdns_endpoint_info(1, "Phone");
        let info = service(&"p".repeat(255), "192.0.2.1", 1234, &metadata);
        assert!(info.get_fullname().len() > 254);
        assert!(resolved_endpoint(&info).is_none());
    }
}
