//! Serveur de monitoring local (J9).
//!
//! Expose une petite API HTTP (sans framework lourd) sur `127.0.0.1:PORT` :
//!   - `GET /`            → dashboard (index.html)
//!   - `GET /style.css`, `/app.js`
//!   - `GET /state`       → snapshot JSON de l'état du bot
//! Les fichiers frontend sont embarqués à la compilation (binaire autonome).
//! Le frontend poll `/state` chaque seconde.

use std::sync::Arc;

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::RwLock;

const INDEX_HTML: &str = include_str!("../../frontend/index.html");
const STYLE_CSS: &str = include_str!("../../frontend/style.css");
const APP_JS: &str = include_str!("../../frontend/app.js");

/// Snapshot partagé alimenté par les rôles (radar + exécuteur).
#[derive(Debug, Clone, Default, Serialize)]
pub struct DashboardState {
    pub dry_run: bool,
    // Radar
    pub binance_connected: bool,
    pub btc_micro: f64,
    pub obi: f64,
    pub kills_emitted: u64,
    // Exécuteur
    pub market_slug: String,
    pub remaining_s: i64,
    pub sigma: f64,
    pub fair: f64,
    pub up_mid: f64,
    pub up_bid: f64,
    pub up_ask: f64,
    pub in_band: bool,
    pub signals_received: u64,
    // Inventaire / PnL
    pub cash: f64,
    pub up_bal: f64,
    pub down_bal: f64,
    pub latent: f64,
    pub realized_pnl: f64,
    pub fills: u64,
    pub merges: u64,
    // Carnet Up (quelques niveaux autour du mid) pour visualisation.
    pub book_bids: Vec<BookLevel>, // tri décroissant (meilleur en premier)
    pub book_asks: Vec<BookLevel>, // tri croissant
}

#[derive(Debug, Clone, Serialize)]
pub struct BookLevel {
    pub price: f64,
    pub size: f64,
}

pub type Shared = Arc<RwLock<DashboardState>>;

pub fn shared(dry_run: bool) -> Shared {
    Arc::new(RwLock::new(DashboardState {
        dry_run,
        ..Default::default()
    }))
}

/// Lance le serveur HTTP de monitoring (boucle infinie).
pub async fn serve(port: u16, state: Shared) -> anyhow::Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", port)).await?;
    tracing::info!(port, "Dashboard sur http://127.0.0.1:{port}");

    loop {
        let (mut sock, _) = match listener.accept().await {
            Ok(c) => c,
            Err(e) => {
                tracing::debug!(error = %e, "accept");
                continue;
            }
        };
        let state = state.clone();
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let Ok(n) = sock.read(&mut buf).await else { return };
            let req = String::from_utf8_lossy(&buf[..n]);
            let path = req
                .split_whitespace()
                .nth(1)
                .unwrap_or("/")
                .split('?')
                .next()
                .unwrap_or("/");

            let (ctype, body) = match path {
                "/" | "/index.html" => ("text/html; charset=utf-8", INDEX_HTML.to_string()),
                "/style.css" => ("text/css; charset=utf-8", STYLE_CSS.to_string()),
                "/app.js" => ("application/javascript; charset=utf-8", APP_JS.to_string()),
                "/state" => {
                    let s = state.read().await;
                    (
                        "application/json",
                        serde_json::to_string(&*s).unwrap_or_else(|_| "{}".into()),
                    )
                }
                _ => ("text/plain", "not found".to_string()),
            };
            let status = if path == "/state"
                || path == "/"
                || path == "/index.html"
                || path == "/style.css"
                || path == "/app.js"
            {
                "200 OK"
            } else {
                "404 Not Found"
            };
            let resp = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.flush().await;
        });
    }
}
