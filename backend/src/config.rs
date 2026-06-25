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
    pub our_size: f64,            // taille de nos ordres (tokens) — legacy/test
    pub reward_pool_per_min: f64, // pool de reward estimé ($/min)
    pub base_half_spread_cents: f64, // R2 : demi-spread de base (remplace le terme A-S mal échelonné)

    // Bankroll / gates (R4)
    pub bankroll_fraction: f64,    // max % equity par ordre
    pub max_net_exposure_pct: f64, // plafond |net|·mid vs equity
    pub min_cash_reserve_pct: f64, // cash minimum
    pub max_window_loss_pct: f64,  // stop si window_pnl/window_start < -X
    pub max_order_size: f64,       // plafond absolu tokens/ordre
    pub max_position: f64,         // plafond absolu de position par côté
    pub paired_buy_margin: f64,    // achat pairé si up_ask+down_ask < 1 - margin

    // Exécution maker (R3)
    pub maker_fill_prob: f64, // proba de fill maker par tick
    pub maker_only: bool,     // true = pas de fills taker

    // KILL / panic stop (R5)
    pub kill_pause_secs: i64,
    pub panic_stop_secs: i64,

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

            // R1 (truth protocol) : floor MONTÉ — un σ plus élevé rapproche le fair du mid.
            volatility_floor: env_or("VOLATILITY_FLOOR", 0.80),

            gamma: env_or("AS_GAMMA", 0.1),
            kappa: env_or("AS_KAPPA", 1.5),
            our_size: env_or("OUR_SIZE", 50.0),
            reward_pool_per_min: env_or("REWARD_POOL_PER_MIN", 1.0),
            // 0.5¢ → quotes au touch (marchés ~1-2¢ de spread). Calibrable.
            base_half_spread_cents: env_or("BASE_HALF_SPREAD_CENTS", 0.5),

            bankroll_fraction: env_or("BANKROLL_FRACTION", 0.02),
            max_net_exposure_pct: env_or("MAX_NET_EXPOSURE_PCT", 0.15),
            min_cash_reserve_pct: env_or("MIN_CASH_RESERVE_PCT", 0.25),
            max_window_loss_pct: env_or("MAX_WINDOW_LOSS_PCT", 0.10),
            max_order_size: env_or("MAX_ORDER_SIZE", 100.0),
            max_position: env_or("MAX_POSITION", 500.0),
            paired_buy_margin: env_or("PAIRED_BUY_MARGIN", 0.01),

            maker_fill_prob: env_or("MAKER_FILL_PROB", 0.2),
            maker_only: env_or("MAKER_ONLY", true),

            kill_pause_secs: env_or("KILL_PAUSE_SECS", 5),
            panic_stop_secs: env_or("PANIC_STOP_SECS", 30),

            start_cash: env_or("START_CASH", 100.0),
            state_path: env::var("STATE_PATH").unwrap_or_else(|_| "paper_state.json".into()),
            trades_path: env::var("TRADES_PATH").unwrap_or_else(|_| "paper_trades.jsonl".into()),

            min_merge_threshold: env_or("MIN_MERGE_THRESHOLD", 5.0),
            safety_mult: env_or("SAFETY_MULT", 3.0),
        }
    }
}
