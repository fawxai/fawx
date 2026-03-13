use axum::Router;
use std::net::{IpAddr, SocketAddr};
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

pub fn print_startup_targets(listeners: &BoundListeners) {
    eprintln!("Fawx HTTP API listening on:");
    eprintln!(
        "  http://{} ({})",
        listeners.local.target.addr, listeners.local.target.label
    );
    match &listeners.tailscale {
        Some(listener) => {
            eprintln!(
                "  http://{} ({})",
                listener.target.addr, listener.target.label
            );
        }
        None => {
            eprintln!("  Tailscale not detected or unavailable; serving localhost only");
        }
    }
}

pub async fn run_listeners(router: Router, listeners: BoundListeners) -> anyhow::Result<()> {
    match listeners.tailscale {
        Some(tailscale) => run_listener_pair(router, listeners.local, tailscale).await,
        None => {
            serve_listener(
                listeners.local.listener,
                router,
                listeners.local.target.label,
            )
            .await
        }
    }
}

pub async fn bind_listeners(plan: ListenPlan) -> anyhow::Result<BoundListeners> {
    let local = bind_required_listener(plan.local).await?;
    let tailscale = bind_optional_listener(plan.tailscale).await;
    Ok(BoundListeners { local, tailscale })
}

pub async fn bind_required_listener(target: ListenTarget) -> anyhow::Result<BoundListener> {
    let listener = bind_listener(target).await?;
    Ok(BoundListener { target, listener })
}

pub async fn bind_optional_listener(target: Option<ListenTarget>) -> Option<BoundListener> {
    let target = target?;
    optional_bound_listener(target, bind_listener(target).await)
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
        local_label,
        local_server,
        tailscale_label,
        tailscale_server,
        shutdown_tx,
    )
    .await
}

pub async fn bind_listener(target: ListenTarget) -> anyhow::Result<TcpListener> {
    TcpListener::bind(target.addr).await.map_err(|e| {
        anyhow::anyhow!(
            "failed to bind {} HTTP server on {}: {e}",
            target.label,
            target.addr
        )
    })
}

pub async fn serve_listener(
    listener: TcpListener,
    router: Router,
    label: &'static str,
) -> anyhow::Result<()> {
    axum::serve(listener, router)
        .await
        .map_err(|e| anyhow::anyhow!("{label} HTTP server error: {e}"))
}

pub async fn serve_listener_with_shutdown(
    listener: BoundListener,
    router: Router,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let label = listener.target.label;
    axum::serve(listener.listener, router)
        .with_graceful_shutdown(async move {
            if !*shutdown.borrow() {
                let _ = shutdown.changed().await;
            }
        })
        .await
        .map_err(|e| anyhow::anyhow!("{label} HTTP server error: {e}"))
}

pub async fn wait_for_server_pair(
    local_label: &'static str,
    local_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    tailscale_label: &'static str,
    tailscale_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let mut local_server = local_server;
    let mut tailscale_server = tailscale_server;

    tokio::select! {
        result = &mut local_server => {
            finalize_server_exit(local_label, result, tailscale_label, tailscale_server, shutdown_tx).await
        }
        result = &mut tailscale_server => {
            finalize_server_exit(tailscale_label, result, local_label, local_server, shutdown_tx).await
        }
    }
}

pub async fn finalize_server_exit(
    exited_label: &'static str,
    exited_result: Result<anyhow::Result<()>, tokio::task::JoinError>,
    peer_label: &'static str,
    peer_server: tokio::task::JoinHandle<anyhow::Result<()>>,
    shutdown_tx: tokio::sync::watch::Sender<bool>,
) -> anyhow::Result<()> {
    let exited = join_server_result(exited_label, exited_result);
    log_server_exit(
        exited_label,
        &exited,
        "HTTP server exited; shutting down peer",
    );
    let _ = shutdown_tx.send(true);
    let peer = join_server_result(peer_label, peer_server.await);
    log_server_exit(
        peer_label,
        &peer,
        "Peer HTTP server stopped after shutdown signal",
    );
    exited.and(peer)
}

pub fn log_server_exit(label: &str, result: &anyhow::Result<()>, message: &str) {
    match result {
        Ok(()) => tracing::warn!(server = label, "{message}"),
        Err(error) => tracing::warn!(server = label, error = %error, "{message}"),
    }
}

pub fn join_server_result(
    label: &str,
    result: Result<anyhow::Result<()>, tokio::task::JoinError>,
) -> anyhow::Result<()> {
    match result {
        Ok(inner) => inner,
        Err(error) => Err(anyhow::anyhow!("{label} HTTP server task failed: {error}")),
    }
}
