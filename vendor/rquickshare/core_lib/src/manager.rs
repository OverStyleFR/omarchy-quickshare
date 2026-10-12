use crate::{
    channel::{ChannelAction, ChannelDirection, ChannelMessage, TransferType},
    hdl::{OutboundPayload, State, Visibility},
    session::Session,
    transport::TransportStream,
};
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{atomic::AtomicBool, Arc};
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, watch, Semaphore};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Debug)]
pub struct SendInfo {
    pub id: String,
    pub addr: String,
    pub ob: OutboundPayload,
    pub cancellation: CancellationToken,
}
#[derive(Debug, Clone)]
pub struct Route {
    pub endpoint: String,
    pub preferred: Option<String>,
    pub fallback: Option<String>,
}
pub type Routes = Arc<RwLock<HashMap<String, Route>>>;
#[derive(Clone)]
pub struct Manager {
    pub endpoint: [u8; 4],
    pub name: String,
    pub events: broadcast::Sender<ChannelMessage>,
    pub visibility: watch::Receiver<Visibility>,
    pub download: Arc<RwLock<PathBuf>>,
    pub consent: Arc<AtomicBool>,
    pub lan: bool,
    pub hotspot: bool,
    pub routes: Routes,
    #[cfg(target_os = "linux")]
    pub bluetooth: Option<Arc<crate::bluetooth::Bluetooth>>,
}
impl Manager {
    pub async fn run(
        self,
        listener: Option<TcpListener>,
        mut incoming: mpsc::Receiver<TransportStream>,
        mut sends: mpsc::Receiver<SendInfo>,
        ctk: CancellationToken,
    ) -> Result<()> {
        let sessions = TaskTracker::new();
        let slots = Arc::new(Semaphore::new(16));
        loop {
            enum Work {
                Stop,
                Send(SendInfo),
                Incoming(TransportStream, &'static str),
            }
            let work = tokio::select! {
                _=ctk.cancelled()=>Work::Stop,
                Some(send)=sends.recv()=>Work::Send(send),
                Some(stream)=incoming.recv()=>Work::Incoming(stream,"bluetooth"),
                connection=async {listener.as_ref().expect("guarded listener").accept().await},if listener.is_some()=>{
                    let (stream,_)=connection?;Work::Incoming(Box::new(stream),"lan")
                }
            };
            if matches!(&work, Work::Stop) {
                break;
            }
            let permit = match slots.clone().try_acquire_owned() {
                Ok(permit) => permit,
                Err(_) => {
                    if let Work::Send(send) = work {
                        self.failure(
                            &send.id,
                            true,
                            None,
                            "maximum active sessions reached".into(),
                        );
                    }
                    continue;
                }
            };
            match work {
                Work::Stop => break,
                Work::Incoming(stream, transport) => {
                    if *self.visibility.borrow() == Visibility::Invisible {
                        continue;
                    }
                    let manager = self.clone();
                    let token = ctk.child_token();
                    sessions.spawn(async move {
                        let _permit = permit;
                        let id = uuid::Uuid::new_v4().to_string();
                        let watcher = manager.watch_cancel(id.clone(), token.clone(), None);
                        manager
                            .session(id, stream, transport, None, None, token.clone())
                            .await;
                        token.cancel();
                        let _ = watcher.await;
                    });
                }
                Work::Send(send) => {
                    let manager = self.clone();
                    let token = ctk.child_token();
                    sessions.spawn(async move {
                        let _permit = permit;
                        manager.connect(send, token).await;
                    });
                }
            }
        }
        sessions.close();
        sessions.wait().await;
        Ok(())
    }
    fn failure(&self, id: &str, outbound: bool, transport: Option<String>, error: String) {
        let _ = self.events.send(ChannelMessage {
            id: id.into(),
            direction: ChannelDirection::LibToFront,
            rtype: Some(if outbound {
                TransferType::Outbound
            } else {
                TransferType::Inbound
            }),
            state: Some(State::Disconnected),
            transport,
            error: Some(error),
            ..Default::default()
        });
    }
    async fn connect(&self, send: SendInfo, ctk: CancellationToken) {
        if send.cancellation.is_cancelled() {
            ctk.cancel();
        }
        let watcher = self.watch_cancel(
            send.id.clone(),
            ctk.clone(),
            Some(send.cancellation.clone()),
        );
        let _ = self.events.send(ChannelMessage {
            id: send.id.clone(),
            direction: ChannelDirection::LibToFront,
            rtype: Some(TransferType::Outbound),
            state: Some(State::Initial),
            ..Default::default()
        });
        let route = self.routes.read().get(&send.addr).cloned();
        let (remote_endpoint, preferred, fallback) = match route {
            Some(route) => (Some(route.endpoint), route.preferred, route.fallback),
            None => (None, None, None),
        };
        let destinations = [preferred, Some(send.addr), fallback];
        let connection = async {
            let mut error = anyhow::anyhow!("no usable route");
            for route in destinations.into_iter().flatten() {
                if route.starts_with("bt:") {
                    #[cfg(target_os = "linux")]
                    if let Some(bluetooth) = &self.bluetooth {
                        match bluetooth.connect(&route).await {
                            Ok(stream) => return Ok((stream, "bluetooth")),
                            Err(e) => {
                                error = e;
                                continue;
                            }
                        }
                    }
                    error = anyhow::anyhow!("Bluetooth is unavailable");
                } else {
                    match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&route))
                        .await
                    {
                        Ok(Ok(stream)) => return Ok((Box::new(stream) as TransportStream, "lan")),
                        Ok(Err(e)) => error = e.into(),
                        Err(e) => error = e.into(),
                    }
                }
            }
            Err(error)
        };
        match tokio::select! {biased;_=send.cancellation.cancelled()=>{ctk.cancel();Err(anyhow::anyhow!("connection cancelled"))},_=ctk.cancelled()=>Err(anyhow::anyhow!("connection cancelled")),result=connection=>result}
        {
            Ok((stream, transport)) => {
                self.session(
                    send.id,
                    stream,
                    transport,
                    Some(send.ob),
                    remote_endpoint,
                    ctk.clone(),
                )
                .await
            }
            Err(error) => {
                if ctk.is_cancelled() {
                    let _ = self.events.send(ChannelMessage {
                        id: send.id,
                        direction: ChannelDirection::LibToFront,
                        rtype: Some(TransferType::Outbound),
                        state: Some(State::Cancelled),
                        ..Default::default()
                    });
                } else {
                    self.failure(&send.id, true, None, error.to_string());
                }
            }
        }
        ctk.cancel();
        let _ = watcher.await;
    }
    async fn session(
        &self,
        id: String,
        stream: TransportStream,
        transport: &str,
        outgoing: Option<OutboundPayload>,
        remote_endpoint: Option<String>,
        ctk: CancellationToken,
    ) {
        let download = self.download.clone();
        let outgoing_flag = outgoing.is_some();
        let mut session = match Session::new(
            id.clone(),
            stream,
            transport,
            self.endpoint,
            self.name.clone(),
            self.events.clone(),
            download,
            self.lan,
            self.hotspot,
            self.consent.clone(),
            outgoing,
            remote_endpoint,
        ) {
            Ok(session) => session,
            Err(error) => {
                self.failure(
                    &id,
                    outgoing_flag,
                    Some(transport.into()),
                    error.to_string(),
                );
                return;
            }
        };
        let _ = session.run(ctk).await;
    }
    fn watch_cancel(
        &self,
        id: String,
        token: CancellationToken,
        retained: Option<CancellationToken>,
    ) -> tokio::task::JoinHandle<()> {
        let mut messages = self.events.subscribe();
        tokio::spawn(async move {
            loop {
                tokio::select! {_=token.cancelled()=>break,_=async{retained.as_ref().expect("guarded retained cancellation").cancelled().await},if retained.is_some()=>{token.cancel();break;},message=messages.recv()=>match message{
                    Ok(message) if message.direction==ChannelDirection::FrontToLib && message.id==id && message.action==Some(ChannelAction::CancelTransfer)=>{token.cancel();break;},
                    Err(broadcast::error::RecvError::Closed)=>break,
                    Err(broadcast::error::RecvError::Lagged(_))=>{token.cancel();break;},_=>{}
                }}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    struct Source(PathBuf);
    impl Drop for Source {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelling_a_queued_send_never_opens_a_peer_connection() {
        let source = Source(
            std::env::temp_dir().join(format!("quickshare-queue-test-{}", uuid::Uuid::new_v4())),
        );
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source.0)
            .unwrap()
            .write_all(b"Do not send after cancellation")
            .unwrap();
        let peer = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        peer.set_nonblocking(true).unwrap();
        let (events, mut receive) = broadcast::channel(64);
        let (_, visibility) = watch::channel(Visibility::Invisible);
        let manager = Manager {
            endpoint: *b"TEST",
            name: "test".into(),
            events,
            visibility,
            download: Arc::new(RwLock::new(std::env::temp_dir())),
            consent: Arc::new(AtomicBool::new(false)),
            lan: true,
            hotspot: false,
            routes: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(target_os = "linux")]
            bluetooth: None,
        };
        let (outgoing, sends) = mpsc::channel(1);
        let (_incoming, incoming) = mpsc::channel(1);
        let cancellation = CancellationToken::new();
        outgoing
            .try_send(SendInfo {
                id: "queued".into(),
                addr: peer.local_addr().unwrap().to_string(),
                ob: OutboundPayload::Files(vec![source.0.to_str().unwrap().into()]),
                cancellation: cancellation.clone(),
            })
            .unwrap();
        cancellation.cancel();
        let stop = CancellationToken::new();
        let worker = tokio::spawn(manager.run(None, incoming, sends, stop.clone()));
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let event = receive.recv().await.unwrap();
                if event.id == "queued" && event.state == Some(State::Cancelled) {
                    break;
                }
            }
        })
        .await
        .expect("queued cancellation must reach a terminal state");
        stop.cancel();
        worker.await.unwrap().unwrap();
        assert_eq!(
            peer.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock,
            "a cancelled queued transfer must not contact the peer"
        );
    }

    #[tokio::test]
    async fn routes_prefer_available_network_and_fall_back_after_connection_failure() {
        let source = Source(
            std::env::temp_dir().join(format!("quickshare-route-test-{}", uuid::Uuid::new_v4())),
        );
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&source.0)
            .unwrap()
            .write_all(b"Route selection still requires file consent")
            .unwrap();
        for prefer in [true, false] {
            let original = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            original.set_nonblocking(true).unwrap();
            let address = original.local_addr().unwrap().to_string();
            let candidate = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let candidate_address = candidate.local_addr().unwrap().to_string();
            let (events, _) = broadcast::channel(64);
            let (_, visibility) = watch::channel(Visibility::Invisible);
            let routes = Arc::new(RwLock::new(HashMap::from([(
                address.clone(),
                Route {
                    endpoint: "PEER".into(),
                    preferred: prefer.then(|| candidate_address.clone()),
                    fallback: (!prefer).then_some(candidate_address),
                },
            )])));
            let manager = Manager {
                endpoint: *b"TEST",
                name: "test".into(),
                events,
                visibility,
                download: Arc::new(RwLock::new(std::env::temp_dir())),
                consent: Arc::new(AtomicBool::new(false)),
                lan: true,
                hotspot: false,
                routes,
                #[cfg(target_os = "linux")]
                bluetooth: None,
            };
            let original = prefer.then_some(original); // closed selected route in fallback case
            let stop = CancellationToken::new();
            let worker_token = stop.clone();
            let path = source.0.to_str().unwrap().to_owned();
            let worker = tokio::spawn(async move {
                manager
                    .connect(
                        SendInfo {
                            id: "route".into(),
                            addr: address,
                            ob: OutboundPayload::Files(vec![path]),
                            cancellation: CancellationToken::new(),
                        },
                        worker_token,
                    )
                    .await;
            });
            let connection = tokio::time::timeout(Duration::from_secs(2), candidate.accept())
                .await
                .expect("must use the preferred available route or the backup after failure")
                .unwrap();
            stop.cancel();
            drop(connection);
            worker.await.unwrap();
            if let Some(original) = original {
                assert_eq!(
                    original.accept().unwrap_err().kind(),
                    std::io::ErrorKind::WouldBlock,
                    "the selected route must not be used while the preferred network is available"
                );
            }
        }
    }
}
