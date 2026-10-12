# Verification Record

This English record consolidates the original README and implementation-plan history together with subsequent publication and lifecycle checks. It preserves approved cases, earlier failures, corrections and genuine blockers while omitting private device/session identifiers, personal filenames, network identifiers, paths and private artifact links. Each observation is limited to its recorded scope.

The full scope and acceptance matrix are in [IMPLEMENTATION_PLAN.md](../IMPLEMENTATION_PLAN.md). Basic native sending/receiving, the approved UI and Nautilus cases must not be repeated just to reconstruct a receipt. Missing independent hashes can be obtained from existing files; a new physical test must identify a distinct unmet condition.

## Recorded environment and evidence boundaries

- Desktop: Omarchy 4.0.4, Quickshell 0.3.1; local Linux/BlueZ investigation used kernel 7.2.5, BlueZ 5.87 and BlueR 0.17.4.
- Bluetooth controller: Realtek RTL8821C, Classic and BLE. Wi-Fi supported AP but did not advertise P2P or AP+station concurrency.
- Android pilot was described as a Pixel 7; later USB identification reported Pixel 7a. This diagnostic discrepancy is retained rather than turned into two independently tested models. Exact Android/Google Play services versions were not recorded; the user reported an updated Android.
- Normal operation uses native Android Quick Share, not OBEX or a custom Android application. The phone user's confirmations are retained as content/arrival evidence, not substituted for SHA-256.
- The computer stayed connected to Wi-Fi during documented Bluetooth isolation and most physical checks. Backend LAN/hotspot disable flags isolate the engine, not both devices' radio state.
- PIN comparison/acceptance/cancellation remained manual. No verification code or credential is published here.
- Earlier source/package lifecycle observations used installed package `0.1.0-2`. The published layout/branding update to `0.1.0-4` and subsequent real reboot/new-login startup were also verified, using `/usr/bin/omarchy-quickshare`; the previous source executable remains absent.

## 1. Native LAN files and non-overwrite

| Case | Observed result | Integrity/evidence boundary |
| --- | --- | --- |
| Initial receive photo | Android → Omarchy, `completed`, 392,114 bytes | Original historical download was absent when checked later; historical state alone is not publication proof. A later independently hashed photo is recorded below. |
| Initial sends | Omarchy → Android, 49 bytes and an earlier 164-byte send completed | Earlier code match physically confirmed; do not extrapolate missing destination hashes. |
| Small round trip | 104 bytes each way, LAN, `completed`, no errors | User confirmed code/arrival. Returned file `0600`, SHA-256 and `cmp` matched source; independent Android stored-copy hash was not obtained for this case. Computer Bluetooth backend unavailable does not prove Bluetooth off on the phone. |
| Three-file batch | Omarchy → Android, 135,229 bytes total, LAN, `completed`, no errors | Two binary fixtures and Unicode text; user confirmed arrival/content/names. Returns verified below; independent Android hashes of that original batch remain unrecorded. |
| Unicode/text return | Android → Omarchy, 213 bytes total, LAN, `completed`, no errors | Three published `0600` files. Unicode name/content retained; conflicting text names gained `1_` prefixes, original files remained intact. Returned source hashes and `cmp` matched. **Non-overwrite is approved**, not pending another test. |
| Binary return | Android → Omarchy, 4,096 + 131,073 = 135,169 bytes, LAN, `completed` | Both `0600`, source/return hashes and `cmp` matched. Completes round-trip evidence for the earlier three-file batch, not all transport cases. |
| Large round trip | 67,108,864 bytes each way, LAN, `completed`, no errors | Returned `0600`; SHA-256 and `cmp` matched. Actual panel showed completed/LAN/64 MiB. Independent Android hash was subsequently obtained during active-reload testing. |
| Initial LAN rejection | 104-byte offer, `waiting_acceptance` → `rejected`, zero received bytes, no error | Existing originals unchanged; no new file. Panel had been closed; the observer screenshot failed because its subprocess lacked graphical environment. This initial receipt proves rejection/non-publication, not the notification button's appearance by itself. |

### Preserved reproducible-fixture hashes

These are fixture/content receipts, not personal filenames or identifiers.

| Fixture | SHA-256 of source and matching Linux return |
| --- | --- |
| Small 104-byte text | `d6ea0ba8f8af4009f3eb38d084fe050fb25dfcfc8ada2d34360b8c98895488d1` |
| Unicode text | `501c059abcd7b2b05677de0c6645dca9dd5d63922c567a0f116b6bd4a12d4676` |
| 4,096-byte binary | `213927ff176c0c6cc14665ded6ccccd3edbbdb6aed3c14611e4d5018a36e3324` |
| 131,073-byte binary | `a8d4f6663b3fa6beeac4994201de116bbb5b7f4c4e64e72c9231fbf96b8195ca` |
| 64 MiB binary | `b9e838f0fa17dd4b381a64b6ccc2056d9956e4f0046048f10abb58186cde9b8e` |

### Native empty file: failed, not waived

A zero-byte Omarchy → Android LAN send ended `failed`: Android sent `Disconnection/6` during `Consent`, before transfer confirmation. `0/0` is not success. The sender announces the real size (`Some(0)`) and does not change content.

The pinned [Google Nearby `IncomingShareSession::ProcessIntroduction`](https://github.com/google/nearby/blob/d128b5fca30b3fa8d4642f8c75dd9e314e090e74/sharing/incoming_share_session.cc) rejects `file.size() <= 0` as `kUnsupportedAttachmentType`. **[INFERENCE]** This is consistent with the observed Android close, but does not establish that private GMS version's internal reason. No ZIP, padding, altered content or companion application silently substitutes for the native empty-file criterion. It remains unapproved pending specific Android evidence or an explicitly authorized scope change.

## 2. Bluetooth discovery, failures and native file data

### Radio coordination investigation

Earlier RTL8821C runs showed HCI scan-disable timeouts (`0x200c`, `-110`) and automatic kernel/USB resets. Serializing advertising and scanning alone did not prevent every reset. A BlueZ monitor later observed alternating approximately 8-second advertisements and 4-second scans, with no own active advertisement before scanning and no discovery before advertising. Visibility expiry removed advertising and resumed hidden scanning; the service/profile stayed available and no reset occurred in that observation window. A 120-second run without incoming readers/files was **not** native interoperability proof.

A later Android metadata read was followed by own scanning while its BLE reader remained connected and then a firmware timeout/reset after disconnection. `LE Read Supported States` returned `0x00000000ffffffff`, without bits 35/38 for connectable advertising alongside LE central/peripheral connections. The [Linux kernel rejects that legacy advertising combination](https://github.com/torvalds/linux/blob/master/net/bluetooth/hci_sync.c). **[INFERENCE]** This restriction is consistent with an earlier advertising `Invalid Parameters`/`0x0d` rejection whose exact HCI sequence was not captured. It does not establish the firmware timeout's cause.

The gate now tracks incoming metadata readers until `Device1.Connected=false`; multiple reads/readers, hiding and cancellation cannot prematurely release it. It defers role transitions/own scans, does not replay missed ticks, disconnect foreign readers or reset the adapter. Regression coverage includes concurrent readers, repeated ATT reads, hide and deadlines.

A separate controller disappearance was diagnosed as USB-controller resume failure (`HC died`), not a manual radio disable. Authorized targeted USB-function re-enumeration restored Bluetooth and camera without changing the Wi-Fi device or rebooting the PCI bridge/computer. This was recovery, **not a persistent suspend fix**. After restarting the service with no active transfers, its backend reported Bluetooth ready and no errors during the observed interval.

### Native Bluetooth reception

With phone Wi-Fi prepared off and temporary engine `serve --no-lan --no-hotspot`, Android → Omarchy completed a **44,501,859-byte video** with exact byte count and no transfer errors. Capture showed Classic RFCOMM (BR-ACL/L2CAP PSM 3), not IP. Two native 52-byte GATT reads retained the gate until each remote disconnect, then scan restarted. Visibility expiry did not interrupt file data.

Published file was `0600`, exact size, HEVC 1920×1080/AAC; subsequent full video/audio decoding succeeded. Temporary isolation/capture was removed, normal LAN/Bluetooth/hotspot capability restored without errors, and computer Wi-Fi/download retained. This proves reception and observed radio ordering, not both-Wi-Fi-off acceptance or universal firmware stability.

### Outgoing connection race and native send

The first inverse isolated attempt failed before code confirmation/transport establishment: zero of 104 bytes, `ENOTCONN` 107. A physical BlueR 0.17.4 reproduction preserved stale writable readiness on a bound, not-connected socket: `connect` returned in approximately 45 µs with `SO_ERROR=0`, while kernel `poll(POLLOUT)` did not show completion; first SDP send failed 107. `getpeername` did not prove completion either.

SDP/RFCOMM now start `connect` on the owned descriptor after deregistration and only register reactor readiness after connection begins, waiting for kernel completion/error. No duplicated descriptor, sleep workaround, data retry or adapter reset. The consent-required, default-ignored physical regression `sdp_connect_waits_for_kernel_after_cached_writable` forced that ordering and received native Nearby SDP channel 11. An SDP reply is not file delivery.

After correction, native Omarchy → Android sent the **104-byte fixture over Bluetooth**, `completed`, no errors, with engine LAN/hotspot disabled. The code was presented and acceptance was not automated. User confirmed correct received files; independent hash of that Android destination remains unrecorded. Normal service restored, visibility off, capabilities available/errors empty, computer Wi-Fi retained. Do not ask the user to repeat the already approved visual inspection.

### Closed-panel native notification and independent photo hash

With the panel closed and DND off, Android produced an actual **Quick Share request** notification. The observer only read state/captured a redacted heading; the user handled consent and confirmed completion. Bluetooth receive completed **392,114/392,114 bytes** without error.

Published `0600` JPEG was 1376×768 and fully decoded. Independent Android original read by USB/MTP matched size, `cmp` and SHA-256:

`580341b0abd54ade010fd75750018734ca2462b24ddd67ac665bf976757bd6dd`

No USB-debugging settings changed. Computer Wi-Fi remained connected; phone Wi-Fi state was not independently established. Hotspot upgrade correctly warned that network-change consent was absent; Bluetooth still completed. This approves native closed-panel notification/consent/reception, not routerless Wi-Fi.

### Existing camera video and location metadata

The existing camera file was read through USB/MTP without another send or edits. Both files are **44,501,859 bytes**, but their **full hashes do not match**:

| Existing copy | Full-file SHA-256 |
| --- | --- |
| Quick Share received video | `335d1fa45b80b633724a8a10e1ba46b780e5e8ca3e5096ed0ab2ff9521344be7` |
| Android camera original | `204c83fd2069c585720471e16c5546f40ae61182b33ed62e554f16dbb1420bc7` |

Top-level MP4 box offsets/sizes matched. All audiovisual `mdat` bytes/hash matched; only **23 bytes** differed in a **28-byte location box** under `moov/udta`. Android retains type `©xyz`; type/content are zero in the received copy. No coordinates were exposed or reconstructed.

**[INFERENCE]** This is compatible with location redaction by an Android provider/exporter, but does not establish GMS internals or the exact provider bytes sent. It does not dispute the user's confirmation that the video is correct. Whole-file SHA-256 equality against the camera original remains **unapproved**. Do not patch metadata or silently resend to manufacture equality.

## 3. Shell reload and visual recovery

A native Omarchy → Android LAN **64 MiB** send was active at 65,536/67,108,864 bytes when `omarchy restart shell` ran. Engine instance/process remained unchanged and the send completed all bytes without error. Independent USB/MTP Android read matched source SHA-256 (`b9e838f0fa17dd4b381a64b6ccc2056d9956e4f0046048f10abb58186cde9b8e`) and `cmp`. Computer Wi-Fi remained connected, code acceptance manual.

The observer failed when querying `org.freedesktop.Notifications` during temporary ownership absence, not because transfer failed. Notifications subsequently had a new owner; the native panel reopened and the user confirmed **Completed · 100%**. This approves continuity and visual recovery without repeating the send. Shell ping readiness is not notification-service readiness; future observers should wait on the service condition/event rather than infer it from restart return.

## 4. Cancellation, rejection and abrupt disconnection

### Earlier cancellation failure and correction

During a LAN 512 MiB send, manual Cancel ended `cancelled` at **64,618,496/536,870,912 bytes**, socket closed, engine/computer Wi-Fi retained. The user needed multiple clicks and USB/MTP showed a **64,618,496-byte public Android partial**. This is a recorded failure; the file was not deleted to hide it.

UI cards now retain delegates keyed by transfer ID instead of reconstructing the cancel button on every progress snapshot. A first click shows **Cancelling… / Cancelando…** and blocks duplicate requests for that transfer without fabricating terminal completion. Real QML/Wayland smoke held a click across eleven updates, exercised both languages, terminal reset and a real CLI failure releasing pending state.

The engine retains the queued encrypted frame/cursor/sequence even if a write future was never polled, completes at most that frame before controls, sends active-file `PAYLOAD_CANCELED` with ID/offset and Sharing `CANCEL`, then allows up to five seconds for remote processing/disconnection (EOF can end earlier). Terminal publication follows resource/lease cleanup; signaling/restoration errors remain explicit. A real daemon/CLI on private D-Bus with controlled localhost peer decoded authenticated `CANCEL` before consent, ending cancelled without files/code acceptance. That is local protocol evidence, **not** Android cleanup evidence.

Pinned Google references: [Sharing CANCEL](https://github.com/google/nearby/blob/d128b5fca30b3fa8d4642f8c75dd9e314e090e74/sharing/share_session.cc#L299-L308), [payload cancellation/window](https://github.com/google/nearby/blob/d128b5fca30b3fa8d4642f8c75dd9e314e090e74/sharing/nearby_sharing_service_impl.cc#L903-L986), [receiver file cleanup](https://github.com/google/nearby/blob/d128b5fca30b3fa8d4642f8c75dd9e314e090e74/sharing/nearby_sharing_service_impl.cc#L3145-L3161). They do not prove private GMS's cause or deletion in any individual run.

### Corrected native LAN cancellation: approved for observed runs

- **Outgoing:** new 512 MiB fixture, real data observed from 65,536 bytes; manual cancellation ended `cancelled`, **28,508,160 bytes**, no error. Engine unchanged, socket closed, no active transfers, capabilities available, computer Wi-Fi retained. User reported it looked correct after the one-click instruction; no instrument counted physical clicks.
- **Public Android cleanup:** initial unavailable/timed-out MTP was not treated as deletion. After functional MTP mount, both enumeration and GIO size listing showed only the old historical partial, **not the new fixture**. No Android file was deleted and private cache was not inspected. Absence in the public receive folder is approved for this run, not every transport or abrupt disconnect.
- **Incoming:** an intended rejection test instead accepted then cancelled a LAN video; `waiting_acceptance` → `transferring` → `cancelled`, **38,273,024/44,501,859 bytes**, no error. User explicitly confirmed accepting then cancelling. No new final/local partial; prior video hash unchanged. Engine/Wi-Fi retained. Observer exit 2 came from expecting rejection/zero bytes, not an engine failure. Incoming cancellation/local cleanup approved for this run, rejection separately below.

### Native incoming Bluetooth rejection: approved

A **7,561,800-byte** offered video went `authenticating` → `waiting_acceptance` → `rejected`, zero bytes and no error; it never transferred. Observer completed normally, no automated consent/PIN recording. Before/after Downloads entries unchanged, no offered file or `.omarchy-quickshare-*.partial`. Engine unchanged, hidden, no active transfers/errors, capabilities available; computer Wi-Fi connected. This is rejection/non-publication proof, not phone-Wi-Fi-off proof.

### Abrupt disconnection: explicit failure, remote cleanup failed

1. An earlier incoming Bluetooth cutoff reached `transferring` but stayed at zero of **7,561,800 bytes**, then failed `Connection reset by peer (os error 104)`. User confirmed disabling phone Wi-Fi/Bluetooth caused the error. No new local final/partial, engine/network retained. Observer exit 2 correctly meant no data-in-motion observation; this did **not** satisfy cutoff-during-data acceptance. Missing hotspot consent warning stayed visible and was not bypassed.
2. First outgoing LAN 512 MiB attempt failed `Disconnection/6` in Consent, zero bytes, before a cutoff prompt. User requested a repeat because instructions were unclear; no automatic retry occurred.
3. The explicitly authorized repeat observed data from **65,536 bytes**, issued the cutoff notification, then ended `failed`, **90,374,144/536,870,912 bytes**, `deadline has elapsed`. User confirmed seeing the prompt then turning off both phone radios. Engine unchanged, session retired, no active transfers; computer Wi-Fi retained. This proves cutoff detection/explicit error/resource release, **not remote cleanup**.
4. Functional USB/MTP and GIO showed a new public Android partial of **88,735,744 bytes** alongside the older cancellation partial. A real read produced SHA-256 `ba69f1468489b317bedff1c9ab9774273bd97d394d3908485900552b15624ef0`; it was not merely stale metadata. Sender count and Android published bytes need not match and do not establish integrity/completion. No Android file was deleted.
5. A TCP `FIN-WAIT-1` entry initially remained without listed process owner, then was absent after the partial read. Own observer/fixture were removed; no fake remote deletion/reconnect signaling was added.
6. After the user restored phone radios and reopened Quick Share, GIO still listed the same partial. A new direct GIO/MTP read, separate from the prior FUSE read, produced the same hash. No deferred cleanup was observed.

The implemented Sharing schema provides no remote-file deletion operation. Cancel signaling needs a usable channel after the pending frame; radio cutoff removed that route. Local `IncomingFiles::Drop` cleans Omarchy temporaries, not Android storage. Google's [non-Complete receiver cleanup reference](https://github.com/google/nearby/blob/d128b5fca30b3fa8d4642f8c75dd9e314e090e74/sharing/nearby_sharing_service_impl.cc#L2398-L2449) does not establish private GMS behavior. Remote cleanup remains a real failed acceptance condition, not silently narrowed to local resource release.

## 5. Nautilus and bilingual UI

- Native action preloaded exact Unicode and six-file selections without sending. After the real Nautilus process loaded the extension, a **104-byte single-file LAN send** completed and user confirmed arrival.
- A later real **two-file batch**, one spaced name and one Unicode name, completed **309/309 bytes** over LAN. User confirmed both files correct; direct USB/MTP reads retained names/sizes and independently matched source hashes:

| Fixture | Bytes | SHA-256 source = Android |
| --- | ---: | --- |
| Spaced-name text | 155 | `5c1798c763c243c6a2c8416cbae104f82bfbfc4a10d62f136aa6155a1aee662c` |
| Unicode-name text | 154 | `23202a04e86adad78859646b94d8176f5199e8b404c8f55c6819ae7e735a2cb0` |

- Real QML/Wayland exercised zero/one/multiple destinations, late discovery, manual selection/disappearance and new file selections without automatic sending. A manual destination is cleared on disappearance, not redirected.
- Panel uses installed `Ui` components and `Color`/`Style` tokens, not a private palette. English default and persistent English/Español selector were exercised on the real panel; shell inline preferences and unrelated fields were preserved. New Nautilus providers showed both labels in private windows without closing existing windows; new notifications used explicitly marked preview requests. PIN/progress/Unicode fixture cards are UI evidence, not physical Android evidence.
- During a redundant follow-up preparation, Android returned **both** texts over Bluetooth, `completed`, 309 bytes, no error; user again confirmed correct sends. An observer filtered for only one file and matched no events, then was interrupted with exit 130 on user request. That is **not product failure**. Downloads was restored, only own fixtures removed, no Android deletion/cancel. Later active visibility was preserved rather than overwritten from an earlier observer snapshot.

**R8/Phase 8 is approved** by actual native notification, consent, single/multi-menu sending, cancellation, rejection and active-reload recovery, together with explicit UI evidence. It does not certify untested network conditions, complete installation acceptance or Android cleanup after physical cutoff. Do not repeat approved basics.

## 6. Lifecycle and retained preferences

Source uninstall/native plugin removal were exercised: owned Nautilus link disappeared; `share` rejected an absent plugin. Preferences, two foreign extensions and twelve original/received files retained hashes/size/mode/owner/mtime (15 recorded entries). Invalid empty/truncated/extra-content ownership markers were rejected before resource changes. Native new-selection menu omitted Quick Share and preserved another provider; cached labels could linger without authorizing sends. A real service removed its link after plugin rename with no Nautilus instance using that isolated registry and stayed responsive on D-Bus.

Real Arch package install, administrative `pacman -R`, reinstall and corrected removal were performed with local authorization, preserving shared dependencies and computer connectivity. The first removal exposed an Omarchy API behavior: `PluginRegistry.setEnabled(false)` deletes the entire bar entry, including Spanish preference. The project uninstaller was corrected to preserve `shell.json`, stop/disable service, remove only marked plugin/owned link, and restart shell to unload it. Plugin absence made `share` return `unknown`; the saved entry is inactive preference data, not a functional installed plugin.

Corrected uninstall/reinstall preserved the full inline JSON/Spanish, witness file and `settings.toml` bytes/mode/owner/mtime, and preexisting Downloads metadata. Nautilus registration returned, normal service active, no transfers, capabilities available. Package supplies `omarchy-quickshare-uninstall` without a checkout. Native Omarchy removal deletes inline preferences; preference preservation belongs to the project uninstaller.

### Published-package upgrade and real automatic startup: approved

The owner authorized installation of `0.1.0-4` and ran its installed setup locally. The installed client reached the real user service; Spanish and the owned Nautilus link remained intact. The native panel displayed **Ready / Listo** and **Powered by Jhonny Suarez - PuroDelphi**.

The subsequent observation had a **different Linux boot identity** and a new graphical-login timestamp. Before invoking the CLI or opening the panel, passive `systemctl --user show` reported the service **active/running/enabled**, executing `/usr/bin/omarchy-quickshare serve`, with a new process and invocation. It started at the same second as `graphical-session.target`; no manual service start, restart or setup was performed after return. This is actual reboot/new-login evidence, not a shell-reload substitute or a lingering old daemon.

The installed CLI then reached a new engine instance, visibility off, zero transfers, `errors: []`, and LAN/Bluetooth/hotspot backend availability. Opening the real Spanish panel did not change the passively observed service process/invocation. Computer Wi-Fi was connected; no native Android transfer was repeated.

Configuration bytes/hash, size, mode, owner and mtime matched the prior lifecycle receipt. All **23** previously recorded received-file entries retained size, mode, owner and mtime; the only missing entry was the deliberately removed owned lifecycle witness already recorded as cleanup. These file checks are metadata preservation, not a new full-file hash certification.

**Phase 9/R10 is approved for the observed desktop**, combining the earlier administrative install/remove/reinstall checks with this published-package upgrade, real automatic startup, native panel and retained data/preferences. This does not approve untested transports, universal Android compatibility or remote partial cleanup.

## 7. Local development evidence and remaining matrix

Historical suite count progressed from **63** passing regressions to **66**, with one consent-required physical Bluetooth regression ignored by default. Two Linux engines exchanged encrypted empty/Unicode/3 MiB fixtures with matching hashes; queued cancellation did not open TCP; real D-Bus/temporary visibility/portal chooser/panel/notifications were exercised. These are useful local checks but cannot approve native Android empty files or physical transports.

Local NetworkManager created a real WPA2 AP, then explicitly removed temporary profiles and restored the previous Wi-Fi connection. This proves backend creation/cleanup, **not Android negotiation or file data over negotiated hotspot**. Own hotspot credentials are carried through an encrypted accepted channel; hosting a bandwidth upgrade on a shared LAN was not offered because its public introduction lacks early key-possession proof. Direct LAN and joining peer-offered routes remain available.

Distinct unmet criteria retained in the plan:

- Native routerless Wi-Fi negotiation, both-way files and network restoration/abnormal cleanup.
- Both devices' Wi-Fi disabled, native Bluetooth both-way integrity and remaining cancel/disconnect conditions.
- Controlled mDNS block, client isolation, WAN-off and both-device Bluetooth-off LAN dependency.
- Unwritable/full destination physical behavior and remaining equipment/version coverage.
- Native empty-file failure, public Android partial after abrupt cutoff, and unmatched full camera-video hash remain explicitly unapproved.
- Independent Android hashes still missing for particular historical fixtures can be read from existing copies; do not manufacture another basic send to compensate for documentation gaps.

No universal hardware/version claim, marketplace approval, lossless resume or completed R1–R10 delivery follows from these records.

## 8. Public repository, licensing and native branding

- Published the complete source at [PuroDelphi/omarchy-quickshare](https://github.com/PuroDelphi/omarchy-quickshare), preserving the repository's initial history. English README, implementation plan, contribution/security/conduct policies, issue forms, PR checklist and CODEOWNERS are included. Private vulnerability reporting is enabled.
- The owner explicitly chose MIT for original project code while retaining GPL-3.0-only for the vendored core and its modifications. The combined daemon is **not MIT-only**; corresponding-source and notice obligations remain. Dependency notices were regenerated, and the Git dependency is pinned to its full locked revision.
- Exactly one root `manifest.json` supplies root QML entry points with the deployed stable ID `macondo.quickshare`. The native `omarchy plugin validate` command accepted this layout.
- The initial published commit `496274f4f50bfd6757aa2489c07a7667f29b9d87` passed [GitHub CI](https://github.com/PuroDelphi/omarchy-quickshare/actions/runs/38191860491). Local locked workspace tests passed **66**, with one physical Bluetooth regression ignored by default. Shell scripts and the Nautilus module were parsed individually.
- Official marketplace tooling at revision `2b49af8e6fbc24ea25ad8ac6a54b3d55f09b4364` accepted that public snapshot. Its complete static baseline reported **zero findings** and **review-required** for installer, privilege, remote-build, package-manager and service-management capabilities. This is not a security audit or marketplace approval; later commits require their own exact-commit validation.
- **Powered by Jhonny Suarez - PuroDelphi** was observed on the real native English and Spanish panel. It sits below the hero so transfers do not hide it below the fold. The English `preview.png` is an actual idle Wayland capture, not a mockup; it excludes peer names, paths, filenames and PINs. The user's Spanish preference was restored.
- Reloading the shell to deploy the layout kept the existing daemon instance, no active transfers and `errors: []`. The owned Nautilus registration remained intact. No approved basic Android transfer was repeated.
- Administrative upgrade initially could not proceed through this tool session: `pkexec` could not create its textual authentication agent and `sudo -n` required a password. No authorization policy was weakened. The owner subsequently authorized installation of `0.1.0-4` locally; its update and real automatic startup are approved in Section 6.

The repository needs the marketplace's **manual-setup** classification because its external backend and Nautilus registration are not installed by Omarchy's native Git plugin installer. No submission issue was created: the official AI submission policy requires the owner to inspect and explicitly approve the final title, body and checklist first. Publication in the catalog additionally requires a curator's exact-commit review.
