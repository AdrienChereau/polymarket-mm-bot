//! Structures de données centrales du bot (carnet L2 Binance, signaux HFT,
//! quotes Polymarket, inventaire). Partagé entre les rôles Radar et Exécuteur.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Alias de prix pour la clarté des signatures.
pub type Price = OrderedFloat;

/// Wrapper `f64` totalement ordonné, utilisable comme clé de `BTreeMap`.
/// NaN est traité comme égal (jamais produit par les flux de prix).
#[derive(Copy, Clone, PartialEq, PartialOrd, Debug)]
pub struct OrderedFloat(pub f64);

impl Eq for OrderedFloat {}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0
            .partial_cmp(&other.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    }
}

/// Carnet d'ordres L2 local de Binance.
/// Les bids sont triés du plus cher au moins cher via `Reverse`,
/// les asks du moins cher au plus cher.
#[derive(Debug, Clone, Default)]
pub struct BinanceOrderBook {
    pub last_update_id: u64,
    pub bids: BTreeMap<Reverse<OrderedFloat>, f64>,
    pub asks: BTreeMap<OrderedFloat, f64>,
}

impl BinanceOrderBook {
    pub fn new() -> Self {
        Self {
            last_update_id: 0,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
        }
    }

    /// Meilleur prix acheteur (plus haut bid).
    pub fn best_bid(&self) -> Option<f64> {
        self.bids.keys().next().map(|k| k.0 .0)
    }

    /// Meilleur prix vendeur (plus bas ask).
    pub fn best_ask(&self) -> Option<f64> {
        self.asks.keys().next().map(|k| k.0)
    }

    /// Prix milieu simple.
    pub fn mid(&self) -> Option<f64> {
        Some((self.best_bid()? + self.best_ask()?) / 2.0)
    }

    /// Micro-price pondéré par la profondeur du top-of-book :
    /// `(bid·ask_qty + ask·bid_qty) / (bid_qty + ask_qty)`.
    pub fn calculate_micro_price(&self) -> Option<f64> {
        let best_bid = self.best_bid()?;
        let best_ask = self.best_ask()?;
        let bid_depth = *self.bids.values().next()?;
        let ask_depth = *self.asks.values().next()?;

        if bid_depth + ask_depth == 0.0 {
            return None;
        }
        Some(((best_bid * ask_depth) + (best_ask * bid_depth)) / (bid_depth + ask_depth))
    }
}

/// Tick de prix publié par le connecteur Binance vers le reste du bot.
#[derive(Debug, Copy, Clone, Serialize)]
pub struct PriceTick {
    pub mid: f64,
    pub micro_price: f64,
    pub best_bid: f64,
    pub best_ask: f64,
    pub ts_ms: u64,
}

/// Snapshot du carnet Binance publié sur un canal `watch` vers le radar.
#[derive(Debug, Clone)]
pub struct BookUpdate {
    pub book: BinanceOrderBook,
    pub ts_ms: u64,
}

impl BookUpdate {
    /// Construit un `PriceTick` à partir du carnet (None si carnet vide).
    pub fn price_tick(&self) -> Option<PriceTick> {
        Some(PriceTick {
            mid: self.book.mid()?,
            micro_price: self.book.calculate_micro_price()?,
            best_bid: self.book.best_bid()?,
            best_ask: self.book.best_ask()?,
            ts_ms: self.ts_ms,
        })
    }
}

/// Signaux HFT transcontinentaux (1 octet sur le réseau).
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum Signal {
    Kill = 0x4B,      // 'K'
    Heartbeat = 0x48, // 'H'
}

/// Quote produite par le moteur de risque (côté exécuteur).
#[derive(Debug, Clone, Serialize)]
pub struct Quote {
    pub bid_price: f64,
    pub ask_price: f64,
    pub size: f64,
    pub timestamp: DateTime<Utc>,
}

/// Inventaire et PnL du bot (mode paper au J8).
#[derive(Debug, Clone, Default, Serialize)]
pub struct BotInventory {
    pub yes_balance: f64,
    pub no_balance: f64,
    pub cash_usdc: f64,
    pub real_pnl: f64,
    pub latent_pnl: f64,
}
