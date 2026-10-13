# Recorded verification

[Back to Quick Share](../README.md) · [User guide](USER_GUIDE.md) · [Architecture](ARCHITECTURE.md)

This record describes observed native Android and desktop behavior. Radio/backend availability and local simulated peers are distinguished from real file transfers. It contains no verification codes, credentials, private device identifiers or personal paths.

## Native file sharing

Recorded desktop environment: Omarchy 4.0.4, Quickshell 0.3.1, BlueZ 5.87 and Linux 7.2.5. The native Android peer was a Pixel-family phone using its installed Quick Share. Numeric Android versions are diagnostic metadata, not an admission rule.

| Case | Direction and transport | Observed outcome |
| --- | --- | --- |
| Small files and Unicode names | Omarchy ↔ Android, LAN | Native completion; returned content/names and hashes matched |
| Multi-file selection | Omarchy ↔ Android, LAN | Native grouped files; safe duplicate-name prefixes preserved original PC files |
| Large file | Omarchy ↔ Android, LAN | 67,108,864 bytes each direction; SHA-256/byte comparison matched; Android stored copy independently read over USB/MTP |
| Active shell reload | Omarchy → Android, LAN | 64 MiB completed without replacing the daemon instance; destination matched source |
| Native video receive | Android → Omarchy, Bluetooth | 44,501,859 bytes completed; Classic RFCOMM observed; full received video/audio decoding succeeded |
| Native photo receive | Android → Omarchy, Bluetooth | 392,114 bytes completed; independent Android SHA-256 matched |
| Native text batch | Android → Omarchy, Bluetooth | Two files, 309 bytes, completed; user confirmed correct content |
| Both Wi-Fi radios off, large send | Omarchy → Android, Bluetooth | 67,108,864 bytes completed, no error; new Android copy independently matched SHA-256 and bytes |
| Both Wi-Fi radios off, large receive | Android → Omarchy, Bluetooth | 67,108,864 bytes completed; new `1_grande-64MiB.bin` matched independently read Android source and preserved the PC original |
| Incoming active-data cancellation | Android → Omarchy, Bluetooth | Cancelled at 574,200 of 7,561,800 bytes; no published video or leftover local staging |
| Incoming physical Bluetooth cutoff | Android → Omarchy, Bluetooth | Failed at 2,035,440 of 36,372,062 bytes with `Connection reset by peer (os error 104)`; no publication or leftover local staging |

The independently verified 64 MiB payload SHA-256 is `b9e838f0fa17dd4b381a64b6ccc2056d9956e4f0046048f10abb58186cde9b8e`. Source/destination comparisons returned `cmp` exit 0.

For the both-Wi-Fi-off cases, the phone's radio state was confirmed by the user and the computer's off state was recorded at completion. The engine ran `serve --no-lan --no-hotspot`; USB provided Internet for coordination, not Quick Share file data. The new Android receive's timestamp followed that transfer's completion rather than identifying an earlier LAN copy.

For the physical incoming Bluetooth cutoff, the user confirmed disabling phone Bluetooth while data was arriving. The terminal snapshot, nonzero partial byte count, isolated execution and filesystem checks are the evidence; a continuous pre-cut observer and a new HCI capture were not used for that attempt. Acceptance was not automated. Explicit PIN comparison is required by the usage guide but is not claimed as independently observed for every recorded run.

## Desktop integration

- A native incoming request produced a notification with the panel closed; the user handled consent.
- Nautilus single/multi-file selections preloaded the native panel and completed real sends. Opening the menu did not automatically send or accept.
- A sole discovered destination was preselected; multiple destinations and disappearing manual selections were exercised without redirecting a selection.
- English/Español, theme-integrated controls and real QML/Wayland rendering were exercised. Progress cards retain their delegates by transfer ID so pointer presses are not discarded when snapshots update.
- Incoming rejection and cancellation did not publish files. Local staged files were removed by storage rollback, not by manually deleting evidence.

## Service and installation observations

Source/package setup, package update, service removal/reinstallation and automatic startup after a real reboot/new graphical login were exercised. Configuration and recorded received-file metadata were preserved. Service startup was observed before invoking a D-Bus client, avoiding a false autostart result caused by client activation.

After isolated Bluetooth checks, the owned runtime overrides were removed, normal installed execution restored, all three backends returned ready with no engine errors, visibility was off, and computer-Wi-Fi-bound HTTPS returned 200. Original/reference files and the configuration hash remained intact.

Local NetworkManager checks created an AP and restored the previous connection while removing owned profiles. This observation describes local network-resource management, not a separate Android transfer result.

## Native Git lifecycle and service controls — 0.1.1

The native Omarchy add/enable, fast-forward update and remove commands were exercised against a local Git origin and the actual SDK plugin registry. The checkout retained `.git` through backend maintenance; native removal unloaded the widget and removed its checkout.

Source service installation, repeated installation, update and removal ran against the real 0.1.1 executable and a private session bus. Executable, unit/drop-in, source, notices and Nautilus registration were inspected. Foreign or modified resources were refused without replacement or stopping the running daemon. Declined removal was inert; authorized removal removed owned resources while preserving configuration, language and download witnesses.

The actual QML panel was rendered in English/Español with missing, older, compatible and disabled backends. An older 0.1.0 executable displayed **Service update required**, not **Ready**. Spanish persisted through native removal/re-addition and component reload even after the inline shell entry had been removed. Maintenance controls and scrolling were inspected at 1366×768.

The 0.1.1-1 Arch package was built using the disk-backed temporary workspace, extracted and executed on a private D-Bus session. Its embedded corresponding source matched the separately generated source archive byte-for-byte; licensing and detailed documentation were present. Obsolete copied-plugin setup/removal wrappers were absent.

These local lifecycle scenarios used isolated homes and network namespaces. Process-control adapters managed real daemons without modifying the host systemd manager; package authorization boundaries used adapters rather than host package changes. Isolated NetworkManager errors remained visible. The observations establish lifecycle and UI behavior, not a new Android transfer or transport-readiness result.

## Private file-selection handoff — 0.1.2

The locked workspace build and deterministic tests completed with **74 tests passed** and one consenting physical SDP test ignored. Regression cases cover stdin JSON type/count/byte limits, rejected positional file-path arguments, isolated one-shot selections, monotonic expiry, capacity and preparation without transfer actions.

An isolated home, session D-Bus and network namespace exercised the actual Rust client/daemon, Nautilus provider activation with `Gio.Subprocess`, Quickshell processes and native QML/Wayland panel. Unicode, quotation marks and newline-containing names arrived as exact canonical paths through stdin and the token handoff. Send and destination-folder commands reached the real D-Bus methods; an unavailable synthetic peer was rejected without starting a transfer. **369 process command-line samples** contained no selected path/name, including JSON-escaped forms.

Single/multiple synthetic device models retained their selection rules. Delayed process adapters exercised overlapping imports, stale picker results, picker cancellation and an earlier send completing after a newer selection; they are not file-transfer evidence. Invalid tokens cleared stale selections. Shell spawn, nonzero exit, invalid UTF-8 and unexpected replies discarded prepared selections and preserved all available slots.

The new panel was also rendered against the actually installed 0.1.1 executable on a private bus: it showed **Service update required**, rejected file-selection retrieval with an update message and kept an empty selection without starting a transfer.

No phone transfer, global procfs permission change, production-service maintenance or preview/screenshot modification was part of these privacy checks. The same-user session and root remain trusted boundaries, not a process sandbox.

## Evidence interpretation

Successful transfer means engine completion plus the stated integrity method. A valid media container, a round trip, a capability label or 0/0 bytes alone is not independent destination integrity.

Android owns its receive/cache lifecycle. Omarchy's incoming staging rollback is local; sender-side cancellation does not establish remote-file deletion. Empty native offers can be rejected by Android. These operational behaviors are described in the [user guide](USER_GUIDE.md) and [architecture](ARCHITECTURE.md), not disguised with modified payloads.

A camera original read through USB and an Android share-provider export can differ in location metadata despite matching media data. The comparison is not presented as complete-file equality, and private coordinates are not published or reconstructed.

Locked-source checks run in [GitHub Actions](https://github.com/PuroDelphi/omarchy-quickshare/actions). Verification results are scoped observations, not an external security certification or a claim that every radio/platform combination behaves identically.
