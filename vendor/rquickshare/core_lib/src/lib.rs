#[macro_use]
extern crate log;
use anyhow::{anyhow, ensure, Result};
use base64::Engine;
use channel::{ChannelDirection, ChannelMessage};
use hdl::{MDnsDiscovery, MDnsServer};
use manager::{Manager, Route, Routes};
use parking_lot::RwLock;
use rand::{distr::Alphanumeric, Rng};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
#[cfg(target_os = "linux")]
mod bluetooth;
pub mod channel;
mod crypto;
mod hdl;
mod lan_upgrade;
mod manager;
mod network;
mod session;
mod storage;
pub(crate) mod transport;
mod utils;
pub use hdl::{EndpointInfo, OutboundPayload, State, Visibility};
pub use manager::SendInfo;
pub use utils::DeviceType;
pub mod sharing_nearby {
    include!(concat!(env!("OUT_DIR"), "/sharing.nearby.rs"));
}
pub mod securemessage {
    include!(concat!(env!("OUT_DIR"), "/securemessage.rs"));
}
pub mod securegcm {
    include!(concat!(env!("OUT_DIR"), "/securegcm.rs"));
}
pub mod location_nearby_connections {
    include!(concat!(env!("OUT_DIR"), "/location.nearby.connections.rs"));
}

#[derive(Debug, Clone)]
pub struct EngineOptions {
    pub bluetooth: bool,
    pub lan: bool,
    pub hotspot: bool,
}
impl Default for EngineOptions {
    fn default() -> Self {
        Self {
            bluetooth: true,
            lan: true,
            hotspot: true,
        }
    }
}
#[derive(Debug)]
pub struct RQS {
    tracker: Option<TaskTracker>,
    ctoken: Option<CancellationToken>,
    discovery_ctk: Option<CancellationToken>,
    visibility_sender: watch::Sender<Visibility>,
    visibility_receiver: watch::Receiver<Visibility>,
    port_number: Option<u32>,
    pub message_sender: broadcast::Sender<ChannelMessage>,
    options: EngineOptions,
    consent: Arc<AtomicBool>,
    download: Arc<RwLock<PathBuf>>,
    routes: Routes,
    #[cfg(target_os = "linux")]
    bluetooth: Option<Arc<bluetooth::Bluetooth>>,
}
impl Default for RQS {
    fn default() -> Self {
        Self::new(Visibility::Invisible, None, None)
    }
}
impl RQS {
    pub fn new(
        visibility: Visibility,
        port_number: Option<u32>,
        download_path: Option<PathBuf>,
    ) -> Self {
        Self::with_options(
            visibility,
            port_number,
            download_path,
            EngineOptions::default(),
        )
    }
    pub fn with_options(
        visibility: Visibility,
        port_number: Option<u32>,
        download_path: Option<PathBuf>,
        options: EngineOptions,
    ) -> Self {
        // Progress is capped at 10Hz per transfer; this bounded queue retains
        // consent/terminal events during normal UI scheduling stalls.
        let (message_sender, _) = broadcast::channel(4096);
        let (visibility_sender, visibility_receiver) = watch::channel(visibility);
        Self {
            tracker: None,
            ctoken: None,
            discovery_ctk: None,
            visibility_sender,
            visibility_receiver,
            port_number,
            message_sender,
            options,
            consent: Arc::new(AtomicBool::new(false)),
            download: Arc::new(RwLock::new(
                download_path.unwrap_or_else(utils::get_download_dir),
            )),
            routes: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(target_os = "linux")]
            bluetooth: None,
        }
    }
    pub fn set_network_consent(&self, allowed: bool) {
        self.consent.store(allowed, Ordering::Release);
    }
    fn background_error(events: &broadcast::Sender<ChannelMessage>, error: String) {
        let _ = events.send(ChannelMessage {
            direction: ChannelDirection::LibToFront,
            error: Some(error),
            ..Default::default()
        });
    }
    pub async fn run(&mut self) -> Result<mpsc::Sender<SendInfo>> {
        ensure!(self.tracker.is_none(), "engine is already running");
        ensure!(
            self.options.lan || self.options.bluetooth,
            "no session transport is enabled"
        );
        if let Err(error) = network::NetworkManager::recover().await {
            Self::background_error(
                &self.message_sender,
                format!("temporary network recovery: {error:#}"),
            );
        }
        let endpoint: [u8; 4] = {
            let mut rng = rand::rng();
            std::array::from_fn(|_| rng.sample(Alphanumeric))
        };
        let name = utils::get_hostname()?;
        let listener = if self.options.lan {
            Some(
                TcpListener::bind(("0.0.0.0", u16::try_from(self.port_number.unwrap_or(0))?))
                    .await?,
            )
        } else {
            None
        };
        let mdns = match &listener {
            Some(listener) => Some(MDnsServer::new(
                endpoint,
                listener.local_addr()?.port(),
                self.visibility_receiver.clone(),
                &name,
            )?),
            None => None,
        };
        let tracker = TaskTracker::new();
        let token = CancellationToken::new();
        let (incoming_sender, incoming_receiver) = mpsc::channel(16);
        #[cfg(target_os = "linux")]
        if self.options.bluetooth {
            match bluetooth::Bluetooth::new(
                endpoint,
                name.clone(),
                self.visibility_receiver.clone(),
                self.message_sender.clone(),
            )
            .await
            {
                Ok(bluetooth) => {
                    let bluetooth = Arc::new(bluetooth);
                    self.bluetooth = Some(bluetooth.clone());
                    let child = token.clone();
                    let events = self.message_sender.clone();
                    tracker.spawn(async move {
                        if let Err(error) = bluetooth.run(incoming_sender, child).await {
                            Self::background_error(
                                &events,
                                format!("Bluetooth transport: {error:#}"),
                            );
                        }
                    });
                }
                Err(error) => {
                    Self::background_error(
                        &self.message_sender,
                        format!("Bluetooth initialization: {error:#}"),
                    );
                    if !self.options.lan {
                        return Err(error);
                    }
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        if self.options.bluetooth {
            Self::background_error(
                &self.message_sender,
                "Bluetooth backend requires Linux BlueZ".into(),
            );
            if !self.options.lan {
                return Err(anyhow!("Bluetooth unavailable"));
            }
        }
        if let Some(mut mdns) = mdns {
            let child = token.clone();
            let events = self.message_sender.clone();
            tracker.spawn(async move {
                if let Err(error) = mdns.run(child).await {
                    Self::background_error(&events, format!("LAN advertisement: {error:#}"));
                }
            });
        }
        let (send_sender, send_receiver) = mpsc::channel(16);
        let lan_upgrade_host = match &listener {
            Some(listener) => Some(lan_upgrade::LanUpgradeHost::new(
                listener.local_addr()?.port(),
            )),
            None => None,
        };
        let manager = Manager {
            endpoint,
            name,
            events: self.message_sender.clone(),
            visibility: self.visibility_receiver.clone(),
            download: self.download.clone(),
            consent: self.consent.clone(),
            lan: self.options.lan,
            hotspot: self.options.hotspot,
            routes: self.routes.clone(),
            lan_upgrade_host,
            #[cfg(target_os = "linux")]
            bluetooth: self.bluetooth.clone(),
        };
        let child = token.clone();
        let events = self.message_sender.clone();
        tracker.spawn(async move {
            if let Err(error) = manager
                .run(listener, incoming_receiver, send_receiver, child)
                .await
            {
                Self::background_error(&events, format!("session manager: {error:#}"));
            }
        });
        self.tracker = Some(tracker);
        self.ctoken = Some(token);
        Ok(send_sender)
    }
    pub fn discovery(&mut self, sender: broadcast::Sender<EndpointInfo>) -> Result<()> {
        self.stop_discovery();
        let tracker = self
            .tracker
            .as_ref()
            .ok_or_else(|| anyhow!("engine must start before discovery"))?;
        let token = CancellationToken::new();
        let (proxy, _) = broadcast::channel(256);
        let mut receive = proxy.subscribe();
        let mdns = if self.options.lan {
            Some(MDnsDiscovery::new(proxy.clone())?)
        } else {
            None
        };
        self.discovery_ctk = Some(token.clone());
        if let Some(mdns) = mdns {
            let child = token.clone();
            let events = self.message_sender.clone();
            tracker.spawn(async move {
                if let Err(error) = mdns.run(child).await {
                    Self::background_error(&events, format!("LAN discovery: {error:#}"));
                }
            });
        }
        #[cfg(target_os = "linux")]
        if let Some(bluetooth) = &self.bluetooth {
            let bluetooth = bluetooth.clone();
            let child = token.clone();
            let events = self.message_sender.clone();
            tracker.spawn(async move {
                if let Err(error) = bluetooth.discover(proxy, child).await {
                    Self::background_error(&events, format!("Bluetooth discovery: {error:#}"));
                }
            });
        }
        let child = token.clone();
        let routes = self.routes.clone();
        let events = self.message_sender.clone();
        tracker.spawn(async move{
            let mut endpoints:HashMap<String,(String,String)>=HashMap::new();
            loop {tokio::select!{_=child.cancelled()=>break,message=receive.recv()=>match message{
                Ok(peer)=>{
                    if peer.present==Some(true){
                        if let Some(endpoint)=discovery_endpoint(&peer){endpoints.insert(peer.id.clone(),(endpoint,peer.transport.clone().unwrap_or_default()));}
                    }else{endpoints.remove(&peer.id);}
                    {let mut map=routes.write();map.clear();for (address,(endpoint,transport)) in &endpoints{
                        let preferred=if transport=="bluetooth"{endpoints.iter().find(|(_,value)|value.0==*endpoint && value.1=="lan").map(|(lan,_)|lan.clone())}else{None};
                        let fallback=if transport=="lan"{endpoints.iter().find(|(_,value)|value.0==*endpoint && value.1=="bluetooth").map(|(bt,_)|bt.clone())}else{None};
                        map.insert(address.clone(),Route{endpoint:endpoint.clone(),preferred,fallback});
                    }}
                    let _=sender.send(peer);
                }
                Err(broadcast::error::RecvError::Lagged(_))=>Self::background_error(&events,"discovery channel lagged; rescan required".into()),
                Err(broadcast::error::RecvError::Closed)=>break,
            }}}
        });
        Ok(())
    }
    pub fn stop_discovery(&mut self) {
        if let Some(token) = self.discovery_ctk.take() {
            token.cancel();
        }
        self.routes.write().clear();
    }
    pub fn change_visibility(&mut self, visibility: Visibility) {
        self.visibility_sender.send_replace(visibility);
    }
    pub async fn stop(&mut self) {
        self.stop_discovery();
        if let Some(token) = self.ctoken.take() {
            token.cancel();
        }
        if let Some(tracker) = self.tracker.take() {
            tracker.close();
            tracker.wait().await;
        }
        #[cfg(target_os = "linux")]
        {
            self.bluetooth = None;
        }
    }
    pub fn set_download_path(&self, path: Option<PathBuf>) {
        *self.download.write() = path.unwrap_or_else(utils::get_download_dir);
    }
}
fn discovery_endpoint(peer: &EndpointInfo) -> Option<String> {
    if let Some(endpoint) = peer.fullname.strip_prefix("endpoint:") {
        return (endpoint.len() == 4).then(|| endpoint.to_owned());
    }
    if peer.transport.as_deref() != Some("lan") {
        return None;
    }
    let record = peer.fullname.split('.').next()?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(record)
        .ok()?;
    if bytes.len() < 8 || bytes[0] & 0x1f != 3 || bytes[5..8] != [0xfc, 0x9f, 0x5e] {
        return None;
    }
    String::from_utf8(bytes[1..5].to_vec()).ok()
}
