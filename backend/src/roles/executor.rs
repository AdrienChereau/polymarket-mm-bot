//! Nœud Exécuteur (Dublin).
//! J6 : connecteur Polymarket (marché actif + carnet, rollover).
//! J7 : feed Binance (spot) + volatilité + pricing BS + quotes A-S reward-adjusted.
//! J8 (à venir) : exécution paper + inventaire + fusion CTF.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use crate::config::Config;
use crate::connectors::binance;
use crate::connectors::polymarket::{Market, PolymarketClient};
use crate::dashboard::Shared;
use crate::engines::risk::{compute_quote, QuoteInputs};
use crate::engines::{pricing, volatility::VolatilityEngine};
use crate::inventory::PaperEngine;
use crate::signal::SignalTransport;
use crate::types::BookUpdate;

pub async fn run(cfg: Config, transport: Arc<dyn SignalTransport>, dash: Shared) -> anyhow::Result<()> {
    tracing::info!(
        role = "executor",
        dry_run = cfg.dry_run,
        dashboard_port = cfg.dashboard_port,
        "Nœud Exécuteur démarré"
    );

    // Écoute des signaux radar (en parallèle).
    {
        let dash = dash.clone();
        tokio::spawn(async move {
            loop {
                match transport.recv_signal().await {
                    Ok(sig) => {
                        tracing::warn!(?sig, "⚡ Signal reçu du Radar — retrait des quotes (anti-toxicité)");
                        dash.write().await.signals_received += 1;
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "réception signal");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        });
    }

    // Feed Binance local (spot BTC pour le pricing) + moteur de volatilité.
    let (spot_tx, spot_rx) = watch::channel::<Option<BookUpdate>>(None);
    let (sigma_tx, sigma_rx) = watch::channel::<f64>(cfg.volatility_floor);

    let url = cfg.binance_ws_url.clone();
    tokio::spawn(async move {
        if let Err(e) = binance::run(url, spot_tx).await {
            tracing::error!(error = %e, "feed Binance (exécuteur) arrêté");
        }
    });

    // Tâche volatilité : consomme le micro-price et publie le sigma annualisé.
    {
        let mut vol = VolatilityEngine::new(2000, cfg.volatility_floor);
        let mut rx = spot_rx.clone();
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                let sample = rx.borrow().clone();
                if let Some(u) = sample {
                    if let Some(t) = u.price_tick() {
                        vol.update(t.ts_ms, t.micro_price);
                        let _ = sigma_tx.send(vol.annualized_sigma());
                    }
                }
            }
        });
    }

    quote_loop(cfg, spot_rx, sigma_rx, dash).await
}

async fn quote_loop(
    cfg: Config,
    spot_rx: watch::Receiver<Option<BookUpdate>>,
    sigma_rx: watch::Receiver<f64>,
    dash: Shared,
) -> anyhow::Result<()> {
    let client = PolymarketClient::new();
    let mut paper = PaperEngine::load_or_init(
        cfg.start_cash, cfg.our_size, cfg.min_merge_threshold, cfg.safety_mult,
        cfg.state_path.clone(), cfg.trades_path.clone(),
    );

    let mut current: Option<Market> = None;
    let mut strike: Option<f64> = None;
    let mut last_spot: Option<f64> = None;
    let mut poll = tokio::time::interval(Duration::from_secs(1));
    let mut persist_ctr: u32 = 0;

    loop {
        poll.tick().await;

        let need_resolve = current.as_ref().map_or(true, |m| m.time_remaining_sec() <= 0);
        if need_resolve {
            // Résoudre le marché précédent (Up gagne si close ≥ open de la fenêtre).
            // Close = open de la fenêtre suivante (kline) ; à défaut, dernier spot observé.
            if let (Some(prev), Some(prev_strike)) = (current.as_ref(), strike) {
                let close = binance::price_at_window_open(prev.window_ts + 300)
                    .await
                    .ok()
                    .or(last_spot);
                match close {
                    Some(c) => {
                        paper.resolve(c >= prev_strike);
                        paper.persist();
                    }
                    None => tracing::warn!("résolution sautée : ni close kline ni spot disponible"),
                }
            }
            match client.get_current_btc_5m_market().await {
                Ok(Some(m)) => {
                    strike = binance::price_at_window_open(m.window_ts).await.ok();
                    tracing::info!(
                        slug = %m.slug, remaining_s = m.time_remaining_sec(),
                        strike = ?strike, "=== Nouveau marché BTC 5min ==="
                    );
                    current = Some(m);
                }
                Ok(None) => { tokio::time::sleep(Duration::from_secs(2)).await; continue; }
                Err(e) => { tracing::error!(error=%e,"résolution marché"); tokio::time::sleep(Duration::from_secs(2)).await; continue; }
            }
        }

        let Some(m) = &current else { continue };

        // Strike résilient : si la capture a échoué au rollover, on réessaie chaque
        // tick (le kline d'ouverture peut n'être disponible qu'après quelques secondes).
        if strike.is_none() {
            let w = m.window_ts;
            if let Ok(s) = binance::price_at_window_open(w).await {
                strike = Some(s);
                tracing::info!(strike = s, slug = %m.slug, "strike capturé (retry)");
            }
        }
        let Some(strike) = strike else { continue };

        let spot = spot_rx.borrow().as_ref().and_then(|u| u.price_tick()).map(|t| t.micro_price);
        let Some(spot) = spot else { continue };
        last_spot = Some(spot);
        let sigma = *sigma_rx.borrow();
        let t_years = pricing::years_from_secs(m.time_remaining_sec().max(0) as f64);
        let fair_up = pricing::fair_up_probability(spot, strike, sigma, t_years);

        // Carnets Up et Down (les deux côtés → permet la fusion CTF).
        let (up_book, down_book) = match (
            client.get_book(&m.up_token_id).await,
            client.get_book(&m.down_token_id).await,
        ) {
            (Ok(u), Ok(d)) => (u, d),
            _ => continue,
        };
        let (Some(up_mid), Some(down_mid)) = (up_book.mid(), down_book.mid()) else { continue };

        // Quote + fill simulé sur chaque côté.
        let q_up = compute_quote(&quote_inputs(&cfg, m, fair_up, up_mid, sigma, t_years, paper.state.up_balance), &up_book);
        let q_dn = compute_quote(&quote_inputs(&cfg, m, 1.0 - fair_up, down_mid, sigma, t_years, paper.state.down_balance), &down_book);
        paper.try_buy("up", q_up.bid, up_book.best_ask());
        paper.try_buy("down", q_dn.bid, down_book.best_ask());

        // Fusion CTF (vélocité du capital) — rendement attendu proxy via reward.
        let yield_per_usdc = (q_up.expected_reward + q_dn.expected_reward).max(0.1);
        paper.check_and_merge(yield_per_usdc);

        let latent = paper.mark_to_market(up_mid, down_mid);

        // Mise à jour du dashboard (état exécuteur + PnL).
        {
            let mut d = dash.write().await;
            d.market_slug = m.slug.clone();
            d.remaining_s = m.time_remaining_sec();
            d.sigma = sigma;
            d.fair = fair_up;
            d.up_mid = up_mid;
            d.up_bid = q_up.bid;
            d.up_ask = q_up.ask;
            d.in_band = q_up.in_reward_band;
            d.cash = paper.state.cash_usdc;
            d.up_bal = paper.state.up_balance;
            d.down_bal = paper.state.down_balance;
            d.latent = latent;
            d.realized_pnl = paper.state.realized_pnl;
            d.fills = paper.state.fills;
            d.merges = paper.state.merges;
            // Carnet Up : 6 meilleurs niveaux de chaque côté pour visualisation.
            let mut bids = up_book.bids.clone();
            bids.sort_by(|a, b| b.price.partial_cmp(&a.price).unwrap());
            let mut asks = up_book.asks.clone();
            asks.sort_by(|a, b| a.price.partial_cmp(&b.price).unwrap());
            d.book_bids = bids.iter().take(6)
                .map(|l| crate::dashboard::BookLevel { price: l.price, size: l.size }).collect();
            d.book_asks = asks.iter().take(6)
                .map(|l| crate::dashboard::BookLevel { price: l.price, size: l.size }).collect();
        }

        tracing::info!(
            rem_s = m.time_remaining_sec(),
            fair = format!("{:.3}", fair_up),
            up_mid = format!("{:.3}", up_mid),
            up_bid = format!("{:.2}", q_up.bid), up_ask = format!("{:.2}", q_up.ask),
            cash = format!("{:.2}", paper.state.cash_usdc),
            up_bal = format!("{:.0}", paper.state.up_balance),
            dn_bal = format!("{:.0}", paper.state.down_balance),
            fills = paper.state.fills, merges = paper.state.merges,
            latent = format!("{:.2}", latent),
            "paper"
        );

        persist_ctr += 1;
        if persist_ctr % 5 == 0 {
            paper.persist();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn quote_inputs(
    cfg: &Config, m: &Market, fair: f64, mid: f64, sigma: f64, t_years: f64, inventory: f64,
) -> QuoteInputs {
    QuoteInputs {
        fair, mid, sigma, t_years, inventory,
        gamma: cfg.gamma, kappa: cfg.kappa, tick: m.tick_size,
        rewards_max_spread_cents: m.rewards_max_spread,
        our_size: cfg.our_size,
        reward_pool_per_min: cfg.reward_pool_per_min,
    }
}
