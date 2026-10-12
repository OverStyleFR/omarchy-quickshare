mod integration;
mod service;
mod ui_language;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use std::io::{self, Write};
use std::path::PathBuf;

const BUS: &str = "io.omarchy.QuickShare";
const PATH: &str = "/io/omarchy/QuickShare";
const INTERFACE: &str = "io.omarchy.QuickShare1";

#[derive(Parser)]
#[command(version, about = "Quick Share para Omarchy")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the session service. Visibility starts disabled.
    Serve {
        #[arg(long, default_value_t = 47593, value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
        #[arg(long)]
        download_dir: Option<PathBuf>,
        #[arg(long)]
        no_bluetooth: bool,
        #[arg(long)]
        no_lan: bool,
        #[arg(long)]
        no_hotspot: bool,
    },
    Status,
    /// Print an initial snapshot and subsequent changes as NDJSON.
    Watch,
    /// Make discoverable for 1–600 seconds; 0 disables visibility.
    Visible {
        seconds: u32,
    },
    /// Permit or forbid temporary Wi-Fi interruption for direct transfers.
    NetworkConsent {
        #[arg(value_parser = ["on", "off"])]
        value: String,
    },
    Send {
        peer: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// Open the Omarchy panel with local files selected; never start a transfer.
    Share {
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    Accept {
        id: String,
    },
    Reject {
        id: String,
    },
    Cancel {
        id: String,
    },
    DownloadDir {
        path: PathBuf,
    },
    /// Select local files through the desktop portal; output a JSON array.
    PickFiles,
    /// Report actual local tools and radio capabilities (not Android interoperability).
    Doctor,
}

fn print_json_line(value: &str) -> Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()?;
    Ok(())
}

fn canonical_files(files: &[PathBuf]) -> Result<Vec<String>> {
    files
        .iter()
        .map(|p| {
            let path = std::fs::canonicalize(p)
                .with_context(|| format!("Archivo inaccesible: {}", p.display()))?;
            path.into_os_string()
                .into_string()
                .map_err(|_| anyhow::anyhow!("La ruta no puede representarse en D-Bus"))
        })
        .collect()
}

async fn share(files: &[PathBuf]) -> Result<()> {
    if files.is_empty() || files.len() > 1000 {
        bail!("Selecciona entre 1 y 1000 archivos");
    }
    let paths = canonical_files(files)?;
    for path in &paths {
        let metadata =
            std::fs::metadata(path).with_context(|| format!("Archivo inaccesible: {path}"))?;
        if !metadata.is_file() {
            bail!("Solo se pueden enviar archivos normales: {path}");
        }
        std::fs::File::open(path).with_context(|| format!("Archivo no legible: {path}"))?;
    }
    let payload = serde_json::json!({ "files": paths }).to_string();
    let output = tokio::process::Command::new("omarchy-shell")
        .args(["shell", "summon", "macondo.quickshare", &payload])
        .output()
        .await
        .context("No se pudo ejecutar omarchy-shell para abrir Quick Share")?;
    if !output.status.success() {
        bail!(
            "No se pudo abrir el panel Quick Share: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let response = std::str::from_utf8(&output.stdout)
        .context("Respuesta no válida del shell Omarchy")?
        .trim();
    if response != "ok" {
        bail!(
            "El shell no abrió Quick Share (respuesta: {response:?}); comprueba que el plugin esté instalado y habilitado"
        );
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct SnapshotOrder<'a> {
    #[serde(borrow)]
    instance_id: &'a str,
    revision: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Command::Serve {
        port,
        download_dir,
        no_bluetooth,
        no_lan,
        no_hotspot,
    } = cli.command
    {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "warn".into()),
            )
            .with_writer(io::stderr)
            .init();
        return service::serve(
            port,
            download_dir,
            rqs_lib::EngineOptions {
                bluetooth: !no_bluetooth,
                lan: !no_lan,
                hotspot: !no_hotspot,
            },
        )
        .await;
    }
    match cli.command {
        Command::Share { files } => share(&files).await?,
        Command::PickFiles => {
            let selection = ashpd::desktop::file_chooser::SelectedFiles::open_file()
                .title("Compartir con Quick Share")
                .multiple(true)
                .modal(false)
                .send()
                .await
                .and_then(|request| request.response());
            let response = match selection {
                Ok(response) => response,
                Err(ashpd::Error::Response(ashpd::desktop::ResponseError::Cancelled)) => {
                    print_json_line("[]")?;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            };
            let paths: Vec<PathBuf> = response
                .uris()
                .iter()
                .map(|uri| {
                    url::Url::parse(uri.as_str())?
                        .to_file_path()
                        .map_err(|_| anyhow::anyhow!("El portal devolvió una URI no local"))
                })
                .collect::<Result<_>>()?;
            print_json_line(&serde_json::to_string(&paths)?)?;
        }
        Command::Doctor => service::doctor().await?,
        other => {
            let connection = zbus::Connection::session().await?;
            let proxy = zbus::Proxy::new(&connection, BUS, PATH, INTERFACE).await?;
            match other {
                Command::Status => {
                    let snapshot: String = proxy
                        .call("Snapshot", &())
                        .await
                        .context("El servicio Quick Share no está disponible")?;
                    print_json_line(&snapshot)?;
                }
                Command::Watch => {
                    // Subscribe before taking the snapshot so an update cannot be missed.
                    let mut signals = proxy.receive_signal("Changed").await?;
                    let mut owners = proxy.receive_owner_changed().await?;
                    let snapshot: String = proxy.call("Snapshot", &()).await?;
                    let order: SnapshotOrder<'_> = serde_json::from_str(&snapshot)?;
                    let instance = order.instance_id.to_owned();
                    let mut revision = order.revision;
                    print_json_line(&snapshot)?;
                    loop {
                        tokio::select! {
                            signal = signals.next() => {
                                let signal = signal.context("La conexión con el servicio terminó")?;
                                let (snapshot,): (String,) = signal.body().deserialize()?;
                                let order: SnapshotOrder<'_> = serde_json::from_str(&snapshot)?;
                                if order.instance_id != instance { bail!("El servicio Quick Share se reinició"); }
                                if order.revision > revision {
                                    revision = order.revision;
                                    print_json_line(&snapshot)?;
                                }
                            }
                            _ = owners.next() => bail!("El servicio Quick Share se detuvo o reinició"),
                        }
                    }
                }
                Command::Visible { seconds } => {
                    let _: () = proxy.call("SetVisibility", &(seconds,)).await?;
                }
                Command::NetworkConsent { value } => {
                    let _: () = proxy.call("SetNetworkConsent", &(value == "on",)).await?;
                }
                Command::Send { peer, files } => {
                    let paths = canonical_files(&files)?;
                    let id: String = proxy.call("Send", &(peer, paths)).await?;
                    println!("{id}");
                }
                Command::Accept { id } => {
                    let _: () = proxy.call("Respond", &(id, true)).await?;
                }
                Command::Reject { id } => {
                    let _: () = proxy.call("Respond", &(id, false)).await?;
                }
                Command::Cancel { id } => {
                    let _: () = proxy.call("Cancel", &(id,)).await?;
                }
                Command::DownloadDir { path } => {
                    let path = std::fs::canonicalize(path)?;
                    let path = path.to_str().context("Ruta no representable en D-Bus")?;
                    let _: () = proxy.call("SetDownloadDirectory", &(path,)).await?;
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}
