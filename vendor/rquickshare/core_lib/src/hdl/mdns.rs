use std::time::Duration;

use mdns_sd::{AddrType, ServiceDaemon, ServiceInfo};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::utils::{gen_mdns_endpoint_info, gen_mdns_name, DeviceType};

const INNER_NAME: &str = "MDnsServer";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visibility {
    Visible = 0,
    Invisible = 1,
}

pub struct MDnsServer {
    daemon: ServiceDaemon,
    service_info: ServiceInfo,
    visibility_receiver: watch::Receiver<Visibility>,
}

impl MDnsServer {
    pub fn new(
        endpoint_id: [u8; 4],
        service_port: u16,
        visibility_receiver: watch::Receiver<Visibility>,
        hostname: &str,
    ) -> Result<Self, anyhow::Error> {
        let service_info =
            Self::build_service(endpoint_id, service_port, DeviceType::Laptop, hostname)?;

        Ok(Self {
            daemon: ServiceDaemon::new()?,
            service_info,
            visibility_receiver,
        })
    }

    pub async fn run(&mut self, ctk: CancellationToken) -> Result<(), anyhow::Error> {
        info!("{INNER_NAME}: service starting");
        let monitor = self.daemon.monitor()?;
        let mut visibility = *self.visibility_receiver.borrow();
        if visibility != Visibility::Invisible {
            self.daemon.register(self.service_info.clone())?;
        }

        loop {
            tokio::select! {
                _ = ctk.cancelled() => {
                    info!("{INNER_NAME}: tracker cancelled, breaking");
                    break;
                }
                r = monitor.recv_async() => {
                    match r {
                        Ok(_) => continue,
                        Err(err) => return Err(err.into()),
                    }
                },
                _ = self.visibility_receiver.changed() => {
                    visibility = *self.visibility_receiver.borrow_and_update();

                    debug!("{INNER_NAME}: visibility changed: {visibility:?}");
                    if visibility == Visibility::Visible {
                        self.daemon.register(self.service_info.clone())?;
                    } else if visibility == Visibility::Invisible {
                        let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
                        let _ = tokio::time::timeout(Duration::from_secs(3), receiver.recv_async()).await;
                    }
                }
            }
        }

        // Unregister the mDNS service - we're shutting down
        let receiver = self.daemon.unregister(self.service_info.get_fullname())?;
        if let Ok(Ok(event)) =
            tokio::time::timeout(Duration::from_secs(3), receiver.recv_async()).await
        {
            info!("MDnsServer: service unregistered: {:?}", &event);
        }

        let _ = self.daemon.shutdown();
        Ok(())
    }

    fn build_service(
        endpoint_id: [u8; 4],
        service_port: u16,
        device_type: DeviceType,
        hostname: &str,
    ) -> Result<ServiceInfo, anyhow::Error> {
        let name = gen_mdns_name(endpoint_id);
        info!("Broadcasting with: {hostname}");
        let endpoint_info = gen_mdns_endpoint_info(device_type as u8, hostname);

        let properties = [("n", endpoint_info)];
        let si = ServiceInfo::new(
            "_FC9F5ED42C8A._tcp.local.",
            &name,
            hostname,
            "",
            service_port,
            &properties[..],
        )?
        .enable_addr_auto(AddrType::V4);

        Ok(si)
    }
}
