//! Routes plaintext LAN introductions to their already-existing transfer.
//! Endpoint IDs identify an admission slot, not an authenticated peer.
use crate::location_nearby_connections::bandwidth_upgrade_negotiation_frame::ClientIntroduction;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

#[derive(Debug)]
pub(crate) struct LanCandidate {
    pub stream: TcpStream,
    pub introduction: ClientIntroduction,
}

#[derive(Clone)]
pub(crate) struct LanUpgradeHost {
    port: u16,
    admissions: Arc<Mutex<HashMap<[u8; 4], mpsc::Sender<LanCandidate>>>>,
}

impl LanUpgradeHost {
    pub(crate) fn new(port: u16) -> Self {
        Self {
            port,
            admissions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    pub(crate) fn register(&self, endpoint: &str) -> Result<LanUpgradeRegistration> {
        let endpoint = endpoint_key(endpoint)?;
        let mut admissions = self.admissions.lock();
        let Entry::Vacant(entry) = admissions.entry(endpoint) else {
            return Err(anyhow!("LAN upgrade endpoint already registered"));
        };
        let (sender, receiver) = mpsc::channel(1);
        entry.insert(sender.clone());
        Ok(LanUpgradeRegistration {
            host: self.clone(),
            endpoint,
            sender,
            receiver,
        })
    }

    pub(crate) fn route(&self, candidate: LanCandidate) -> Result<()> {
        let endpoint = endpoint_key(candidate.introduction.endpoint_id())?;
        let admissions = self.admissions.lock();
        let sender = admissions
            .get(&endpoint)
            .ok_or_else(|| anyhow!("LAN upgrade endpoint is not registered"))?;
        // Hold the registry lock through try_send so closing admission cannot
        // race a sender cloned from an entry that has already been removed.
        sender.try_send(candidate).map_err(|error| match error {
            mpsc::error::TrySendError::Full(_) => anyhow!("LAN upgrade admission is full"),
            mpsc::error::TrySendError::Closed(_) => anyhow!("LAN upgrade admission is closed"),
        })
    }
}

fn endpoint_key(endpoint: &str) -> Result<[u8; 4]> {
    endpoint
        .as_bytes()
        .try_into()
        .map_err(|_| anyhow!("invalid LAN upgrade endpoint"))
}

pub(crate) struct LanUpgradeRegistration {
    host: LanUpgradeHost,
    endpoint: [u8; 4],
    sender: mpsc::Sender<LanCandidate>,
    receiver: mpsc::Receiver<LanCandidate>,
}

impl LanUpgradeRegistration {
    pub(crate) async fn recv(&mut self) -> Option<LanCandidate> {
        self.receiver.recv().await
    }

    pub(crate) fn close_admission(&mut self) {
        let mut admissions = self.host.admissions.lock();
        if let Entry::Occupied(entry) = admissions.entry(self.endpoint) {
            if entry.get().same_channel(&self.sender) {
                entry.remove();
            }
        }
        self.receiver.close();
        // A selected candidate belongs to the caller; discard only candidates
        // still waiting in this registration after the caller freezes admission.
        while self.receiver.try_recv().is_ok() {}
    }
}

impl Drop for LanUpgradeRegistration {
    fn drop(&mut self) {
        self.close_admission();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn candidate(endpoint: &str) -> (LanCandidate, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (stream, peer) = tokio::join!(
            TcpStream::connect(listener.local_addr().unwrap()),
            listener.accept()
        );
        (
            LanCandidate {
                stream: stream.unwrap(),
                introduction: ClientIntroduction {
                    endpoint_id: Some(endpoint.into()),
                    supports_disabling_encryption: Some(false),
                },
            },
            peer.unwrap().0,
        )
    }

    async fn assert_closed(peer: &mut TcpStream) {
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), peer.read(&mut byte))
                .await
                .expect("rejected candidate must be dropped")
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn registrations_route_only_the_exact_endpoint() {
        let host = LanUpgradeHost::new(47593);
        let mut first = host.register("ONE1").unwrap();
        let mut second = host.register("TWO2").unwrap();
        let (first_routed, first_peer) = candidate("ONE1").await;
        host.route(first_routed).unwrap();
        assert!(matches!(
            second.receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Empty)
        ));
        let first_candidate = first.recv().await.unwrap();
        assert_eq!(first_candidate.introduction.endpoint_id(), "ONE1");
        assert_eq!(
            first_candidate.stream.peer_addr().unwrap(),
            first_peer.local_addr().unwrap()
        );
        let (second_routed, second_peer) = candidate("TWO2").await;
        host.route(second_routed).unwrap();
        let second_candidate = second.recv().await.unwrap();
        assert_eq!(second_candidate.introduction.endpoint_id(), "TWO2");
        assert_eq!(
            second_candidate.stream.peer_addr().unwrap(),
            second_peer.local_addr().unwrap()
        );
        let (unknown, mut unknown_peer) = candidate("NONE").await;
        assert!(host.route(unknown).is_err());
        assert_closed(&mut unknown_peer).await;
        assert!(host.register("short").is_err());
    }

    #[tokio::test]
    async fn duplicate_registration_and_stale_drop_cannot_choose_another_session() {
        let host = LanUpgradeHost::new(47593);
        let mut original = host.register("PEER").unwrap();
        assert!(host.register("PEER").is_err());
        original.close_admission();
        let mut replacement = host.register("PEER").unwrap();
        original.close_admission();
        drop(original);
        assert!(host.register("PEER").is_err());
        let (candidate, peer) = candidate("PEER").await;
        host.route(candidate).unwrap();
        assert_eq!(
            replacement.recv().await.unwrap().stream.peer_addr().unwrap(),
            peer.local_addr().unwrap()
        );
        drop(replacement);
        assert!(host.register("PEER").is_ok());
    }

    #[tokio::test]
    async fn backpressure_rejects_excess_candidates_without_freezing_admission() {
        let host = LanUpgradeHost::new(47593);
        let mut registration = host.register("PEER").unwrap();
        let (first, _first_peer) = candidate("PEER").await;
        host.route(first).unwrap();
        let (excess, mut excess_peer) = candidate("PEER").await;
        assert!(host.route(excess).is_err());
        assert_closed(&mut excess_peer).await;
        drop(registration.recv().await.unwrap());
        let (retry, retry_peer) = candidate("PEER").await;
        host.route(retry).unwrap();
        assert_eq!(
            registration.recv().await.unwrap().stream.peer_addr().unwrap(),
            retry_peer.local_addr().unwrap()
        );
    }

    #[tokio::test]
    async fn freezing_admission_preserves_only_the_selected_candidate() {
        let host = LanUpgradeHost::new(47593);
        let mut registration = host.register("PEER").unwrap();
        let (first, mut first_peer) = candidate("PEER").await;
        host.route(first).unwrap();
        let mut selected = registration.recv().await.unwrap();
        let (queued, mut queued_peer) = candidate("PEER").await;
        host.route(queued).unwrap();
        registration.close_admission();
        assert!(registration.recv().await.is_none());
        assert_closed(&mut queued_peer).await;
        let (late, mut late_peer) = candidate("PEER").await;
        assert!(host.route(late).is_err());
        assert_closed(&mut late_peer).await;
        first_peer.write_all(b"selected").await.unwrap();
        let mut bytes = [0; 8];
        selected.stream.read_exact(&mut bytes).await.unwrap();
        assert_eq!(&bytes, b"selected");
    }

    #[tokio::test]
    async fn a_closed_receiver_rejects_routing_until_its_owner_releases_the_entry() {
        let host = LanUpgradeHost::new(47593);
        let mut registration = host.register("PEER").unwrap();
        registration.receiver.close();
        let (candidate, mut peer) = candidate("PEER").await;
        assert!(host.route(candidate).is_err());
        assert_closed(&mut peer).await;
        assert!(host.register("PEER").is_err());
        registration.close_admission();
        assert!(host.register("PEER").is_ok());
    }
}
