# Installation and maintenance

[Back to Quick Share](../README.md) · [User guide](USER_GUIDE.md) · [Architecture](ARCHITECTURE.md)

## Native Omarchy plugin lifecycle

Quick Share uses Omarchy's normal Git plugin commands. Its stable ID is **`macondo.quickshare`** and the repository has one root manifest.

```bash
omarchy plugin add https://github.com/PuroDelphi/omarchy-quickshare.git --enable
omarchy plugin update macondo.quickshare
omarchy plugin remove macondo.quickshare
```

Omarchy clones the repository, validates the manifest, enables the widget and manages later Git updates/removal. The service installer does **not** replace that checkout with a copied QML folder. Keep local code modifications separate from the installed checkout so native updates can fast-forward safely.

The native commands manage the plugin, not external Rust packages. **Install service**, **Update service** and **Remove service** in the Quick Share panel open a dedicated terminal for the backend and Nautilus integration. This follows the native-plugin-plus-service pattern used by other Omarchy applications.

When the service is ready, open **Settings** to reveal the **Service** controls near the top of the panel. Setup controls are already visible if the service is missing, incompatible or disabled.

Do not run these operations as root. Any necessary system-package authorization happens locally inside the setup terminal. Read the plan and prompts before approving.

## Dependencies

| Purpose | Requirements |
| --- | --- |
| Desktop | Omarchy, Quickshell, session D-Bus, systemd user manager, `jq` |
| Bluetooth | BlueZ with a Classic/BLE-capable adapter and normal user permissions |
| Local networking | A usable local interface; NetworkManager for read-only upgrade selection and managed network changes |
| Shared IPv4/AP support | `dnsmasq`, invoked by NetworkManager when needed |
| File picker | `xdg-desktop-portal` and a FileChooser backend, such as `xdg-desktop-portal-gtk` |
| Nautilus action | `nautilus`, `nautilus-python`, `python-gobject`, `gtk4` |
| Source compilation | Rust/Cargo, `protobuf`/`protoc`, `pkgconf`, D-Bus development files and a C toolchain |
| Local Arch package build | `base-devel`, `makepkg`, `git` and the source compilation dependencies |

NetworkManager starts its own `dnsmasq` for shared IPv4. Do **not** enable `dnsmasq.service` just for Quick Share. Setup must not globally disable a firewall or overwrite Omarchy's package-owned desktop files.

LAN needs mDNS UDP **5353** and the configured TCP listener, normally **47593**, on the trusted local network. Backend **0.2.3** uses that same listener for existing-LAN bandwidth upgrades instead of opening per-session TCP ports. Firewall and access-point policy must permit peer access to the configured listener; setup does not add firewall rules, enable router forwarding or weaken network policy.

## First setup

1. Add and enable the plugin with the native command above.
2. Open **Quick Share** from the bar.
3. Select **Install service**.
4. Follow the terminal's dependency/build/authorization messages.
5. Wait for the panel's **Ready** state.

Restart Nautilus after saving work to load a newly installed Python extension. Setup does not silently close file-manager windows.

### Source-managed service

Without an installed system package, setup builds the locked Rust source from the plugin checkout and installs an owned user executable, user unit and file-manager registration. It retains source/license material for the distributed daemon. The current native Git checkout remains untouched.

The graphical session must have its normal user executable path available for CLI use. The plugin and extension resolve the maintained backend explicitly rather than choosing an arbitrary executable from a modified PATH.

### System-package service

An existing `/usr/bin/omarchy-quickshare` package is used rather than silently shadowed by another user binary. The original `0.1.0` integration used a copied-plugin ownership marker; native-Git-compatible setup upgrades that backend through the visible package-build/install flow when required.

Package changes use the real package manager and require local authorization. Package generation uses source from the current plugin checkout, not an unrelated remote build script or a promised AUR package.

## Update

Finish or explicitly cancel active transfers first.

```bash
omarchy plugin update macondo.quickshare
```

The native updater follows the repository's stable `main` channel, including **v0.2.0** and its optional transfer effects. Then open **Settings** in the panel and choose **Update service** in the **Service** section when you want to update the backend to the release supplied by the current checkout. Source mode rebuilds the current locked checkout; package mode creates and installs the corresponding Arch package with local authorization. Do not claim the daemon is updated merely because the QML checkout changed.

Backend **0.1.2 or newer** supports the private file-selection handoff and the UI-only transfer effects; effects alone do not require a backend upgrade. A 0.1.1 or older backend displays **Service update required**; update the service as well as the native plugin before sharing files. Restart Nautilus after saving work so its process loads the updated extension.

Incoming LAN-first negotiation with shared-port upgrades requires backend **0.2.3**, currently on the **alpha** branch. Updating QML or reloading the shell alone does not update the running daemon. The stable `main` channel and marketplace snapshot are separate from alpha; use the matching checkout's service setup when testing it.

If Omarchy keeps a previously loaded component after a plugin update, use `omarchy restart shell` before opening its updated controls. Shell reload does not itself cancel the independent daemon's transfers, but service maintenance still requires them to end first.

## Remove

1. Finish or cancel transfers.
2. Open **Settings** and select **Remove service** in the **Service** section.
3. Read and confirm the setup terminal's removal plan.
4. Remove the plugin with `omarchy plugin remove macondo.quickshare`.

The backend remover handles only owned executable/unit/integration resources and preserves received files and project configuration. For a system package, the terminal explicitly asks whether to remove the package or retain it with the user's service disabled. Retaining a package is reported as retained, not as removal of its system files. Shared dependencies are not automatically removed.

Native Omarchy removal owns the widget's bar entry and inline shell settings. The language preference is also retained in the project's user configuration so reinstall can restore English/Español without editing unrelated shell entries.

## Ownership, data and troubleshooting

Setup records ownership outside the disposable Git plugin checkout and refuses unrelated destination collisions. Updates/removal stop at an active-transfer guard rather than interrupt someone else's files. Backend configuration, language and received files survive maintenance; deleting personal data is a separate user decision.

Useful diagnostics:

```bash
systemctl --user status omarchy-quickshare.service
journalctl --user -u omarchy-quickshare.service
omarchy-quickshare doctor
omarchy-quickshare status
```

Do not launch a second daemon with the same session bus name. When troubleshooting a retained/disabled service, use the panel's install action to restore the maintained service rather than bypassing its ownership state.

Redact paths, device addresses, SSIDs, credentials and verification codes before reporting errors. Private security reports belong in the repository's [Security](../SECURITY.md) channel.

## Marketplace packaging

The repository root supplies the manifest, README, license and `preview.png`. The plugin keeps the stable `macondo.quickshare` ID across installation and updates. Follow the [official publishing guide](https://plugins.omarchy.org/publish.html) and [submission contract](https://github.com/omacom/omarchy-plugin-marketplace/blob/main/SUBMISSION.md).

The listing's setup classification must describe the separate backend setup accurately. Marketplace compatibility validation and any automated baseline are not a security audit or endorsement; a curator controls listing approval.

## Existing copied-plugin installations

An older setup may have copied QML into the plugin directory instead of leaving a Git checkout. Native `omarchy plugin update` cannot update such a directory.

Finish transfers before migrating. Preserve your download folder and project configuration. If the old language preference exists only in the shell entry, save it before native removal:

```bash
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
mkdir -p "$config_home/omarchy-quickshare"
language=$(jq -r '[((.bar.layout.left // []) + (.bar.layout.center // []) + (.bar.layout.right // []) + (.plugins // []))[] | select(.id == "macondo.quickshare") | .language | select(. == "en" or . == "es")][0] // "en"' \
  "$HOME/.config/omarchy/shell.json") &&
printf '%s\n' "$language" > "$config_home/omarchy-quickshare/language" &&
omarchy plugin remove macondo.quickshare &&
omarchy plugin add https://github.com/PuroDelphi/omarchy-quickshare.git --enable
```

Omarchy backs up a non-Git plugin directory when removing it. The native commands do not delete your Quick Share download/configuration folders. Open the new panel and use **Install service** to complete the backend upgrade and registration; an older system package requires the terminal's explicit authorization.

Native plugin and shell-registry locations follow **`$HOME/.config/omarchy`**, including when backend XDG paths are customized. Backend configuration, source/notices and maintenance receipts use `XDG_CONFIG_HOME`, `XDG_DATA_HOME` and `XDG_STATE_HOME`, respectively.

For terminal-only maintenance, the panel runs this same helper:

```bash
bash "$HOME/.config/omarchy/plugins/macondo.quickshare/scripts/setup-service.sh" install en
```

Actions are `install`, `update` and `remove`; language can be `en` or `es`. This is backend maintenance, not another plugin installer. Source builds use Arch's `/usr/bin/cargo` and `/usr/bin/rustc` so an inactive toolchain shim does not select a different compiler; the owned build cache is outside the watched plugin checkout.

Package builds use the user's cache rather than memory-backed `/tmp` by default and remove their temporary work directory when finished. An explicit `TMPDIR` takes precedence; choose a filesystem with enough space for the Rust build.

## Returning an existing alpha checkout to main

Transfer effects are included in stable **v0.2.0**. An existing native installation checked out on `alpha` can return to `main` through the same native plugin lifecycle:

```bash
plugin="$HOME/.config/omarchy/plugins/macondo.quickshare"
git -C "$plugin" fetch origin main
git -C "$plugin" switch main
omarchy plugin update macondo.quickshare
omarchy restart shell
```

Keep local code edits committed or separately backed up before switching. These commands do not force a checkout or discard modifications; if Git reports conflicting or divergent local changes, preserve and resolve them before continuing rather than resetting the checkout.

The plugin ID remains `macondo.quickshare`. Configuration, language, the saved transfer-effects choice, download destination and received files remain in their existing locations. Backend **0.1.2 or newer** remains compatible and need not be downgraded. To update the backend, finish or cancel transfers and explicitly choose **Update service**, then read and authorize the setup terminal's requested changes.
