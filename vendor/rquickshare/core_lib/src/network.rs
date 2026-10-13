// Original NetworkManager backend for rquickshare; distributed under the
// repository's GPLv3 license (../../LICENSE).
// Protocol context: Google Nearby's Apache-2.0 WifiHotspotBwuHandler, revision
// d128b5fca30b3fa8d4642f8c75dd9e314e090e74. No upstream implementation is copied.
// nmcli command/property reference: https://networkmanager.dev/docs/api/latest/nmcli.html

//! Ephemeral WPA2/IPv4 leases, independent of BWU framing. Callers must keep
//! credentials on the authenticated, encrypted session and explicitly close a
//! lease before shutting down their runtime. No radio is enabled by this module.
//! AP/STA concurrency and Wi-Fi Direct are deliberately not implemented. A
//! lease requires consent if any Wi-Fi interface is already active, and only
//! replaces the selected interface's connection. User changes made during a
//! lease take precedence over automatic restoration.

use std::{
    fmt, io,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, LazyLock},
    time::Duration,
};

use anyhow::{anyhow, bail, ensure, Context, Result};
#[cfg(unix)]
use std::os::{
    fd::{AsRawFd, FromRawFd, OwnedFd, RawFd},
    unix::fs::{MetadataExt, PermissionsExt},
};
use tokio::{
    io::AsyncWriteExt,
    process::Command,
    sync::{Mutex, OwnedMutexGuard},
    time::timeout,
};
use uuid::Uuid;

const PROFILE_PREFIX: &str = "omarchy-quickshare-";
const PROFILE_MARKER: &str = "omarchy-quickshare-ephemeral-v1";
const QUERY_WAIT: u32 = 10;
const ACTIVATION_WAIT: u32 = 35;
static LEASE_LOCK: LazyLock<Arc<Mutex<()>>> = LazyLock::new(|| Arc::new(Mutex::new(())));

type Permit = Arc<OwnedMutexGuard<()>>;

#[derive(Clone, Debug)]
pub struct NetworkManager;

/// A currently activated physical LAN address, not a lease or permission to
/// change networking. Callers must re-query before committing an upgrade.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanInterface {
    pub interface: String,
    pub address: Ipv4Addr,
    pub netmask: Ipv4Addr,
}

/// Distinguish failed activation rollback from a harmless cancelled operation.
/// Sessions must never turn this failure into a successful Cancelled terminal.
#[derive(Debug)]
pub(crate) struct RestorationFailure(anyhow::Error);
impl fmt::Display for RestorationFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "network restoration failed: {}", self.0)
    }
}
impl std::error::Error for RestorationFailure {}

/// A real active NetworkManager connection. Public credentials are solely for
/// encrypted BWU negotiation; neither Debug nor command errors include them.
pub struct NetworkLease {
    pub gateway: IpAddr,
    pub interface: String,
    pub ssid: String,
    pub password: String,
    /// Actual active-channel center frequency in MHz, not a requested band.
    pub frequency: i32,
    cleanup: CleanupGuard,
}

impl fmt::Debug for NetworkLease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetworkLease")
            .field("gateway", &self.gateway)
            .field("interface", &self.interface)
            .field("ssid", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .field("frequency", &self.frequency)
            .field("closed", &self.cleanup.state.is_none())
            .finish()
    }
}

impl NetworkLease {
    /// Restore the previous active UUID unless the user has activated another
    /// connection, then delete only our in-memory profile and recovery journal.
    /// A session must await this operation even after its cancellation token
    /// fires; cancellation of a close future is only a crash-safety fallback.
    /// Safe to repeat, including after an error or cancellation of a close.
    pub async fn close(&mut self) -> Result<()> {
        self.cleanup.close().await
    }
}

impl NetworkManager {
    /// Read-only, bounded snapshot of existing physical Wi-Fi/Ethernet LANs.
    /// Wi-Fi sorts before Ethernet, then interface and IPv4 address; at most
    /// eight exact NM/kernel address matches are returned. No radio, profile,
    /// lease lock, consent setting or mutation permission is involved.
    pub async fn lan_interfaces(
        ctk: &tokio_util::sync::CancellationToken,
    ) -> Result<Vec<LanInterface>> {
        ensure!(!ctk.is_cancelled(), "LAN discovery cancelled");
        if !cfg!(target_os = "linux") {
            return Ok(Vec::new());
        }
        tokio::select! {
            biased;
            _ = ctk.cancelled() => bail!("LAN discovery cancelled"),
            result = timeout(
                Duration::from_secs(u64::from(QUERY_WAIT) + 5),
                query_lan_interfaces(),
            ) => result.context("LAN discovery exceeded its bounded deadline")?,
        }
    }

    /// Probe daemon/radio state, managed Wi-Fi, AP/WPA2/CCMP capability and
    /// non-interactive Polkit permissions. This does not mutate network state.
    /// An occupied interface is available only subject to create/join consent.
    pub async fn available() -> Result<bool> {
        if !cfg!(target_os = "linux") {
            return Ok(false);
        }
        let Ok(_permit) = LEASE_LOCK.clone().try_lock_owned() else {
            return Ok(false);
        };
        current_owner().await?;
        let devices = match probe(true).await {
            Ok(devices) => devices,
            Err(error) if is_unavailable(&error) => return Ok(false),
            Err(error) => return Err(error),
        };
        Ok(devices.iter().any(|device| device.suitable(true, None)))
    }

    /// Recover only positively owned, dead-process leases at application
    /// startup, even when hotspot negotiation is disabled. This does not need
    /// AP capability and never enables a disabled radio or networking.
    /// Empty recovery state does not require NetworkManager mutation permissions.
    pub async fn recover() -> Result<()> {
        if !cfg!(target_os = "linux") {
            return Ok(());
        }
        let permit = acquire_permit()?;
        let owner = current_owner().await?;
        recover_journals(&owner, &permit).await?;
        reclaim_stale(&owner, &permit).await
    }

    /// Cancellation is cooperative between bounded nmcli calls. Never drop this
    /// future: activation errors/cancellation await rollback before returning.
    pub async fn create(
        consent: bool,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<NetworkLease> {
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        let permit = acquire_permit()?;
        let owner = current_owner().await?;
        ensure_permissions(false).await?;
        recover_journals(&owner, &permit).await?;
        reclaim_stale(&owner, &permit).await?;
        let devices = probe(true).await?;
        require_consent(
            devices.iter().any(|device| device.active.is_some()),
            consent,
        )?;
        let device = select_device(&devices, true, None)?;
        refuse_other_lease(device).await?;
        let band = if device.band_2ghz != "no" { "bg" } else { "a" };
        let ssid = format!("QuickShare-{}", hex::encode(rand::random::<[u8; 8]>()));
        let password = hex::encode(rand::random::<[u8; 16]>());
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        activate(
            device,
            owner,
            permit,
            ssid,
            password,
            band,
            None,
            cancellation,
        )
        .await
    }

    pub async fn join(
        ssid: &str,
        password: &str,
        frequency: i32,
        consent: bool,
        cancellation: &tokio_util::sync::CancellationToken,
    ) -> Result<NetworkLease> {
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        let (band, channel) = validate_credentials(ssid, password, frequency)?;
        let permit = acquire_permit()?;
        let owner = current_owner().await?;
        ensure_permissions(false).await?;
        recover_journals(&owner, &permit).await?;
        reclaim_stale(&owner, &permit).await?;
        let devices = probe(false).await?;
        require_consent(
            devices.iter().any(|device| device.active.is_some()),
            consent,
        )?;
        let device = select_device(&devices, false, Some(band))?;
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        refuse_other_lease(device).await?;
        activate(
            device,
            owner,
            permit,
            ssid.to_owned(),
            password.to_owned(),
            band,
            Some((channel, frequency)),
            cancellation,
        )
        .await
    }
}

fn acquire_permit() -> Result<Permit> {
    ensure!(
        cfg!(target_os = "linux"),
        "NetworkManager leases require Linux"
    );
    LEASE_LOCK
        .clone()
        .try_lock_owned()
        .map(Arc::new)
        .map_err(|_| anyhow!("a Quick Share network lease or its cleanup is already running"))
}

fn require_consent(active_wifi: bool, consent: bool) -> Result<()> {
    ensure!(
        !active_wifi || consent,
        "Quick Share may interrupt active Wi-Fi; explicit network-change consent is required"
    );
    if active_wifi {
        log::warn!(
            "Quick Share is changing Wi-Fi with consent; the previous connection will be restored"
        );
    }
    Ok(())
}

fn validate_credentials(ssid: &str, password: &str, frequency: i32) -> Result<(&'static str, u32)> {
    ensure!(
        !ssid.is_empty() && ssid.len() <= 32 && !ssid.chars().any(char::is_control),
        "hotspot SSID must contain 1 to 32 bytes and no control characters"
    );
    let passphrase = (8..=63).contains(&password.len())
        && password.bytes().all(|byte| (b' '..=b'~').contains(&byte));
    let raw_psk = password.len() == 64 && password.bytes().all(|byte| byte.is_ascii_hexdigit());
    ensure!(passphrase || raw_psk, "hotspot password must be an 8 to 63 byte ASCII WPA2 passphrase or a 64 digit hexadecimal PSK");
    frequency_channel(frequency)
}

fn frequency_channel(frequency: i32) -> Result<(&'static str, u32)> {
    if (2412..=2472).contains(&frequency) && (frequency - 2407) % 5 == 0 {
        return Ok(("bg", ((frequency - 2407) / 5) as u32));
    }
    if frequency == 2484 {
        return Ok(("bg", 14));
    }
    const CHANNELS_5GHZ: &[i32] = &[
        36, 40, 44, 48, 52, 56, 60, 64, 100, 104, 108, 112, 116, 120, 124, 128, 132, 136, 140, 144,
        149, 153, 157, 161, 165, 169, 173, 177,
    ];
    if frequency > 5000 && (frequency - 5000) % 5 == 0 {
        let channel = (frequency - 5000) / 5;
        if CHANNELS_5GHZ.contains(&channel) {
            return Ok(("a", channel as u32));
        }
    }
    bail!("unsupported hotspot frequency: only valid 2.4 GHz and 5 GHz WPA2 channels are supported")
}

struct Device {
    interface: String,
    active: Option<String>,
    state: u32,
    managed: bool,
    ap: bool,
    wpa2: bool,
    ccmp: bool,
    band_2ghz: String,
    band_5ghz: String,
}

impl Device {
    fn suitable(&self, ap: bool, band: Option<&str>) -> bool {
        self.managed
            && matches!(self.state, 30 | 100)
            && self.wpa2
            && self.ccmp
            && (!ap || self.ap)
            && match band {
                Some("bg") => self.band_2ghz != "no",
                Some("a") => self.band_5ghz != "no",
                _ => self.band_2ghz != "no" || self.band_5ghz != "no",
            }
    }
}

fn select_device<'a>(devices: &'a [Device], ap: bool, band: Option<&str>) -> Result<&'a Device> {
    devices
        .iter()
        .filter(|device| device.suitable(ap, band))
        .min_by_key(|device| device.active.is_some())
        .ok_or_else(|| anyhow!("no ready NetworkManager-managed Wi-Fi interface supports the required WPA2 mode and band"))
}

struct LanDevice {
    interface: String,
    wifi: bool,
    addresses: Vec<(Ipv4Addr, Ipv4Addr)>,
}

async fn query_lan_interfaces() -> Result<Vec<LanInterface>> {
    // One live device snapshot avoids querying saved profile addresses or
    // coupling wired LAN eligibility to the global Wi-Fi radio state.
    let output = nmcli(
        &[
            "--terse",
            "--fields",
            "GENERAL.DEVICE,GENERAL.TYPE,GENERAL.STATE,GENERAL.NM-MANAGED,GENERAL.CON-UUID,IP4.ADDRESS",
            "device",
            "show",
        ],
        None,
        QUERY_WAIT,
    )
    .await?;
    let mut devices = parse_lan_devices(&output);
    devices.sort_by(|left, right| {
        (!left.wifi, &left.interface).cmp(&(!right.wifi, &right.interface))
    });
    if devices.is_empty() {
        return Ok(Vec::new());
    }
    // An unbound datagram socket is only an ioctl handle: no packets are sent.
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error()).context("cannot inspect LAN link flags");
    }
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    let addresses = if_addrs::get_if_addrs().context("cannot read live LAN addresses")?;
    let mut candidates = Vec::new();
    for device in devices {
        if !kernel_lan_ready(&device.interface, fd.as_raw_fd()).await {
            continue;
        }
        for (address, netmask) in device.addresses {
            if !addresses.iter().any(|live| {
                live.name == device.interface
                    && matches!(&live.addr, if_addrs::IfAddr::V4(ip)
                        if ip.ip == address && ip.netmask == netmask)
            }) {
                continue;
            }
            let candidate = LanInterface {
                interface: device.interface.clone(),
                address,
                netmask,
            };
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
            if candidates.len() == 8 {
                return Ok(candidates);
            }
        }
    }
    Ok(candidates)
}

fn parse_lan_devices(output: &str) -> Vec<LanDevice> {
    let mut devices = Vec::new();
    let mut rows = Vec::new();
    for line in output.lines().filter(|line| !line.is_empty()) {
        let fields = split_escaped(line);
        if fields.first().is_some_and(|key| key == "GENERAL.DEVICE") && !rows.is_empty() {
            if let Some(device) = parse_lan_device(&rows) {
                devices.push(device);
            }
            rows.clear();
        }
        rows.push(fields);
    }
    if let Some(device) = parse_lan_device(&rows) {
        devices.push(device);
    }
    devices
}

fn parse_lan_device(rows: &[Vec<String>]) -> Option<LanDevice> {
    if rows.iter().any(|row| row.len() != 2) {
        return None;
    }
    let field = |key: &str| {
        let mut values = rows.iter().filter(|row| row[0] == key);
        let value = values.next()?.get(1)?;
        // Duplicate device identity/state properties are ambiguous.
        values.next().is_none().then_some(value.as_str())
    };
    let interface = field("GENERAL.DEVICE")?;
    validate_interface(interface).ok()?;
    let wifi = match field("GENERAL.TYPE")? {
        "wifi" | "802-11-wireless" => true,
        "ethernet" | "802-3-ethernet" => false,
        _ => return None,
    };
    if field("GENERAL.STATE")?.split_whitespace().next()? != "100"
        || field("GENERAL.NM-MANAGED")? != "yes"
    {
        return None;
    }
    Uuid::parse_str(field("GENERAL.CON-UUID")?).ok()?;
    let mut addresses: Vec<_> = rows
        .iter()
        .filter(|row| {
            row[0] == "IP4.ADDRESS"
                || row[0]
                    .strip_prefix("IP4.ADDRESS[")
                    .and_then(|key| key.strip_suffix(']'))
                    .and_then(|index| index.parse::<usize>().ok())
                    .is_some_and(|index| index > 0)
        })
        .filter_map(|row| parse_lan_ipv4(&row[1]))
        .collect();
    addresses.sort_unstable();
    addresses.dedup();
    (!addresses.is_empty()).then(|| LanDevice {
        interface: interface.to_owned(),
        wifi,
        addresses,
    })
}

fn parse_lan_ipv4(value: &str) -> Option<(Ipv4Addr, Ipv4Addr)> {
    let (address, prefix) = value.split_once('/')?;
    let address = address.parse().ok()?;
    let prefix: u32 = prefix.parse().ok()?;
    if !(1..=30).contains(&prefix) {
        return None;
    }
    let netmask = Ipv4Addr::from(u32::MAX << (32 - prefix));
    usable_lan_ipv4(address, netmask).then_some((address, netmask))
}

fn usable_lan_ipv4(address: Ipv4Addr, netmask: Ipv4Addr) -> bool {
    let mask = u32::from(netmask);
    let prefix = mask.leading_ones();
    if !(1..=30).contains(&prefix) || mask != u32::MAX << (32 - prefix) {
        return false;
    }
    let host = u32::from(address) & !mask;
    is_usable_ipv4(address) && host != 0 && host != !mask
}

async fn kernel_lan_ready(interface: &str, fd: RawFd) -> bool {
    let path = Path::new("/sys/class/net").join(interface);
    // NM calls virtual Ethernet devices "ethernet" too. Hardware presence,
    // rather than an interface-name convention, excludes bridges/veth/tunnels.
    let Ok(device) = tokio::fs::canonicalize(path.join("device")).await else {
        return false;
    };
    if !device.starts_with("/sys/devices") || device.starts_with("/sys/devices/virtual") {
        return false;
    }
    let (Ok(operstate), Ok(carrier)) = (
        tokio::fs::read_to_string(path.join("operstate")).await,
        tokio::fs::read_to_string(path.join("carrier")).await,
    ) else {
        return false;
    };
    let Some(flags) = kernel_lan_flags(fd, interface) else {
        return false;
    };
    kernel_lan_link_up(flags, &operstate, &carrier)
}

fn kernel_lan_flags(fd: RawFd, interface: &str) -> Option<u32> {
    // Unlike sysfs's raw dev->flags (often 0x1003 on a healthy link),
    // SIOCGIFFLAGS includes the kernel-computed IFF_RUNNING state.
    let mut request: libc::ifreq = unsafe { std::mem::zeroed() };
    if interface.len() >= request.ifr_name.len() {
        return None;
    }
    for (slot, byte) in request.ifr_name.iter_mut().zip(interface.bytes()) {
        *slot = byte as libc::c_char;
    }
    if unsafe { libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut request) } < 0 {
        return None;
    }
    Some(u32::from(unsafe { request.ifr_ifru.ifru_flags } as u16))
}

fn kernel_lan_link_up(flags: u32, operstate: &str, carrier: &str) -> bool {
    flags & (libc::IFF_UP as u32 | libc::IFF_RUNNING as u32)
        == (libc::IFF_UP as u32 | libc::IFF_RUNNING as u32)
        && flags & libc::IFF_LOOPBACK as u32 == 0
        && operstate.trim() == "up"
        && carrier.trim() == "1"
}

async fn radio_ready() -> Result<bool> {
    let output = nmcli(
        &[
            "--get-values",
            "RUNNING,NETWORKING,WIFI-HW,WIFI",
            "general",
            "status",
        ],
        None,
        QUERY_WAIT,
    )
    .await?;
    parse_radio_ready(&output)
}

fn parse_radio_ready(output: &str) -> Result<bool> {
    let mut fields = output.trim().split(':');
    let values = [fields.next(), fields.next(), fields.next(), fields.next()];
    ensure!(
        values.iter().all(Option::is_some) && fields.next().is_none(),
        "NetworkManager returned incomplete radio state"
    );
    Ok(values
        == [
            Some("running"),
            Some("enabled"),
            Some("enabled"),
            Some("enabled"),
        ])
}

async fn ensure_permissions(ap: bool) -> Result<()> {
    let output = nmcli(
        &[
            "--terse",
            "--fields",
            "PERMISSION,VALUE",
            "general",
            "permissions",
        ],
        None,
        QUERY_WAIT,
    )
    .await?;
    let required = [
        "org.freedesktop.NetworkManager.network-control",
        "org.freedesktop.NetworkManager.settings.modify.system",
        "org.freedesktop.NetworkManager.wifi.share.protected",
    ];
    for &permission in &required[..if ap { 3 } else { 2 }] {
        let allowed = output
            .lines()
            .any(|line| line.split_once(':') == Some((permission, "yes")));
        ensure!(
            allowed,
            "NetworkManager permission denied or requires interactive authorization: {permission}"
        );
    }
    Ok(())
}

async fn probe(ap: bool) -> Result<Vec<Device>> {
    if !radio_ready().await? {
        return Ok(Vec::new());
    }
    ensure_permissions(ap).await?;
    let output = nmcli(
        &["--terse", "--fields", "DEVICE,TYPE", "device", "status"],
        None,
        QUERY_WAIT,
    )
    .await?;
    let mut devices = Vec::new();
    for line in output.lines() {
        let fields = split_escaped(line);
        if fields.len() != 2 || !matches!(fields[1].as_str(), "wifi" | "802-11-wireless") {
            continue;
        }
        let interface = &fields[0];
        validate_interface(interface)?;
        let detail = nmcli(
            &[
                "--get-values",
                "GENERAL.CON-UUID,GENERAL.STATE,GENERAL.NM-MANAGED,WIFI-PROPERTIES.AP,WIFI-PROPERTIES.WPA2,WIFI-PROPERTIES.CCMP,WIFI-PROPERTIES.2GHZ,WIFI-PROPERTIES.5GHZ",
                "device", "show", interface,
            ],
            None,
            QUERY_WAIT,
        )
        .await?;
        let values: Vec<_> = detail.lines().collect();
        ensure!(
            values.len() == 8,
            "NetworkManager returned incomplete Wi-Fi capabilities"
        );
        let state = values[1]
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .context("NetworkManager returned invalid Wi-Fi state")?;
        devices.push(Device {
            interface: interface.to_owned(),
            active: parse_active_uuid(values[0])?,
            state,
            managed: values[2] == "yes",
            ap: values[3] == "yes",
            wpa2: values[4] == "yes",
            ccmp: values[5] == "yes",
            band_2ghz: values[6].to_owned(),
            band_5ghz: values[7].to_owned(),
        });
    }
    Ok(devices)
}

async fn refuse_other_lease(device: &Device) -> Result<()> {
    if let Some(uuid) = &device.active {
        let id = nmcli(
            &[
                "--get-values",
                "connection.id",
                "connection",
                "show",
                "uuid",
                uuid,
            ],
            None,
            QUERY_WAIT,
        )
        .await?;
        ensure!(
            !id.trim().starts_with(PROFILE_PREFIX),
            "selected Wi-Fi interface belongs to another Quick Share lease; it will not be disrupted"
        );
    }
    Ok(())
}

async fn activate(
    device: &Device,
    owner: Owner,
    permit: Permit,
    ssid: String,
    password: String,
    band: &str,
    channel: Option<(u32, i32)>,
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<NetworkLease> {
    let uuid = fresh_uuid();
    // Crash recovery carries no secret: uid, PID/start ticks, our UUID and the
    // original active UUID. A stable-id marker and absence of a backing file
    // must also match before another process can reclaim this profile.
    let name = format!(
        "{PROFILE_PREFIX}{}.{}.{}.{}.{}",
        owner.uid,
        owner.pid,
        owner.start_ticks,
        uuid,
        device.active.as_deref().unwrap_or("none"),
    );
    let journal = journal_directory(&owner)
        .await?
        .join(format!("{uuid}.lease"));
    let mut cleanup = CleanupGuard {
        state: Some(Cleanup {
            uuid: uuid.clone(),
            interface: device.interface.clone(),
            original: device.active.clone(),
            profile_pending: false,
            activation_attempted: false,
            journal: Some(journal.clone()),
            _permit: permit,
        }),
    };
    let result = async {
        write_journal(&journal, &uuid, &name, device).await?;
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        ensure!(
            radio_ready().await?,
            "Wi-Fi or NetworkManager networking was disabled; it will not be enabled"
        );
        ensure!(
            active_uuid(&device.interface).await? == device.active,
            "Wi-Fi connection changed before lease creation; refusing to replace it"
        );
        let mut args = vec![
            "connection",
            "add",
            "save",
            "no",
            "type",
            "wifi",
            "connection.uuid",
            &uuid,
            "connection.id",
            &name,
            "connection.interface-name",
            &device.interface,
            "connection.autoconnect",
            "no",
            "connection.auth-retries",
            "1",
            "connection.stable-id",
            PROFILE_MARKER,
            "802-11-wireless.ssid",
            &ssid,
            "802-11-wireless.mode",
            if channel.is_some() {
                "infrastructure"
            } else {
                "ap"
            },
            "802-11-wireless.band",
            band,
            "802-11-wireless-security.key-mgmt",
            "wpa-psk",
            "802-11-wireless-security.proto",
            "rsn",
            "802-11-wireless-security.pairwise",
            "ccmp",
            "802-11-wireless-security.group",
            "ccmp",
            "802-11-wireless-security.psk-flags",
            "2",
            "ipv4.method",
            if channel.is_some() { "auto" } else { "shared" },
            "ipv4.never-default",
            "yes",
            "ipv4.ignore-auto-dns",
            "yes",
            "ipv6.method",
            "disabled",
        ];
        let channel_number = channel.map(|(channel, _)| channel.to_string());
        if let Some(channel_number) = &channel_number {
            args.extend(["802-11-wireless.channel", channel_number.as_str()]);
        }
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        cleanup.state.as_mut().unwrap().profile_pending = true;
        nmcli(&args, None, QUERY_WAIT).await?;
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        ensure!(
            radio_ready().await?,
            "Wi-Fi or NetworkManager networking was disabled; it will not be enabled"
        );
        ensure!(
            active_uuid(&device.interface).await? == device.active,
            "Wi-Fi connection changed before activation; refusing to replace it"
        );
        // The password is delivered through a pipe, never argv or a disk file.
        // NOT_SAVED prevents a secret agent from persisting it. 'save no' is
        // nmcli add's in-memory equivalent of modify --temporary.
        let secret = format!("802-11-wireless-security.psk:{password}\n");
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        cleanup.state.as_mut().unwrap().activation_attempted = true;
        nmcli(
            &[
                "connection",
                "up",
                "uuid",
                &uuid,
                "ifname",
                &device.interface,
                "passwd-file",
                "/dev/stdin",
            ],
            Some(secret.as_bytes()),
            ACTIVATION_WAIT,
        )
        .await?;
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        ensure!(
            active_uuid(&device.interface).await?.as_deref() == Some(uuid.as_str()),
            "NetworkManager did not activate the requested Quick Share profile"
        );
        let interface = nmcli(
            &[
                "--get-values",
                "GENERAL.IP-IFACE",
                "device",
                "show",
                &device.interface,
            ],
            None,
            QUERY_WAIT,
        )
        .await?;
        let interface = interface.trim().to_owned();
        validate_interface(&interface)?;
        let gateway = active_gateway(&interface, channel.is_none()).await?;
        let frequency = active_frequency(&device.interface).await?;
        let (actual_band, _) = frequency_channel(frequency)?;
        ensure!(
            actual_band == band,
            "NetworkManager activated the wrong Wi-Fi band"
        );
        if let Some((_, requested)) = channel {
            ensure!(
                frequency == requested,
                "NetworkManager joined a different channel than the negotiated hotspot frequency"
            );
        }
        ensure!(
            !cancellation.is_cancelled(),
            "network negotiation cancelled"
        );
        Ok::<_, anyhow::Error>((interface, gateway, frequency))
    }
    .await;
    match result {
        Ok((interface, gateway, frequency)) => Ok(NetworkLease {
            interface,
            gateway,
            frequency,
            ssid,
            password,
            cleanup,
        }),
        Err(error) => match cleanup.close().await {
            Ok(()) => Err(error),
            Err(cleanup_error) => Err(anyhow::Error::new(RestorationFailure(cleanup_error))
                .context(format!("network negotiation failed: {error}"))),
        },
    }
}

async fn active_gateway(interface: &str, hosting: bool) -> Result<IpAddr> {
    let property = if hosting {
        "IP4.ADDRESS"
    } else {
        "IP4.GATEWAY"
    };
    let output = nmcli(
        &["--get-values", property, "device", "show", interface],
        None,
        QUERY_WAIT,
    )
    .await?;
    for line in output.lines() {
        let address = if hosting {
            line.split('/').next().unwrap_or("")
        } else {
            line
        };
        if let Some(address) = usable_ipv4(address.trim()) {
            return Ok(IpAddr::V4(address));
        }
    }
    // never-default avoids hijacking the user's global routing/DNS. NM may
    // consequently omit IP4.GATEWAY, but still exposes the actual DHCP router.
    if !hosting {
        let options = nmcli(
            &["--get-values", "DHCP4.OPTION", "device", "show", interface],
            None,
            QUERY_WAIT,
        )
        .await?;
        for option in options.lines() {
            if let Some((key, value)) = option.split_once('=') {
                if key.trim() == "routers" {
                    for router in value.split_whitespace() {
                        if let Some(address) = usable_ipv4(router) {
                            return Ok(IpAddr::V4(address));
                        }
                    }
                }
            }
        }
    }
    bail!("NetworkManager did not provide the active hotspot's IPv4 address or DHCP gateway")
}

fn usable_ipv4(value: &str) -> Option<Ipv4Addr> {
    let address = value.parse().ok()?;
    is_usable_ipv4(address).then_some(address)
}

fn is_usable_ipv4(address: Ipv4Addr) -> bool {
    !address.is_unspecified()
        && !address.is_loopback()
        && !address.is_multicast()
        && !address.is_broadcast()
        && !address.is_link_local()
}

async fn active_frequency(interface: &str) -> Result<i32> {
    let output = nmcli(
        &[
            "--terse",
            "--fields",
            "IN-USE,FREQ",
            "device",
            "wifi",
            "list",
            "ifname",
            interface,
            "--rescan",
            "no",
        ],
        None,
        QUERY_WAIT,
    )
    .await?;
    for line in output.lines() {
        let fields = split_escaped(line);
        if fields.len() == 2 && fields[0] == "*" {
            return fields[1]
                .split_whitespace()
                .next()
                .and_then(|frequency| frequency.parse().ok())
                .context("NetworkManager returned an invalid active Wi-Fi frequency");
        }
    }
    bail!("NetworkManager did not report the active Wi-Fi frequency; hotspot credentials cannot be advertised")
}

async fn active_uuid(interface: &str) -> Result<Option<String>> {
    let output = nmcli(
        &[
            "--get-values",
            "GENERAL.CON-UUID",
            "device",
            "show",
            interface,
        ],
        None,
        QUERY_WAIT,
    )
    .await?;
    parse_active_uuid(output.trim())
}

fn parse_active_uuid(value: &str) -> Result<Option<String>> {
    if value.is_empty() || value == "--" {
        return Ok(None);
    }
    Uuid::parse_str(value).context("NetworkManager returned an invalid active connection UUID")?;
    Ok(Some(value.to_owned()))
}

fn validate_interface(interface: &str) -> Result<()> {
    ensure!(
        !interface.is_empty()
            && interface.len() <= 15
            && interface
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.')),
        "NetworkManager returned an unsupported Wi-Fi interface name"
    );
    Ok(())
}

fn fresh_uuid() -> String {
    let mut bytes = rand::random::<[u8; 16]>();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes).to_string()
}

struct Owner {
    uid: u32,
    pid: u32,
    start_ticks: u64,
}

async fn current_owner() -> Result<Owner> {
    let status = tokio::fs::read_to_string("/proc/self/status")
        .await
        .context("cannot determine the unprivileged network lease owner")?;
    let uid = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|uid| uid.parse().ok())
        .context("cannot determine the network lease owner's effective UID")?;
    ensure!(uid != 0, "Quick Share network leases must not run as root");
    let pid = std::process::id();
    let stat = tokio::fs::read_to_string("/proc/self/stat").await?;
    let start_ticks = process_start(&stat)
        .context("cannot determine the network lease owner's process identity")?;
    Ok(Owner {
        uid,
        pid,
        start_ticks,
    })
}

fn process_start(stat: &str) -> Option<u64> {
    // comm (field 2) may contain spaces or parentheses; field 22 is starttime.
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

/// Journals contain no SSID, password, IP address or payload metadata. The
/// durable record exists before any nmcli mutation and survives profile
/// deletion, so interrupted deletion/restoration remains recoverable.
async fn journal_directory(owner: &Owner) -> Result<PathBuf> {
    let dirs = directories::BaseDirs::new()
        .context("cannot determine the network recovery journal directory")?;
    let base = dirs
        .runtime_dir()
        .or_else(|| dirs.state_dir())
        .context("neither an XDG runtime directory nor a state directory is available")?;
    ensure!(
        base.is_absolute(),
        "network recovery directory must be absolute"
    );
    tokio::fs::create_dir_all(base)
        .await
        .context("could not create the network recovery state directory")?;
    let base_metadata = tokio::fs::symlink_metadata(base).await?;
    ensure_secure_metadata(&base_metadata, owner.uid, false)?;
    let path = base.join("omarchy-quickshare-network");
    match tokio::fs::create_dir(&path).await {
        Ok(()) => {
            #[cfg(unix)]
            tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).await?;
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => {
            return Err(error).context("could not create the network recovery journal directory")
        }
    }
    let metadata = tokio::fs::symlink_metadata(&path).await?;
    ensure_secure_metadata(&metadata, owner.uid, true)?;
    Ok(path)
}

fn ensure_secure_metadata(metadata: &std::fs::Metadata, uid: u32, private: bool) -> Result<()> {
    ensure!(
        metadata.is_dir(),
        "network recovery directory is not a real directory"
    );
    #[cfg(unix)]
    ensure!(
        metadata.uid() == uid && metadata.mode() & (if private { 0o077 } else { 0o022 }) == 0,
        "network recovery directory has an unsafe owner or permissions"
    );
    #[cfg(not(unix))]
    {
        let _ = (uid, private);
        bail!("network recovery journals require Linux");
    }
    Ok(())
}

async fn write_journal(path: &Path, uuid: &str, id: &str, device: &Device) -> Result<()> {
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .await
        .context("could not create the private network recovery journal")?;
    #[cfg(unix)]
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .await?;
    let record = format!(
        "{PROFILE_MARKER}\n{uuid}\n{id}\n{}\n{}\n",
        device.active.as_deref().unwrap_or("none"),
        device.interface,
    );
    file.write_all(record.as_bytes()).await?;
    file.flush().await?;
    file.sync_all()
        .await
        .context("could not commit the network recovery journal")?;
    Ok(())
}

struct OwnedProfile {
    pid: u32,
    start_ticks: u64,
    original: Option<String>,
}

fn owned_profile(id: &str, uuid: &str, uid: u32) -> Option<OwnedProfile> {
    let parts: Vec<_> = id.strip_prefix(PROFILE_PREFIX)?.split('.').collect();
    if parts.len() != 5
        || parts[0].parse::<u32>().ok()? != uid
        || parts[3] != uuid
        || Uuid::parse_str(uuid).is_err()
    {
        return None;
    }
    let pid = parts[1].parse().ok()?;
    if pid == 0 {
        return None;
    }
    let start_ticks = parts[2].parse().ok()?;
    let original = if parts[4] == "none" {
        None
    } else {
        Uuid::parse_str(parts[4]).ok()?;
        if parts[4] == uuid {
            return None;
        }
        Some(parts[4].to_owned())
    };
    Some(OwnedProfile {
        pid,
        start_ticks,
        original,
    })
}

async fn owner_is_dead(profile: &OwnedProfile) -> bool {
    match tokio::fs::read_to_string(format!("/proc/{}/stat", profile.pid)).await {
        Ok(stat) => process_start(&stat).is_some_and(|current| current != profile.start_ticks),
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

async fn recover_journals(owner: &Owner, permit: &Permit) -> Result<()> {
    let directory = journal_directory(owner).await?;
    let mut entries = tokio::fs::read_dir(directory).await?;
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        let Some(uuid) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".lease"))
        else {
            continue;
        };
        if Uuid::parse_str(uuid).is_err() {
            continue;
        }
        let metadata = tokio::fs::symlink_metadata(&path).await?;
        if !metadata.is_file() || metadata.len() > 4096 {
            continue;
        }
        #[cfg(unix)]
        if metadata.uid() != owner.uid || metadata.mode() & 0o777 != 0o600 {
            continue;
        }
        let record = tokio::fs::read_to_string(&path)
            .await
            .context("could not read the network recovery journal")?;
        let fields: Vec<_> = record.lines().collect();
        if fields.len() != 5
            || fields[0] != PROFILE_MARKER
            || fields[1] != uuid
            || validate_interface(fields[4]).is_err()
        {
            continue;
        }
        let Some(profile) = owned_profile(fields[2], uuid, owner.uid) else {
            continue;
        };
        if profile.original.as_deref().unwrap_or("none") != fields[3]
            || !owner_is_dead(&profile).await
        {
            continue;
        }
        let profiles = nmcli(
            &[
                "--terse",
                "--fields",
                "UUID,NAME,TYPE,FILENAME",
                "connection",
                "show",
            ],
            None,
            QUERY_WAIT,
        )
        .await?;
        let existing = profiles
            .lines()
            .map(split_escaped)
            .find(|values| values.first().is_some_and(|value| value == uuid));
        if let Some(existing) = &existing {
            ensure!(
                existing.len() == 4 && existing[1] == fields[2] && existing[2] == "802-11-wireless"
                    && (existing[3].is_empty() || existing[3] == "--"),
                "network recovery refuses a connection that no longer matches its owned in-memory profile"
            );
            let detail = nmcli(
                &[
                    "--get-values",
                    "connection.stable-id,connection.autoconnect,connection.interface-name",
                    "connection",
                    "show",
                    "uuid",
                    uuid,
                ],
                None,
                QUERY_WAIT,
            )
            .await?;
            ensure!(
                detail.lines().collect::<Vec<_>>() == [PROFILE_MARKER, "no", fields[4]],
                "network recovery profile ownership or interface changed; it will not be modified"
            );
        }
        let restore_needed = if profile.original.is_some() {
            let current = active_uuid(fields[4]).await?;
            current.is_none() || current.as_deref() == Some(uuid)
        } else {
            false
        };
        if existing.is_some() || restore_needed {
            ensure_permissions(false).await?;
        }
        let mut cleanup = CleanupGuard {
            state: Some(Cleanup {
                uuid: uuid.to_owned(),
                interface: fields[4].to_owned(),
                original: profile.original,
                profile_pending: existing.is_some(),
                activation_attempted: restore_needed,
                journal: Some(path.clone()),
                _permit: permit.clone(),
            }),
        };
        cleanup
            .close()
            .await
            .context("failed to recover an interrupted Quick Share network lease")?;
    }
    Ok(())
}

async fn reclaim_stale(owner: &Owner, permit: &Permit) -> Result<()> {
    let output = match nmcli(
        &[
            "--terse",
            "--fields",
            "UUID,NAME,TYPE,FILENAME",
            "connection",
            "show",
        ],
        None,
        QUERY_WAIT,
    )
    .await
    {
        Ok(output) => output,
        Err(error) if is_unavailable(&error) => return Ok(()),
        Err(error) => return Err(error),
    };
    for line in output.lines() {
        let fields = split_escaped(line);
        if fields.len() != 4
            || fields[2] != "802-11-wireless"
            || (!fields[3].is_empty() && fields[3] != "--")
        {
            continue;
        }
        let Some(profile) = owned_profile(&fields[1], &fields[0], owner.uid) else {
            continue;
        };
        if !owner_is_dead(&profile).await {
            continue;
        }
        let detail = nmcli(
            &[
                "--get-values",
                "connection.stable-id,connection.autoconnect,connection.interface-name",
                "connection",
                "show",
                "uuid",
                &fields[0],
            ],
            None,
            QUERY_WAIT,
        )
        .await?;
        let values: Vec<_> = detail.lines().collect();
        if values.len() != 3
            || values[0] != PROFILE_MARKER
            || values[1] != "no"
            || validate_interface(values[2]).is_err()
        {
            continue;
        }
        let interface = values[2].to_owned();
        let was_active = active_uuid(&interface).await?.as_deref() == Some(fields[0].as_str());
        ensure_permissions(false).await?;
        let mut cleanup = CleanupGuard {
            state: Some(Cleanup {
                uuid: fields[0].clone(),
                interface,
                original: profile.original,
                profile_pending: true,
                // Restore only an actually active stale lease. An inactive
                // orphan must not cause unrelated current networking to change.
                activation_attempted: was_active,
                journal: None,
                _permit: permit.clone(),
            }),
        };
        cleanup
            .close()
            .await
            .context("failed to reclaim an owned, dead-process Quick Share connection")?;
    }
    Ok(())
}

struct Cleanup {
    uuid: String,
    interface: String,
    original: Option<String>,
    profile_pending: bool,
    activation_attempted: bool,
    journal: Option<PathBuf>,
    _permit: Permit,
}

impl Cleanup {
    async fn close(&mut self) -> Result<()> {
        // Restore while our profile is still active. Deleting it first would
        // allow NM to autoconnect a different profile before we restore the
        // exact original UUID. A genuinely newer user connection is preserved.
        let restoration = async {
            if !self.activation_attempted {
                return Ok(());
            }
            if let Some(original) = &self.original {
                let current = active_uuid(&self.interface).await?;
                if current.as_deref() == Some(original.as_str()) {
                    self.activation_attempted = false;
                    return Ok(());
                }
                if current.is_some() && current.as_deref() != Some(self.uuid.as_str()) {
                    log::warn!("Quick Share is preserving a newer Wi-Fi connection instead of restoring the old one");
                    self.activation_attempted = false;
                    return Ok(());
                }
                ensure!(radio_ready().await?, "previous Wi-Fi cannot be restored because its radio or networking was disabled; it will not be enabled");
                nmcli(
                    &["connection", "up", "uuid", original, "ifname", &self.interface],
                    None,
                    ACTIVATION_WAIT,
                )
                .await?;
                ensure!(active_uuid(&self.interface).await?.as_deref() == Some(original.as_str()), "NetworkManager did not restore the previous active Wi-Fi UUID");
            }
            self.activation_attempted = false;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let mut deletion_error = None;
        if self.profile_pending {
            match nmcli(
                &["connection", "delete", "uuid", &self.uuid],
                None,
                QUERY_WAIT,
            )
            .await
            {
                Ok(_) => self.profile_pending = false,
                Err(error)
                    if matches!(
                        error.downcast_ref::<NmcliFailure>(),
                        Some(NmcliFailure::Exit(Some(10)))
                    ) =>
                {
                    self.profile_pending = false;
                }
                Err(error) => deletion_error = Some(error),
            }
        }
        match (deletion_error, restoration) {
            (None, result) => result,
            (Some(error), Ok(())) => {
                Err(error.context("could not delete the temporary Quick Share connection"))
            }
            (Some(error), Err(restore_error)) => Err(error.context(format!(
                "temporary connection deletion and Wi-Fi restoration failed: {restore_error}"
            ))),
        }?;
        if let Some(path) = &self.journal {
            match tokio::fs::remove_file(path).await {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error)
                        .context("could not remove the completed network recovery journal")
                }
            }
        }
        self.journal = None;
        Ok(())
    }
}

struct CleanupGuard {
    state: Option<Cleanup>,
}

impl CleanupGuard {
    async fn close(&mut self) -> Result<()> {
        if let Some(state) = &mut self.state {
            state.close().await?;
            self.state.take();
        }
        Ok(())
    }
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let Some(state) = self.state.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let mut recovery = ShutdownRecovery(Some(state));
            runtime.spawn(async move {
                // If the runtime cancels this cleanup task during shutdown,
                // move its remaining work to a short-lived independent runtime.
                if let Err(error) = recovery.0.as_mut().unwrap().close().await {
                    log::warn!("Quick Share network cleanup failed: {error}");
                }
                recovery.0.take();
            });
        } else {
            cleanup_on_thread(state);
        }
    }
}

struct ShutdownRecovery(Option<Cleanup>);

impl Drop for ShutdownRecovery {
    fn drop(&mut self) {
        if let Some(state) = self.0.take() {
            cleanup_on_thread(state);
        }
    }
}

fn cleanup_on_thread(mut state: Cleanup) {
    let result = std::thread::Builder::new()
        .name("quickshare-network-cleanup".to_owned())
        .spawn(move || {
            match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => {
                    if let Err(error) = runtime.block_on(state.close()) {
                        log::warn!("Quick Share network cleanup failed: {error}");
                    }
                }
                Err(_) => log::warn!("Quick Share could not start the network cleanup runtime"),
            }
        });
    if result.is_err() {
        log::warn!("Quick Share could not start its network cleanup thread");
    }
}

#[derive(Debug)]
enum NmcliFailure {
    Missing,
    Stopped,
    Permission,
    Timeout,
    Exit(Option<i32>),
}

impl fmt::Display for NmcliFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => f.write_str("nmcli is not installed or not executable"),
            Self::Stopped => f.write_str("NetworkManager is not running"),
            Self::Permission => f.write_str("NetworkManager denied permission or requires interactive authorization"),
            Self::Timeout => f.write_str("NetworkManager operation exceeded its bounded deadline"),
            Self::Exit(code) => write!(f, "NetworkManager operation failed (nmcli exit status {code:?}; command output is withheld to protect credentials)"),
        }
    }
}

impl std::error::Error for NmcliFailure {}

fn is_unavailable(error: &anyhow::Error) -> bool {
    matches!(
        error.downcast_ref::<NmcliFailure>(),
        Some(NmcliFailure::Missing | NmcliFailure::Stopped)
    )
}

async fn nmcli(args: &[&str], input: Option<&[u8]>, wait_seconds: u32) -> Result<String> {
    let wait = wait_seconds.to_string();
    let mut command = Command::new("nmcli");
    command
        .args(["--colors", "no", "--escape", "yes", "--wait", &wait])
        .args(args)
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            anyhow!(NmcliFailure::Missing)
        } else {
            anyhow!(error).context("could not start nmcli")
        }
    })?;
    let operation = async {
        if let Some(input) = input {
            let mut stdin = child
                .stdin
                .take()
                .context("nmcli input pipe was not created")?;
            stdin
                .write_all(input)
                .await
                .context("could not provide nmcli credentials through its input pipe")?;
            stdin.shutdown().await?;
        }
        child
            .wait_with_output()
            .await
            .context("could not collect nmcli result")
    };
    let output = timeout(Duration::from_secs(u64::from(wait_seconds) + 5), operation)
        .await
        .map_err(|_| anyhow!(NmcliFailure::Timeout))??;
    if !output.status.success() {
        // Do not return or log raw output: nmcli can echo SSIDs or credentials
        // when parsing an invalid property or reporting an activation failure.
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        let failure = if stderr.contains("not authorized")
            || stderr.contains("not authorised")
            || stderr.contains("permission denied")
            || stderr.contains("insufficient privileges")
            || stderr.contains("authorization")
        {
            NmcliFailure::Permission
        } else {
            match output.status.code() {
                Some(3) => NmcliFailure::Timeout,
                Some(8) => NmcliFailure::Stopped,
                code => NmcliFailure::Exit(code),
            }
        };
        return Err(anyhow!(failure));
    }
    String::from_utf8(output.stdout).context("nmcli returned non-UTF-8 output")
}

fn split_escaped(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut escaped = false;
    for character in line.chars() {
        if escaped {
            fields.last_mut().unwrap().push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ':' {
            fields.push(String::new());
        } else {
            fields.last_mut().unwrap().push(character);
        }
    }
    if escaped {
        fields.last_mut().unwrap().push('\\');
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::{
        frequency_channel, kernel_lan_link_up, parse_lan_devices, parse_lan_ipv4,
        parse_radio_ready, require_consent, usable_lan_ipv4, validate_credentials,
    };
    use std::net::Ipv4Addr;

    #[test]
    fn lan_snapshot_requires_live_activation_and_current_addresses() {
        let detail = "GENERAL.DEVICE:physical0\n\
GENERAL.TYPE:wifi\n\
GENERAL.STATE:100 (connected)\n\
GENERAL.NM-MANAGED:yes\n\
GENERAL.CON-UUID:ab11bc22-cd33-4e44-af55-001122334455\n\
IP4.ADDRESS[1]:192.168.7.9/24\n\
IP4.ADDRESS[2]:10.20.30.40/16\n\
IP4.ADDRESS[3]:192.168.7.9/24\n\
IP4.ADDRESS[4]:169.254.1.2/16\n";
        let devices = parse_lan_devices(detail);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].interface, "physical0");
        assert!(devices[0].wifi);
        assert_eq!(
            devices[0].addresses,
            vec![
                (Ipv4Addr::new(10, 20, 30, 40), Ipv4Addr::new(255, 255, 0, 0)),
                (Ipv4Addr::new(192, 168, 7, 9), Ipv4Addr::new(255, 255, 255, 0)),
            ]
        );
        for (from, to) in [
            ("100 (connected)", "30 (disconnected)"),
            ("GENERAL.NM-MANAGED:yes", "GENERAL.NM-MANAGED:no"),
            ("ab11bc22-cd33-4e44-af55-001122334455", "--"),
            ("ab11bc22-cd33-4e44-af55-001122334455", "invalid"),
            ("GENERAL.TYPE:wifi", "GENERAL.TYPE:bridge"),
            ("GENERAL.TYPE:wifi", "GENERAL.TYPE:tun"),
            ("GENERAL.DEVICE:physical0", "GENERAL.DEVICE:../escape"),
        ] {
            assert!(parse_lan_devices(&detail.replace(from, to)).is_empty());
        }
        assert!(parse_lan_devices(&format!("{detail}GENERAL.STATE:100\n")).is_empty());
        let wired = detail
            .replace("GENERAL.TYPE:wifi", "GENERAL.TYPE:ethernet")
            .replace("physical0", "wired0");
        let devices = parse_lan_devices(&format!("{detail}\n{wired}"));
        assert_eq!(devices.len(), 2);
        assert!(!devices[1].wifi);
        assert_eq!(devices[1].interface, "wired0");
    }

    #[test]
    fn lan_ipv4_accepts_real_subnets_without_private_address_assumptions() {
        for address in ["192.168.7.9/24", "10.20.30.40/16", "198.51.100.9/24"] {
            assert!(parse_lan_ipv4(address).is_some(), "{address}");
        }
        for address in [
            "0.0.0.0/24",
            "127.0.0.1/8",
            "224.0.0.1/24",
            "255.255.255.255/24",
            "169.254.1.2/16",
            "192.168.7.0/24",
            "192.168.7.255/24",
            "192.168.7.9/0",
            "192.168.7.9/31",
            "192.168.7.9/32",
            "192.168.7.9/33",
            "192.168.7.9",
            "192.168.7.9/24/24",
        ] {
            assert!(parse_lan_ipv4(address).is_none(), "{address}");
        }
        let address = Ipv4Addr::new(192, 168, 7, 9);
        for mask in [
            Ipv4Addr::UNSPECIFIED,
            Ipv4Addr::new(255, 0, 255, 0),
            Ipv4Addr::new(255, 255, 255, 254),
            Ipv4Addr::BROADCAST,
        ] {
            assert!(!usable_lan_ipv4(address, mask), "{mask}");
        }
        assert!(usable_lan_ipv4(address, Ipv4Addr::new(255, 255, 255, 0)));
    }

    #[test]
    fn lan_link_requires_up_running_carrier_and_operstate() {
        assert!(kernel_lan_link_up(0x1043, "up\n", "1\n"));
        for (flags, state, carrier) in [
            (0x1003, "up", "1"),
            (0x1042, "up", "1"),
            (0x1043, "down", "1"),
            (0x1043, "dormant", "1"),
            (0x1043, "up", "0"),
            (0x104b, "up", "1"),
        ] {
            assert!(!kernel_lan_link_up(flags, state, carrier));
        }
    }

    #[test]
    fn radio_state_parses_nmcli_terse_rows_without_enabling_disabled_radios() {
        assert!(parse_radio_ready("running:enabled:enabled:enabled\n").unwrap());
        for row in [
            "not running:enabled:enabled:enabled",
            "running:disabled:enabled:enabled",
            "running:enabled:disabled:enabled",
            "running:enabled:enabled:disabled",
        ] {
            assert!(!parse_radio_ready(row).unwrap());
        }
        assert!(parse_radio_ready("running:enabled:enabled").is_err());
        assert!(parse_radio_ready("running:enabled:enabled:enabled:extra").is_err());
    }

    #[test]
    fn consent_is_required_only_for_an_occupied_radio() {
        assert!(require_consent(false, false).is_ok());
        assert!(require_consent(false, true).is_ok());
        assert!(require_consent(true, false).is_err());
        assert!(require_consent(true, true).is_ok());
    }

    #[test]
    fn validates_ssid_byte_length_and_control_characters() {
        for ssid in ["", "bad\nssid", "bad\0ssid"] {
            assert!(validate_credentials(ssid, "password", 2412).is_err());
        }
        assert!(validate_credentials(&"a".repeat(32), "password", 2412).is_ok());
        assert!(validate_credentials(&"a".repeat(33), "password", 2412).is_err());
        assert!(validate_credentials(&"é".repeat(16), "password", 2412).is_ok());
        assert!(validate_credentials(&"é".repeat(17), "password", 2412).is_err());
    }

    #[test]
    fn validates_wpa2_password_without_accepting_pipe_injection() {
        for password in ["", "1234567", "1234567\n", "1234567\0", "pässword"] {
            assert!(validate_credentials("ssid", password, 2412).is_err());
        }
        assert!(validate_credentials("ssid", &"a".repeat(63), 2412).is_ok());
        assert!(validate_credentials("ssid", &"a".repeat(64), 2412).is_ok());
        assert!(validate_credentials("ssid", &"z".repeat(64), 2412).is_err());
        assert!(validate_credentials("ssid", &"a".repeat(65), 2412).is_err());
    }

    #[test]
    fn validates_exact_channel_frequencies_not_arbitrary_band_values() {
        assert_eq!(frequency_channel(2412).unwrap(), ("bg", 1));
        assert_eq!(frequency_channel(2472).unwrap(), ("bg", 13));
        assert_eq!(frequency_channel(2484).unwrap(), ("bg", 14));
        assert_eq!(frequency_channel(5180).unwrap(), ("a", 36));
        assert_eq!(frequency_channel(5825).unwrap(), ("a", 165));
        for frequency in [-1, 0, 2400, 2413, 2480, 5000, 5181, 5955, 7115] {
            assert!(frequency_channel(frequency).is_err());
        }
    }
}
