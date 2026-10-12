# Implementation Plan: Quick Share for Omarchy

This is the complete development and acceptance plan, not a claim that all phases are finished. Detailed historical results moved to [docs/VERIFICATION.md](docs/VERIFICATION.md); every original requirement and unmet acceptance condition remains in scope. The English-first repository is [PuroDelphi/omarchy-quickshare](https://github.com/PuroDelphi/omarchy-quickshare).

## 1. Objective and scope

Integrate Android Quick Share with Omarchy for file sending and receiving, using a Rust engine and a desktop plugin.

Use a compatible local network when available, work without a shared network through a compatible direct connection, and offer Bluetooth file transfer when no usable Wi-Fi route exists. Bluetooth also serves discovery and connection establishment.

The phone uses its existing Quick Share: no custom Android application, internet connection, external-service account or router is required for local transfers. Intermediate phases are not the complete deliverable.

### Required features

| ID | Feature | Acceptance criterion |
| --- | --- | --- |
| R1 | Rust engine | Discovery, protocol, negotiation and transfers run in a Rust service separate from the desktop. |
| R2 | Native Quick Share | Android sends and receives through its Quick Share interface, without an additional application. |
| R3 | Local network | With a compatible LAN available, file data uses it; Bluetooth data is not forced. |
| R4 | Bluetooth/BLE discovery | Both devices find each other without relying exclusively on LAN mDNS. |
| R5 | Bluetooth data | A real transfer works in both directions with Wi-Fi disabled on both devices. Detecting the phone is insufficient. |
| R6 | Routerless Wi-Fi | Both devices transfer without shared LAN over negotiated Quick Share-compatible Wi-Fi Direct and/or hotspot. Document the mechanism actually supported. |
| R7 | Automatic selection | Negotiate a compatible route, prefer Wi-Fi/LAN for data and use Bluetooth when no usable Wi-Fi alternative exists. |
| R8 | Omarchy plugin | The bar controls visibility, device/file selection, acceptance/rejection, cancellation, progress, result and active transport. |
| R9 | Security | Android-compatible key exchange and encryption; explicit consent before receiving files and code comparison when required by the protocol. |
| R10 | Maintainable installation | User service and plugin install, update and uninstall without changing Omarchy's package-owned files. |

### Limits that must remain visible

- Do not promise every adapter, driver or Android version. Publish a matrix of equipment actually tested.
- Enabled Wi-Fi is not proof of a route; check connectivity and negotiated capabilities.
- Direct connectivity removes the router dependency for that route, not all radio, permission or firewall restrictions.
- Conventional Bluetooth OBEX is not Quick Share. An explicitly separate mode could be added, but cannot substitute for R2/R5.
- BLE discovery and Bluetooth file data are different tasks.
- Automatic resume or lossless mid-transfer migration is not promised; enable it only if protocol support and tests justify it.
- Contacts-only visibility, Google account integration, clipboard, synchronization and other services are out of scope.

## 2. Evidence and technical risks

Reviewed references offer reusable parts, not a complete Linux solution with all transports:

- **rquickshare:** Rust Quick Share implementation; upstream documentation limits files to Wi-Fi LAN and describes Bluetooth announcements for discovery.
- **rustdrop:** describes very experimental Bluetooth and partial BLE discovery.
- **Google Nearby Connections:** encrypted connections and bandwidth negotiation; its README states Linux transports are not implemented.
- **BlueR (`bluer`):** Rust BlueZ access, BLE advertising, GATT and RFCOMM/L2CAP sockets; not a Quick Share implementation by itself.
- **Omarchy:** the inspected installation provides plugin add/enable/disable/update/validate operations and uses Quickshell/QML shell plugins.

Nearby Connections compatibility alone does not prove Android Quick Share compatibility. Verify its actual advertisements, services, messages, security and negotiation.

**Main risk:** complete and validate Bluetooth and direct Wi-Fi against Android. Record concrete blockers on target equipment; do not silently replace them with LAN, OBEX or a companion Android app. Original project components use [MIT](LICENSE); the vendored core and modifications retain GPL-3.0-only except separately licensed protobuf schemas. Preserve upstream notices and corresponding source when distributing the combined daemon under its GPL obligations; it is not MIT-only. See the [component license inventory](README.md#license-and-provenance).

## 3. Architecture

### Engine and user service

Working executable name: `omarchy-quickshare`.

- Async Rust service using Tokio and compatible established protocol/cryptography libraries.
- BlueZ through `bluer`; choose RFCOMM/L2CAP from the observed protocol, not convenience.
- Installed network manager APIs for direct/hotspot connectivity; first identify P2P capabilities and permissions.
- Stream files with bounded memory; neither buffer complete files nor carry content over D-Bus.
- `systemd --user` unit; no routine root execution of the engine.
- User configuration under `$XDG_CONFIG_HOME/omarchy-quickshare/`, defaulting to `~/.config`.
- Receive into the user-selected directory, initially Downloads.

### Desktop plugin

- QML/Quickshell installed under the user's Omarchy plugin directory with stable ID `macondo.quickshare`. Canonical source is the repository-root manifest/QML and `nautilus/`; the daemon remains a separate dependency. Installing only the plugin through a marketplace does not install the daemon or complete setup.
- No protocol execution or cryptographic keys in the UI.
- Show service status, visibility, devices, requests, transport, progress and actionable errors.
- Use a desktop-compatible file chooser, preferably a portal when available.
- Closing the panel does not cancel; cancellation is explicit.
- Shell reload does not interrupt service transfers.

### IPC contract

Use session D-Bus between plugin and service. The implemented service is `io.omarchy.QuickShare`, object `/io/omarchy/QuickShare`, interface `io.omarchy.QuickShare1`; keep compatibility with the installed Quickshell version.

| Operation/event | Required content |
| --- | --- |
| Query status | Actual adapter capabilities, available media and visibility. |
| Change visibility | Public discovery on/off, time-limited visible session and explicit shutdown. |
| Enumerate devices | Internal session ID, displayed name and observed media. Names/BLE addresses are not authenticated identity. |
| Send files | Device and local file references, with access validation. |
| Incoming request | Device, file metadata and verification code when applicable. |
| Accept/reject/cancel | Request/transfer ID and observable result. |
| Transfer update | ID, state, bytes transferred/total, active medium and structured error if any. |
| Restore view | Active-request/transfer snapshot when the plugin reconnects. |

Minimum states: discovered, connecting, authenticating, waiting for acceptance, negotiating transport, transferring, completed, rejected, cancelled and failed. Reflect confirmed medium changes in the UI.

### Transport policy

1. Discover through LAN/mDNS and Bluetooth/BLE when available; combine results only when the protocol supports correlating them.
2. Establish and authenticate a Quick Share-compatible session.
3. Negotiate media actually supported at both ends.
4. Prefer usable LAN; otherwise try compatible direct Wi-Fi/hotspot, then Bluetooth data.
5. Retain Bluetooth as the initial/control channel if protocol negotiation requires it, even when files use Wi-Fi.
6. Bound route-selection/connection timeouts; do not retry a blocked route indefinitely.
7. If a route fails before data starts, try the next protocol-permitted medium without downgrading authentication/encryption.
8. If a route is lost during data, use only supported, tested recovery. Otherwise report failure and allow an explicit new attempt; do not claim resume.

This is the target policy, not a guarantee Android permits every negotiation order. Adapt to the real protocol while retaining R3–R7.

## 4. Execution phases

### Phase 1 — Inventory and preparation

1. Record Omarchy, Quickshell, Rust, BlueZ, kernel, Android and Google Play services versions; identify phone model and Wi-Fi/Bluetooth chipset/driver.
2. Read `omarchy plugin --help`, installed plugins/manifests and existing examples before designing integration.
3. Inspect BLE, Classic Bluetooth, P2P/AP modes and concurrency between normal and direct connectivity.
4. Identify network manager, firewall and effective BlueZ/Wi-Fi management permissions.
5. Prepare at least one Android and one Omarchy computer with Classic/BLE and Wi-Fi. Add another Android model/adapter to broaden compatibility.
6. Prepare reproducible small, large, empty, multiple and Unicode-named files; preserve original SHA-256.
7. Prepare LAN, routerless, Wi-Fi-off and mDNS-blocked scenarios with a way to observe actual file transport.

**Output:** inventory and initial capability matrix. Adapter presence alone does not establish medium availability.

### Phase 2 — Protocol audit and reuse

1. Review licenses, dependencies, activity and APIs of rquickshare, rustdrop and Google/NearDrop references.
2. Select a core after interoperability checks, pin its revision and retain attribution. Reuse compatible code rather than maintain two engines.
3. Document BLE advertisements, service identifiers, LAN discovery, message formats, key exchange, acceptance and media negotiation.
4. Determine the target phones' Bluetooth data transport and direct Wi-Fi/hotspot upgrade flow.
5. Explore on owned devices using observation tools, without recording keys, private content or credentials in shared reports.
6. Confirm LAN, a real Bluetooth session and negotiation without LAN; record each incompatibility with version, message and missing capability.
7. Resolve incompatibilities before declaring the full deliverable viable; specify additional messages/APIs needed.

**Output:** observed protocol map, reuse decision and exact per-transport work. BLE visibility or an open socket does not prove a Quick Share transfer.

### Phase 3 — Rust core and security

1. Create a Rust workspace with protocol core and service executable; keep the plugin outside the core.
2. Implement discovery/device lifecycle without assuming permanent radio identifiers.
3. Integrate messages, key exchange, encryption and verification through maintained libraries; do not invent cryptography.
4. Implement sends/receives, accept/reject, cancellation and explicit states.
5. Validate remote names/sizes; prevent traversal, out-of-destination writes, symlink escapes and silent overwrites.
6. Write destination-local temporaries, publish only after successful completion, and remove partials on cancellation/failure according to documented policy.
7. Bound memory/resources for malformed messages, excessive sizes and disk exhaustion; preserve cancellation.
8. Implement user-session-only D-Bus access, not a network-exposed control API.

**Output:** executable core with real encrypted transfers and safety controls; UI is not required to test this phase.

### Phase 4 — LAN transfer

1. Implement/adapt Quick Share mDNS and IP transport.
2. Check reachable addresses/interfaces; do not mistake VPNs or isolation for usable local connectivity.
3. Test both Android directions, including multiple selection and a large file.
4. Observe traffic/state to prove file data uses LAN, not Bluetooth.
5. Define actual ports and minimal firewall rules; never permanently disable the firewall.

**Output:** R2/R3 demonstrated for tested equipment, providing the other-transport baseline.

### Phase 5 — Bluetooth discovery and data

1. Implement compatible BLE advertising/scanning with explicit BlueZ permissions and controlled visibility. Serialize incoming GATT metadata readers too: do not resume own scanning or advertising until `Device1.Connected=false`. Multiple reads/offsets/readers must retain the gate until the last reader disconnects. Hiding/cancelling is not disconnection and must not interrupt established file sockets.
2. Implement the protocol's data connection through appropriate `bluer`/BlueZ APIs.
3. Resolve pairing/system authorization only when required; distinguish it from Quick Share authentication.
4. Perform key exchange and file protocol over Bluetooth.
5. Turn off Wi-Fi on **both** devices and verify native discovery, acceptance and files in both directions.
6. Compare hashes, test cancellation/disconnection and observe actual medium.

**Output:** R4/R5 demonstrated. Discovery-only leaves this phase incomplete.

### Phase 6 — Direct Wi-Fi/hotspot without router

1. Determine from Phase 2 whether Android advertises/accepts Wi-Fi Direct and/or negotiated hotspot.
2. Implement negotiation messages and network-manager creation/join; do not replace this with manually configuring a shared LAN.
3. Carry credentials only through the protocol-protected channel; do not log them or expose them to the plugin.
4. Resolve addressing/interface selection and file data on the established route.
5. Preserve existing connectivity when concurrent operation is possible; otherwise warn and obtain consent before interrupting it.
6. Release temporary connections and restore previous state on finish/failure, including abnormal exits leaving network-manager resources.
7. With router unavailable and devices initially on different/no networks, transfer both directions and prove Wi-Fi—not Bluetooth—carries data.

**Output:** R6 demonstrated with identified mechanism. A manually created hotspot reusing LAN does not prove automatic negotiation.

### Phase 7 — Automatic selection and negotiation

1. Apply Section 3 policy to verified capabilities, not merely enabled radios.
2. Verify usable LAN preference while Bluetooth can remain discovery/control.
3. Verify direct/hotspot without LAN and Bluetooth without compatible Wi-Fi.
4. Block mDNS to test alternative discovery; independently check whether IP remains reachable.
5. Test client isolation, VPN and rejected connections; avoid loops and explain when no compatible route exists.
6. Test medium loss during data and explicit failure or supported recovery.

**Output:** R7 with selected-medium evidence. UI uses active transport, not requested transport.

### Phase 8 — Omarchy plugin

1. Read an installed-version plugin manifest/pattern; select the definitive ID and validate the real format.
2. Implement bar widget/panel: service state, temporary visibility, devices and file selection.
3. Implement incoming requests with device, files, verification code when applicable and accept/reject buttons.
4. Show byte progress, active transport, cancellation and result; never mark completed before engine confirmation.
5. Add shell request/result notifications without requiring the panel to remain open.
6. Restore the D-Bus view after reload; keep transfers in the daemon.
7. Verify native receiving with panel closed, panel sending, cancellation, reload and coherent state.
8. Add native **Send via Quick Share** in the installed browser (Nautilus in the pilot). One/multiple local files preload exact paths including spaces/Unicode. Preselect a unique device even if discovered later; require a choice for multiple devices. Preserve manual choice while available; clear it if it disappears without redirecting. Sending/code confirmation remain explicit; opening the action does not send or accept.
9. Match installed-shell components, typography, colors and spacing. Visually verify selection, destination, consent, progress, errors and state without losing functions or real transport.
10. Default to English with persistent English/Español. Save in inline `shell.json` plugin settings through the shell API while preserving unrelated fields. Translate panel/bar/project Nautilus menu/new notifications, not names/technical errors. Verify both languages, persistence and reload on real surfaces; fixtures are not Android evidence.

**Output:** R8 visually demonstrated with real transfers, not simulated events. **Current approval:** native surfaces and transfers in the verification record close R8/Phase 8; unrelated transport/remote-cleanup criteria remain open.

### Phase 9 — Installation and operation

1. Package executable, user unit and plugin with explicit dependencies/licenses and reproducible Arch/Omarchy distribution.
2. Install plugin in user space; do not edit Omarchy's package-owned desktop files.
3. Configure graphical-session service startup and D-Bus access; installation must not enable permanent public visibility without consent.
4. Document actual BlueZ/network-management permissions; do not recommend unrestricted polkit rules.
5. Validate/enable/install through installed-version CLI using the actual manifest path.
6. Verify login, upgrade and reinstall without losing preferences/received files.
7. Uninstall by stopping/disabling service and unloading/removing only owned plugin resources; retain inactive inline language/other fields. Keep downloads/configuration unless explicitly requested otherwise.
8. Document build, install, use, diagnosis, compatibility matrix and observed limitations.
9. Install browser integration with both source/package setup, register only an owned user resource and reject foreign file/link collisions.
10. Remove browser integration through the project uninstaller and native `omarchy plugin remove`, even with Nautilus closed. Check install/update/remove/reinstall preserving files/preferences and leaving no functional menu after removal. Document that native removal itself deletes inline preferences; the project uninstaller preserves them.

**Output:** R10 through a complete installation lifecycle. **Current approval:** source/package install/remove/reinstall, published-package update and automatic startup after a real reboot/new graphical login are recorded. Preferences, configuration and recorded received-file metadata were retained; Phase 9/R10 is approved for the observed desktop.

### Phase 10 — Final validation and delivery

1. Execute Section 5 matrix with recorded versions/adapters.
2. Compare source/destination hashes and collect medium evidence for each positive.
3. Run deterministic core tests for malformed input, bounds, transitions, cancellation and safe destinations; these do not replace real Android.
4. Observe plugin/notifications/view recovery with active transfers.
5. Record results/blockers without keys/private data; keep reproducible instructions.
6. Update documentation/published changes, remove temporary exploration tools and do not distribute simulated routes/stubs as completed features.

**Output:** full R1–R10 acceptance for the published matrix. A failed criterion leaves the full deliverable unfinished; scope reduction requires explicit approval.

## 5. Minimum physical-test matrix

Each result records equipment/Android, versions, radio/network configuration, direction, observed medium and outcome. Positive transfers require received-file SHA-256, using a local Android tool or USB/MTP if needed. A verification tool is not a product dependency.

### Preparation and follow-up without repeating approved cases

Stage tests without changing multiple conditions together. Existing approvals are not instructions to resend. Before proposing a new physical action, consult [the verification record](docs/VERIFICATION.md) and name the distinct missing condition. Never disconnect the computer's network or restart its session without explicit coordination.

1. **Android inventory:** model and Android/Play services versions are optional diagnostics, not prerequisites or a numeric whitelist. Use native Quick Share, grant its requested nearby-device permissions and keep the phone unlocked for consent. Limit everyone-visibility to the needed time and turn it off afterward.
2. **Files:** preserve originals and use separate received-copy storage. Include empty, small, 64 MiB, two additional binary and Unicode fixtures. Keep `SHA256SUMS`; do not edit/rename during verification. The pilot prepared six originals and verified their hashes.
3. **Android integrity:** use a local SHA-256 tool or data-capable USB/MTP to independently read Android's stored copy. A Quick Share round trip adds bidirectional evidence but is not an independent hash of the Android copy.
4. **Approved LAN pilot:** retain photo/small/binary/Unicode/batch/round-trip results. Do not repeat basic direction or arrival checks. Explicit code comparison still applies to genuinely new cases.
5. **Approved files/UI:** small, batch, Unicode, large, closed-panel notification, rejection, LAN cancellation and active reload are recorded. Do not repeat. Empty-file native failure and Android partial after physical cutoff remain blockers; do not conceal them with copies or rerun unchanged failures.
6. **Bluetooth:** first enumerate an actual Classic/BLE controller; BlueZ/radio switches are insufficient. Intermediate isolation uses phone Wi-Fi off and temporary `serve --no-lan --no-hotspot`, observing capabilities/transport while retaining computer connectivity. Remove isolation afterward. It does not replace both-Wi-Fi-off bidirectional/hash/cancel/disconnect acceptance. Verify incoming GATT readers block role changes until `Connected=false`, including hide/restoration. Neither OBEX nor discovery substitutes for data.
7. **Routerless:** Wi-Fi/Bluetooth available without shared LAN; do not manually create a hotspot to fake negotiation. Save network-dependent work; obtain consent for AP interruption. Check negotiation, Wi-Fi data and previous-network restoration.
8. **Isolation/offline:** control router/guest network to independently block mDNS, isolate clients or remove WAN. Change one condition at a time with a reversal plan, not a global firewall disable.
9. **Installation:** save work before logout/restart. Perform administrative package authorization locally; do not share passwords. Check update, fresh login and uninstall preserving preferences/files.
10. **Approved Nautilus:** single-file and native two-file batch sends, exact space/Unicode names, explicit destination/code and independently matching Android batch hashes are recorded. QML/Wayland covers preselection changes without automatic sending. Do not ask for another basic menu batch; native empty-file failure is a separate case.

For unchanged received names, use `sha256sum --check` with the original manifest from the received directory. If collision avoidance adds a prefix, compare to the corresponding original without deleting/renaming the original to make a test pass.

Record results when observed: direction, radio/network conditions, confirmed medium, sizes and both-end hashes. `completed` alone does not certify publication/integrity. Investigate a failing case rather than hide it by moving to another transport. Independent missing hashes may be obtained from existing files, without new sends.

### Cases and acceptance

| Case | Condition | Required outcome | Current evidence/remaining condition |
| --- | --- | --- | --- |
| Bidirectional LAN | Same compatible network, Bluetooth available | Native Quick Share both ways, LAN data | Native basic LAN approved; retain exact recorded radio conditions. |
| LAN without Bluetooth | Same network, Bluetooth off | Determine Android's actual discovery/connection dependence | Computer backend absent was observed; both-device Bluetooth-off condition is not established. |
| No internet | Local LAN/direct route without WAN | Files without external service | Controlled WAN-off condition unverified. |
| No router | No shared LAN, Wi-Fi/Bluetooth available | Automatic direct/hotspot negotiation, both-way files | Local hotspot creation/restoration only; native Android negotiation open. |
| Bluetooth only | Wi-Fi off at both ends | Discovery/data both ways and correct hashes | Native 64 MiB in both directions with both Wi-Fi off approved: Bluetooth data/no errors, USB only for Internet, independent Android copy/source SHA-256 and byte equality. Incoming duplicate name preserved the original PC file; incoming active-data cancellation removed staging without publication. Outgoing cancel, disconnect, outgoing batches and other specific historical hashes remain open. |
| Blocked mDNS | Existing LAN, Bluetooth available | Alternative discovery, compatible route without exclusive mDNS dependency | Controlled physical case open. |
| Isolated LAN | No inter-client IP connectivity | Do not treat LAN as usable; try a compatible alternative | Controlled physical case open. |
| No route | Necessary radios/transports unavailable | Clear error, no fake transfer or infinite wait | Local backend/error evidence exists; complete physical matrix open. |
| Acceptance/rejection | Incoming request, panel closed | Notification/consent; rejection without publication | Native notification and LAN/Bluetooth rejection approved for recorded cases. |
| Files/names | Empty, small, large, multiple, Unicode | Intact content, bounded memory, safe paths | Nonempty native cases approved; empty native send failed, not waived. |
| File-manager menu | One/multiple local files, spaces/Unicode | Exact preload, unique preselection/manual multi-choice, explicit send/code; safe install/removal | Native single/multi and UI/lifecycle evidence approved. |
| Conflicting destination | Existing name, unwritable/full destination | No silent overwrite/directory escape; explicit result | Existing-name preservation approved; unwritable/full physical cases open. |
| Cancel/disconnect | Real transfer in progress | Coherent state, resources released, managed partials | LAN cancel both directions approved; physical cutoff detected but Android partial cleanup failed. Other scenarios open. |
| Shell reload | Transfer active | Daemon continues, plugin restores state/real medium | Native 64 MiB LAN and independent Android hash/visual recovery approved. |
| Installation cycle | Install, fresh login, update, uninstall | Functional integration; system/user files preserved | Source/package install/remove/reinstall, update to `0.1.0-4` and automatic startup after a real reboot/new graphical login approved; preferences/configuration and recorded received-file metadata preserved. |

See [docs/VERIFICATION.md](docs/VERIFICATION.md) for complete historical positives, failures, corrections, evidence boundaries and camera metadata/hash mismatch. R8 and R10 approval does not close R5/R6/R7 or remote cleanup.

## 6. Order and completion criterion

Inventory and interoperability audit first; then core, LAN, Bluetooth data, routerless Wi-Fi; finally integrated negotiation, plugin, packaging and combined validation.

The first main-risk-reducing milestone is **a real Quick Share transfer without a shared network**. Full support additionally requires Bluetooth data with Wi-Fi off and network preference when available.

A bar icon, BLE discovery, LAN-only demo or companion Android application is not a finished product. No unmet criterion is removed by this publication or translation.

## 7. References

- [rquickshare implementation and limitations](https://github.com/Martichou/rquickshare)
- [rustdrop experimental Bluetooth and partial BLE](https://github.com/bagel897/rustdrop)
- [Google Nearby Connections negotiation/media/Linux status](https://github.com/google/nearby/tree/main/connections)
- [BlueR Rust BlueZ/BLE/RFCOMM/L2CAP API](https://docs.rs/bluer/latest/bluer/)
- [NearDrop interoperability reference](https://github.com/grishka/NearDrop)
- [Omarchy](https://omarchy.org/) and [plugin publication requirements](https://plugins.omarchy.org/publish.html); compare CLI/manifests/examples against the installed version.

Review and pin references as development proceeds. Declared capabilities never replace native Quick Share observations.
