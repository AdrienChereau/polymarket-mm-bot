//! Configuration du bot, chargée depuis l'environnement (`.env` + variables systemd).
//! Le rôle (`radar`|`executor`) détermine quelle boucle `main.rs` lance.

use std::env;
use std::net::SocketAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BotRole {
    Radar,
    Executor,
}

impl BotRole {
    fn from_env() -> Self {
        // Priorité à BOT_ROLE explicite, sinon dérivé de la région AWS.
        let raw = env::var("BOT_ROLE")
            .ok()
            .or_else(|| env::var("AWS_REGION").ok())
            .unwrap_or_default()
            .to_lowercase();
        match raw.as_str() {
            "radar" | "ap-northeast-1" => BotRole::Radar,
            "executor" | "eu-west-1" => BotRole::Executor,
            // Défaut sûr en dev : exécuteur (ne touche pas Binance à haute fréquence).
            _ => BotRole::Executor,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub role: BotRole,
    pub dry_run: bool,

    // Réseau / dashboard
    pub dashboard_port: u16,
    pub binance_ws_url: String,
    pub signal_addr: SocketAddr,        // adresse locale d'écoute (exécuteur)
    pub signal_target: Option<SocketAddr>, // cible (radar → exécuteur)
    pub use_udp_transport: bool,        // false = loopback in-process (dev local)

    // Radar (J2)
    pub obi_depth_levels: usize,
    pub obi_threshold: f64,
    pub velocity_threshold: f64,

    // Volatilité (J4)
    pub volatility_floor: f64,

    // Avellaneda-Stoikov + reward (J7)
    pub gamma: f64,
    pub kappa: f64,
    pub our_size: f64,            // taille de nos ordres (tokens)
    pub reward_pool_per_min: f64, // pool de reward estimé ($/min)

    // Paper / inventaire (J8)
    pub start_cash: f64,
    pub state_path: String,
    pub trades_path: String,

    // Fusion CTF (J8/J11)
    pub min_merge_threshold: f64,
    pub safety_mult: f64,
}

fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

impl Config {
    pub fn from_env() -> Self {
        let dashboard_port: u16 = env_or("PORT", 8767);
        let signal_port: u16 = env_or("SIGNAL_PORT", 9001);

        let signal_addr: SocketAddr = env::var("SIGNAL_ADDR")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| ([127, 0, 0, 1], signal_port).into());

        let signal_target: Option<SocketAddr> = env::var("SIGNAL_TARGET")
            .ok()
            .and_then(|s| s.parse().ok());

        Self {
            role: BotRole::from_env(),
            dry_run: env_or("DRY_RUN", true),

            dashboard_port,
            binance_ws_url: env::var("BINANCE_WS_URL").unwrap_or_else(|_| {
                // Partial book depth : snapshot complet du top-20 à 100ms,
                // pas de resynchro par lastUpdateId nécessaire.
                "wss://stream.binance.com:9443/ws/btcusdt@depth20@100ms".to_string()
            }),
            signal_addr,
            signal_target,
            use_udp_transport: env_or("USE_UDP_TRANSPORT", false),

            obi_depth_levels: env_or("OBI_DEPTH_LEVELS", 5),
            obi_threshold: env_or("OBI_THRESHOLD", 0.85),
            velocity_threshold: env_or("VELOCITY_THRESHOLD", 5.0),

            volatility_floor: env_or("VOLATILITY_FLOOR", 0.40),

            gamma: env_or("AS_GAMMA", 0.1),
            kappa: env_or("AS_KAPPA", 1.5),
            our_size: env_or("OUR_SIZE", 50.0),
            reward_pool_per_min: env_or("REWARD_POOL_PER_MIN", 1.0),

            start_cash: env_or("START_CASH", 100.0),
            state_path: env::var("STATE_PATH").unwrap_or_else(|_| "paper_state.json".into()),
            trades_path: env::var("TRADES_PATH").unwrap_or_else(|_| "paper_trades.jsonl".into()),

            min_merge_threshold: env_or("MIN_MERGE_THRESHOLD", 5.0),
            safety_mult: env_or("SAFETY_MULT", 3.0),
        }
    }
}
