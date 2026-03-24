use anyhow::Context;
use axum::Router;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use tokio::net::TcpListener;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenTarget {
    pub addr: SocketAddr,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ListenPlan {
    pub local: ListenTarget,
    pub tailscale: Option<ListenTarget>,
}

pub struct BoundListener {
    pub target: ListenTarget,
    pub listener: TcpListener,
}

pub struct BoundListeners {
    pub local: BoundListener,
    pub tailscale: Option<BoundListener>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ServerProtocol {
    Http,
    Https,
}

impl ServerProtocol {
    fn scheme(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::Https => "https",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Https => "HTTPS",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ServerIdentity {
    pub label: &'static str,
    pub protocol: ServerProtocol,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TlsConfig {
    pub(crate) cert_path: PathBuf,
    pub(crate) key_path: PathBuf,
}

pub fn listen_targets(port: u16, tailscale_ip: Option<IpAddr>) -> ListenPlan {
    ListenPlan {
        local: ListenTarget {
            addr: SocketAddr::from(([127, 0, 0, 1], port)),
            label: "local",
        },
        tailscale: tailscale_ip.map(|ip| ListenTarget {
            addr: SocketAddr::new(ip, port),
            label: "Tailscale",
        }),
    }
}

pub fn optional_tailscale_ip(result: Result<IpAddr, crate::error::HttpError>) -> Option<IpAddr> {
    match result {
        Ok(ip) => Some(ip),
        Err(error) => {
            tracing::warn!(error = %error, "tailscale IP not detected; serving localhost only");
            None
        }
    }
}

pub fn detect_optional_tailscale_ip() -> Option<IpAddr> {
    optional_tailscale_ip(crate::tailscale::detect_tailscale_ip())
}

pub fn active_tailscale_ip(listeners: &BoundListeners) -> Option<String> {
    listeners
        .tailscale
        .as_ref()
        .map(|listener| listener.target.addr.ip().to_string())
}

pub(crate) fn detect_tls_config(data_dir: &Path) -> Option<TlsConfig> {
    let tls_dir = data_dir.join("tls");
    let cert_path = tls_dir.join("cert.pem");
    let key_path = tls_dir.join("key.pem");

    if cert_path.is_file() && key_path.is_file() {
        Some(TlsConfig {
            cert_path,
            key_path,
        })
    } else {
        None
    }
}

pub(crate) fn startup_target_lines(
    local: ListenTarget,
    tailscale: Option<ListenTarget>,
    tailscale_https_enabled: bool,
) -> Vec<String> {
    let mut lines = vec![
        if tailscale_https_enabled {
            "Fawx API listening on:".to_string()
        } else {
            "Fawx HTTP API listening on:".to_string()
        },
        format!("  http://{} ({})", local.addr, local.label),
    ];

    match tailscale {
        Some(listener) => {
            let protocol = if tailscale_https_enabled {
                ServerProtocol::Https
            } else {
                ServerProtocol::Http
            };
            lines.push(format!(
                "  {}://{} ({})",
                protocol.scheme(),
                listener.addr,
                listener.label
            ));
        }
        None => {
            lines.push(
                "  Tailscale not detected or unavailable; serving localhost only".to_string(),
            );
        }
    }

    lines
}

pub(crate) fn print_startup_targets(listeners: &BoundListeners, tailscale_https_enabled: bool) {
    for line in startup_target_lines(
        listeners.local.target,
        listeners.tailscale.as_ref().map(|listener| listener.target),
        tailscale_https_enabled,
    ) {
        eprintln!("{line}");
    }
}

pub(crate) async fn run_listeners(
    router: Router,
    listeners: BoundListeners,
    tls_config: Option<TlsConfig>,
) -> anyhow::Result<()> {
    let BoundListeners { local, tailscale } = listeners;

    match (tailscale, tls_config) {
        (Some(tailscale), Some(tls_config)) => {
            run_listener_pair_with_tls(router, local, tailscale, tls_config).await
        }
        (Some(tailscale), None) => run_listener_pair(router, local, tailscale).await,
        (None, _) => serve_listener(local.listener, router, local.target.label).await,
    }
}

pub(crate) async fn bind_listeners(
    plan: ListenPlan,
    tailscale_protocol: ServerProtocol,
) -> anyhow::Result<BoundListeners> {
    let local = bind_required_listener(plan.local, ServerProtocol::Http).await?;
    let tailscale = bind_optional_listener(plan.tailscale, tailscale_protocol).await;
    Ok(BoundListeners { local, tailscale })
}

pub(crate) async fn bind_required_listener(
    target: ListenTarget,
    protocol: ServerProtocol,
) -> anyhow::Result<BoundListener> {
    let listener = bind_listener(target, protocol).await?;
    Ok(BoundListener { target, listener })
}

pub(crate) async fn bind_optional_listener(
    target: Option<ListenTarget>,
    protocol: ServerProtocol,
) -> Option<BoundListener> {
    let target = target?;
    optional_bound_listener(target, bind_listener(target, protocol).await)
}

pub fn optional_bound_listener(
    target: ListenTarget,
    result: anyhow::Result<TcpListener>,
) -> Option<BoundListener> {
    match result {
        Ok(listener) => Some(BoundListener { target, listener }),
        Err(error) => {
            tracing::warn!(
                error = %error,
                addr = %target.addr,
                "Tailscale bind failed; continuing with localhost only"
            );
            eprintln!(
                "  Warning: Tailscale bind failed on {}, serving localhost only",
                target.addr
            );
            None
        }
    }
}

pub async fn run_listener_pair(
    router: Router,
    local: BoundListener,
    tailscale: BoundListener,
) -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let local_label = local.target.label;
    let tailscale_label = tailscale.target.label;
    let local_server = tokio::spawn(serve_listener_with_shutdown(
        local,
        router.clone(),
        shutdown_rx.clone(),
    ));
    let tailscale_server =
        tokio::spawn(serve_listener_with_shutdown(tailscale, router, shutdown_rx));

    wait_for_server_pair(
        ServerIdentity {
            label: local_label,
            protocol: ServerProtocol::Http,
        },
        local_server,
        ServerIdentity {
            label: tailscale_label,
            protocol: ServerProtocol::Http,
        },
        tailscale_server,
        shutdown_tx,
    )
    .await
}

pub(crate) async fn run_listener_pair_with_tls(
    router: Router,
    local: BoundListener,
    tailscale: BoundListener,
    tls_config: TlsConfig,
) -> anyhow::Result<()> {
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let local_label = local.target.label;
    let tailscale_label = tailscale.target.label;
    let local_server = tokio::spawn(serve_listener_with_shutdown(
        local,
        router.clone(),
        shutdown_rx.clone(),
    ));
    let tailscale_server = tokio::spawn(async move {
        serve_tls_listener_with_shutdown(tailscale, router, &tls_config, shutdown_rx).await
    });

    wait_for_server_pair(
        ServerIdentity {
            label: local_label,
            protocol: ServerProtocol::Http,
        },
        local_server,
        ServerIdentity {
            label: tailscale_label,
            protocol: ServerProtocol::Https,
        },
        tailscale_server,
        shutdown_tx,
    )
    .await
}

pub(crate) async fn bind_listener(
    target: ListenTarget,
    protocol: ServerProtocol,
) -> anyhow::Result<TcpListener> {
    TcpListener::bind(target.addr).await.map_err(|e| {
        anyhow::anyhow!(
            "failed to bind {} {} server on {}: {e}",
            target.label,
            protocol.name(),
            target.addr
        )
    })
}

pub async fn serve_listener(
    listener: TcpListener,
    router: Router,
    label: &'static str,
) -> anyhow::Result<()> {
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{label} HTTP server error: {e}"))
}

pub async fn serve_listener_with_shutdown(
    listener: BoundListener,
    router: Router,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let label = listener.target.label;
    axum::serve(
        listener.listener,
        router.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        if !*shutdown.borrow() {
            let _ = shutdown.changed().await;
        }
    })
    .await
    .map_err(|e| anyhow::anyhow!("{label} HTTP server error: {e}"))
}

pub(crate) async fn serve_tls_listener_with_shutdown(
    listener: BoundListener,
    router: Router,
    tls_config: &TlsConfig,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let label = listener.target.label;
    let rustls_config = load_rustls_config(tls_config).await?;
    let handle = axum_server::Handle::new();
    let shutdown_handle = handle.clone();
    let shutdown_task = tokio::spawn(async move {
        if !*shutdown.borrow() {
            let _ = shutdown.changed().await;
        }
        shutdown_handle.graceful_shutdown(None);
    });
    let std_listener = listener
        .listener
        .into_std()
        .map_err(|error| anyhow::anyhow!("failed to convert {label} TLS listener: {error}"))?;
    let result = axum_server::from_tcp_rustls(std_listener, rustls_config)
        .map_err(|error| anyhow::anyhow!("failed to start {label} HTTPS server: {error}"))?
        .handle(handle)
        .serve(router.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .map_err(|e| anyhow::anyhow!("{label} HTTPS server error: {e}"));
    shutdown_task.abort();
    result
}

async fn load_rustls_config(
    tls_config: &TlsConfig,
) -> anyhow::Result<axum_server::tls_rustls::RustlsConfig> {
    // Ensure a CryptoProvider is installed before rustls attempts to use one.
    // ring is already a dependency; this makes it the process-level default.
    let _ = rustls::crypto::ring::default_provider().install_default();

    axum_server::tls_rustls::RustlsConfig::from_pem_file(
        tls_config.cert_path.clone(),
        tls_config.key_path.clone(),
    )
    .await
    .context("failed to load TLS certificates")
}

pub async fn wait_for_server_pair(
    local: ServerIdentity,
    local_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    tailscale: ServerIdentity,
    tailscale_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let mut local_server = local_server;
    let mut tailscale_server = tailscale_server;

    tokio::select! {
        result = &mut local_server => {
            finalize_server_exit(local, result, tailscale, tailscale_server, shutdown_tx).await
        }
        result = &mut tailscale_server => {
            finalize_server_exit(tailscale, result, local, local_server, shutdown_tx).await
        }
    }
}

pub async fn finalize_server_exit(
    exited: ServerIdentity,
    exited_result: Result<anyhow::Result<()>, tokio::task::JoinError>,
    peer: ServerIdentity,
    peer_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let exited_result = join_server_result(exited, exited_result);
    let exit_message = format!(
        "{} server exited; shutting down peer",
        exited.protocol.name()
    );
    log_server_exit(exited.label, &exited_result, &exit_message);
    let _ = shutdown_tx.send(true);
    let peer_result = join_server_result(peer, peer_server.await);
    let peer_message = format!(
        "Peer {} server stopped after shutdown signal",
        peer.protocol.name()
    );
    log_server_exit(peer.label, &peer_result, &peer_message);
    exited_result.and(peer_result)
}

pub fn log_server_exit(label: &str, result: &anyhow::Result<()>, message: &str) {
    match result {
        Ok(()) => tracing::warn!(server = label, "{message}"),
        Err(error) => tracing::warn!(server = label, error = %error, "{message}"),
    }
}

pub fn join_server_result(
    server: ServerIdentity,
    result: Result<anyhow::Result<()>, tokio::task::JoinError>,
) -> anyhow::Result<()> {
    match result {
        Ok(inner) => inner,
        Err(error) => Err(anyhow::anyhow!(
            "{} {} server task failed: {error}",
            server.label,
            server.protocol.name()
        )),
    }
}
