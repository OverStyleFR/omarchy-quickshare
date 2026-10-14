# Architecture and command reference

[Back to Quick Share](../README.md) · [User guide](USER_GUIDE.md) · [Installation](INSTALLATION.md)

## Components

| Component | Responsibility |
| --- | --- |
| Rust/Tokio service | Discovery, connection negotiation, authentication, encryption, file streaming and transfer state |
| Omarchy QML plugin | Bar visibility, nearby devices, file selection, consent, progress, cancellation and service maintenance |
| Session D-Bus and CLI | Control commands and versioned snapshots between the independent daemon and UI |
| Nautilus extension | Preload one or multiple local files into the panel without automatically sending |
| BlueZ | BLE/GATT discovery metadata and Classic RFCOMM data sockets |
| NetworkManager | Local network information and consent-controlled temporary network profiles |

The daemon continues independently of the panel. A shell reload reconnects the UI to the daemon snapshot instead of creating a second transfer engine. File content and cryptographic keys do not pass through QML or D-Bus.

## Session interface

- Service: `io.omarchy.QuickShare`
- Object: `/io/omarchy/QuickShare`
- Interface: `io.omarchy.QuickShare1`
- Stable plugin ID: `macondo.quickshare`

Snapshots carry an instance ID, monotonic revision, visibility/deadline, backend availability, nearby devices, transfer IDs/states, bytes, transport and errors. The UI ignores stale revisions and recovers after service ownership changes.

A transfer's medium and terminal result come from engine state, not a UI capability label. Send/accept/reject/cancel operate on the actual peer or transfer IDs.

Selected file paths travel through private stdin pipes and the user's session D-Bus, not subprocess arguments. `PrepareShare` holds a selection in daemon memory and returns an opaque token. The shell summon carries only that token; the panel retrieves the paths with `TakeShare`, which consumes the selection once. Pending selections expire after five minutes and are limited to 16. Opening a selection does not start a transfer. This boundary protects against other local users reading process command lines; it does not isolate the application from root or other processes in the same user session.

## Local transports and authentication

Direct LAN discovery uses mDNS and TCP, default port **47593**. Bluetooth discovery uses BLE/GATT metadata; file content travels through Classic RFCOMM rather than OBEX or GATT characteristics.

The protocol uses UKEY2/P-256 key exchange, authenticated encryption/MAC and message-sequence checks. Names and sizes are introduced for the receiver's consent screen. Device names and Bluetooth addresses are discovery metadata, not authenticated account identity. Verify the displayed code and handle consent explicitly.

Since backend **0.2.3**, accepting an incoming Bluetooth session attempts an existing-LAN upgrade before offering a hotspot, when the peer advertises Wi-Fi LAN support. A read-only NetworkManager snapshot is intersected with live kernel addresses and operational link state. Upgrade hosting admits activated physical Wi-Fi/Ethernet interfaces with usable IPv4 subnets, not software bridges, VLANs, tunnels, link-local or IPv6-only addresses. Candidates reuse the configured LAN listener's actual TCP port, normally **47593**. Accepted upgrade sockets are restricted to the advertised address, active interface and subnet; no profile, firewall rule or Wi-Fi setting is changed.

The shared listener distinguishes new connection requests from upgrade introductions with the same cancellation-safe frame reader used by sessions. New requests still require receive visibility; an existing transfer's registered upgrade can arrive after visibility expires. Four-byte public endpoint IDs route only to a unique owned admission slot. Duplicate registrations are refused, queues are bounded, and selecting one socket closes admission without replacing it or deleting a newer registration.

The LAN offer travels over the authenticated old channel. Public `CLIENT_INTRODUCTION` routes the candidate; it is not key proof. Native `LAST_WRITE` / `SAFE_TO_CLOSE` and old-channel terminal shutdown precede **encrypted** replacement-channel traffic. Existing keys and sequence counters continue without reset. Already-authorized sharing acceptance can be encrypted on the replacement, but file sending and the reported LAN medium wait for authenticated replacement-channel input. The ordering follows [Google Nearby's BWU implementation](https://github.com/google/nearby/blob/d128b5fca30b3fa8d4642f8c75dd9e314e090e74/connections/implementation/bwu_manager.cc).

Setup, connection or introduction failure before either old-channel `LAST_WRITE` retains healthy Bluetooth; it does not replace an active Wi-Fi connection with a hotspot. After drain begins, failure is terminal rather than a pretend Bluetooth rollback. A shared SSID or working Internet connection does not prove peer reachability through a firewall or AP client isolation.

Since backend **0.2.4**, the mDNS cache, aggregate peer/routing index and public snapshot each retain at most **128 discovered peers**. A LAN service is identified by transport and fullname, not its mutable address/port. Replacing its route withdraws the previous ID before probing or publishing the replacement; an old owner's withdrawal cannot remove a newer owner of the same address. Count pressure evicts the oldest observation.

LAN observations have a **120-second monotonic lease**. Source and aggregate timers expire them even while receive visibility is off; snapshot reads and Send also prune expired devices. Lease-only refreshes do not churn the serialized model. Bluetooth keeps its existing discovery-owned liveness because unchanged Bluetooth observations are not emitted as refresh events. Dropped discovery events invalidate the affected cache/model instead of retaining an unknowable partial list.

The pinned mDNS dependency silently renews unchanged DNS records without sending a new resolution event. The owned discovery browser therefore starts a fresh daemon/browse every **60 seconds**, retaining only the bounded application cache and renewing leases only from fresh validated responses and reachable TCP endpoints. Old-generation events are drained and discarded; advertisement, Bluetooth, active file sockets, PIN consent and network configuration are untouched.

Live LE connections hold the radio-coordination gate even when a peer skips reading our GATT metadata. An ordinary-user, unbound HCI socket uses the read-only Linux `HCIGETCONNLIST` ioctl to distinguish LE bearers, including public-address links, from unrelated Classic audio connections. No extra capabilities or privileged Bluetooth monitor are required. Device connection signals wake the coordinator; a passive policy tick also detects LE disappearance when BlueZ's aggregate `Connected` flag remains true for Classic.

Hiding or cancelling a transfer is not disconnect confirmation. Incomplete or unreadable connection snapshots fail closed. Radio errors remain explicit; the service does not reset adapters, disconnect foreign clients or silently retry a transfer. Coordination checks prevent changes while a known LE bearer is live, but are not a guarantee against firmware faults or races with other Bluetooth clients.

Temporary hotspot changes require explicit consent before interrupting an existing Wi-Fi connection. Hosting/joining those routes uses NetworkManager-owned profiles and protocol-protected credentials; existing-LAN upgrades use only read-only interface inspection and owned sockets. No Wi-Fi Direct/P2P, WPA3, 6 GHz, Google account/contact identity or lossless resume is advertised.

## File publication and cancellation

Incoming storage is anchored to the destination directory descriptor. Remote names are validated, size/offset sequences are checked, and staging files use restrictive permissions. Completed files are published without overwriting existing destinations. A collision produces a numbered name.

Cancelled or failed incoming sessions roll back local staging. The sender cannot delete Android's files: Android's Quick Share implementation owns its destination and cache lifecycle. A local cancelled/failed state is not a promise of remote storage cleanup. Zero-byte offers can be rejected by Android; content is not transformed to disguise that result.

The sender's cancellation path retains encrypted-frame state, sends the protocol cancellation messages and retires resources before publishing its terminal result. Cancellation and disconnection are not completion, and a transfer is not silently resumed or retried.

## CLI

```bash
omarchy-quickshare doctor
omarchy-quickshare status
omarchy-quickshare watch
omarchy-quickshare visible 120
omarchy-quickshare visible 0
omarchy-quickshare network-consent on
omarchy-quickshare network-consent off
omarchy-quickshare pick-files | omarchy-quickshare share
```

`share` prepares the panel only. `share` and `send DEVICE_ID` read a JSON array of file paths from stdin; `download-dir` reads a JSON string. File paths are not accepted as positional arguments. Keep paths out of command strings and external programs' arguments when constructing input; the portal pipeline above avoids both. Other commands:

| Command | Effect |
| --- | --- |
| `send DEVICE_ID` | Request an outgoing transfer using a JSON file-path array on stdin |
| `accept TRANSFER_ID` | Approve the identified request |
| `reject TRANSFER_ID` | Reject the identified request |
| `cancel TRANSFER_ID` | Request cancellation |
| `download-dir` | Select an existing absolute destination from a JSON string on stdin |
| `pick-files` | Open the desktop portal chooser; cancellation returns `[]` |
| `take-share TOKEN` | Retrieve and consume a prepared selection; output a JSON array |
| `watch` | Initial snapshot followed by NDJSON changes |

Use `--help` for current arguments. `doctor` inspects local tools, radio availability and permissions; it does not certify a remote Android transfer. `serve` has transport-isolation flags for controlled development. Never run another daemon alongside the installed service with the same D-Bus name.

## Locations and maintenance

Native Omarchy manages the Git checkout under its plugin directory. The service and Nautilus registration are maintained separately by the panel's setup terminal. Configuration and received data are not code-installation resources. See [Installation](INSTALLATION.md) for source/package modes and ownership rules.

Scope firewall permissions to trusted interfaces and actual ports. Do not expose the service through port forwarding, globally disable the firewall or grant unrestricted system authorization.

## Licenses and corresponding source

| Component | License |
| --- | --- |
| Original `src/`, QML, Nautilus integration, scripts and documentation | [MIT](../LICENSE) |
| Vendored rquickshare core, including modifications | [GPL-3.0-only](../vendor/rquickshare/LICENSE) |
| Google protobuf schemas | [Apache-2.0](../vendor/rquickshare/LICENSE-APACHE), retained file headers |
| Chromium protobuf schemas | [BSD-3-Clause](../vendor/rquickshare/LICENSE-CHROMIUM), retained file headers |
| Locked dependencies | Respective licenses in [THIRD_PARTY_NOTICES.txt](../vendor/THIRD_PARTY_NOTICES.txt) |

The core derives from [rquickshare](https://github.com/Martichou/rquickshare), revision `378d8ae969941bee4bf60ad34ac9cf8bb7005eb7`. The **combined daemon is not MIT-only**. Distributing it carries GPL source and notice obligations; MIT licensing of original components does not relicense the core.

Provide the corresponding source for the distributed build, including the vendored modifications, locked dependencies/build scripts and applicable notices. Source installation retains source material and license notices; Arch package generation produces a source snapshot alongside the package. Regenerate dependency notices with `./scripts/update-notices.sh` when dependencies change.

Quick Share and other names belong to their respective owners. This project is independent of Google, Android and Omarchy.
