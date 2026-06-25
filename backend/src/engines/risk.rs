//! Moteur de risque (J7) : Avellaneda-Stoikov + reward-adjusted spread.
//!
//! 1. **Skew d'inventaire (A-S)** :
//!      reservation = fair − inventory · γ · σ² · t
//!      half_spread_AS = ½ · γ · σ² · t + (1/γ) · ln(1 + γ/κ)
//! 2. **Reward-adjusted** : le profit vient surtout des liquidity rewards Polymarket
//!    (score `S(v,s) = ((v−s)/v)²`). On estime notre part de score vs les concurrents
//!    présents dans la bande `rewardsMaxSpread`, et on **resserre** le spread d'une
//!    subvention proportionnelle → cotation plus agressive près du mid.
//!
//! Tous les prix sont des probabilités Polymarket dans [0.01, 0.99].

use crate::connectors::polymarket::PolyBook;

#[derive(Debug, Clone)]
pub struct QuoteInputs {
    pub fair: f64,            // proba "Up" du modèle BS (J5)
    pub mid: f64,            // mid du carnet Polymarket
    pub sigma: f64,          // vol annualisée (J4)
    pub t_years: f64,        // horizon restant
    pub inventory: f64,      // position NETTE (net = up − down ; convention R1)
    pub gamma: f64,          // aversion au risque A-S (pilote le skew d'inventaire)
    pub kappa: f64,          // (conservé pour compat ; non utilisé depuis R2)
    pub base_half_spread_cents: f64, // R2 : demi-spread de base (cents)
    pub tick: f64,           // pas de prix (0.01)
    pub rewards_max_spread_cents: f64, // v, en cents (ex 4.5)
    pub our_size: f64,       // taille de nos ordres (pour le score reward)
    pub reward_pool_per_min: f64, // pool de reward estimé $/min (config/API)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QuoteResult {
    pub reservation: f64,
    pub half_spread_as: f64,
    pub expected_reward: f64,
    pub half_spread_final: f64,
    pub bid: f64,
    pub ask: f64,
    pub in_reward_band: bool,
}

/// Score de reward Polymarket d'un ordre à `s` cents du mid : `((v−s)/v)²`.
pub fn reward_score(v_cents: f64, s_cents: f64) -> f64 {
    if v_cents <= 0.0 || s_cents < 0.0 || s_cents > v_cents {
        return 0.0;
    }
    let r = (v_cents - s_cents) / v_cents;
    r * r
}

/// Estime le score total des concurrents présents dans la bande de reward
/// (somme de `score · size` des deux côtés du carnet, dans `v` cents du mid).
pub fn competitor_q(book: &PolyBook, mid: f64, v_cents: f64) -> f64 {
    let mut q = 0.0;
    for lvl in book.bids.iter().chain(book.asks.iter()) {
        let s_cents = (lvl.price - mid).abs() * 100.0;
        if s_cents <= v_cents {
            q += reward_score(v_cents, s_cents) * lvl.size;
        }
    }
    q
}

fn clamp_tick(p: f64, tick: f64) -> f64 {
    let snapped = (p / tick).round() * tick;
    snapped.clamp(0.01, 0.99)
}

/// Calcule les quotes A-S reward-adjusted.
pub fn compute_quote(inp: &QuoteInputs, book: &PolyBook) -> QuoteResult {
    let var_t = inp.sigma * inp.sigma * inp.t_years;

    // 1. Prix de réservation. MM NEUTRE : on cote autour du **mid du marché**
    //    (et non du fair — on ne parie pas sur notre modèle), décalé par le skew
    //    d'inventaire A-S (sur l'inventaire NET, R1). +net (long Up) → réservation
    //    sous le mid → on penche vendeur Up pour revenir à plat. Le `fair` reste
    //    calculé pour le monitoring (divergence fair vs mid) mais ne pilote pas la quote.
    let reservation = inp.mid - inp.inventory * inp.gamma * var_t;

    // R2 : le terme d'arrivée A-S (1/γ)·ln(1+γ/κ) est mal échelonné en [0,1]
    //   (≈0.645, toujours écrasé au plafond) → on le remplace par un demi-spread de
    //   BASE configurable (cents), élargi marginalement par le risque (½γσ²t) et par
    //   l'inventaire (offloader plus large quand on est chargé).
    let base = inp.base_half_spread_cents / 100.0;
    let inv_widen = inp.gamma * var_t * inp.inventory.abs();
    let half_spread_as = base + 0.5 * inp.gamma * var_t + inv_widen;

    // 2. Subvention reward : on estime NOTRE score à notre spread (maintenant en cents).
    let v = inp.rewards_max_spread_cents;
    let s_ref_cents = (half_spread_as * 100.0).min(v * 0.5).max(0.0);
    let our_score = reward_score(v, s_ref_cents) * inp.our_size * 2.0; // deux côtés
    let comp_q = competitor_q(book, inp.mid, v);
    let share = if our_score + comp_q > 0.0 {
        our_score / (our_score + comp_q)
    } else {
        0.0
    };
    let expected_reward = share * inp.reward_pool_per_min;

    // Plus l'ExpectedReward est élevé, plus on resserre (en unités de prix).
    // Calibrage simple : 1 $/min de reward attendu ⇒ jusqu'à `reward_k` de
    // resserrement, borné pour ne jamais croiser le mid.
    let reward_k = 0.02; // sensibilité (à calibrer en paper)
    let subsidy = (expected_reward * reward_k).min(half_spread_as.max(0.0));
    // Plafonné à la bande de reward : coter plus large n'éligibilise aucun reward
    // et le spread A-S brut n'est pas fiable à l'échelle d'un marché de probabilité
    // (calibration γ/κ à affiner en paper — cf. plan).
    let band_price = inp.rewards_max_spread_cents / 100.0;
    let half_spread_final = (half_spread_as - subsidy).clamp(inp.tick, band_price);

    // 3. Quotes autour du prix de réservation, clampées.
    let bid = clamp_tick(reservation - half_spread_final, inp.tick);
    let ask = clamp_tick(reservation + half_spread_final, inp.tick);

    // Dans la bande de reward si les deux côtés sont à ≤ v cents du mid.
    let in_reward_band = (bid - inp.mid).abs() * 100.0 <= v && (ask - inp.mid).abs() * 100.0 <= v;

    QuoteResult {
        reservation,
        half_spread_as,
        expected_reward,
        half_spread_final,
        bid,
        ask,
        in_reward_band,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connectors::polymarket::{Level, PolyBook};

    fn base_inputs() -> QuoteInputs {
        QuoteInputs {
            fair: 0.50,
            mid: 0.50,
            sigma: 0.6,
            t_years: 300.0 / (365.0 * 24.0 * 3600.0),
            inventory: 0.0,
            gamma: 0.1,
            kappa: 1.5,
            base_half_spread_cents: 2.0,
            tick: 0.01,
            rewards_max_spread_cents: 4.5,
            our_size: 50.0,
            reward_pool_per_min: 0.0,
        }
    }

    fn empty_book() -> PolyBook {
        PolyBook::default()
    }

    #[test]
    fn symmetric_when_flat_inventory() {
        let inp = base_inputs();
        let q = compute_quote(&inp, &empty_book());
        // Réservation = fair quand inventaire nul.
        assert!((q.reservation - 0.50).abs() < 1e-9);
        // Quotes symétriques autour du mid, à un tick près (arrondi au pas).
        assert!((0.50 - q.bid - (q.ask - 0.50)).abs() <= inp.tick + 1e-9);
        assert!(q.bid < 0.50 && q.ask > 0.50 && q.bid < q.ask);
    }

    #[test]
    fn long_inventory_skews_down() {
        let mut inp = base_inputs();
        inp.inventory = 100.0; // long Up → on veut vendre → réservation sous le fair
        let q = compute_quote(&inp, &empty_book());
        assert!(q.reservation < 0.50, "reservation={}", q.reservation);
    }

    #[test]
    fn reward_score_peaks_at_mid() {
        assert!((reward_score(4.5, 0.0) - 1.0).abs() < 1e-9);
        assert!(reward_score(4.5, 2.25) < reward_score(4.5, 0.5));
        assert_eq!(reward_score(4.5, 5.0), 0.0); // hors bande
    }

    #[test]
    fn higher_reward_tightens_spread() {
        let mut inp = base_inputs();
        inp.reward_pool_per_min = 0.0;
        let no_reward = compute_quote(&inp, &empty_book()).half_spread_final;
        inp.reward_pool_per_min = 100.0; // gros pool, on est seuls → grosse subvention
        let with_reward = compute_quote(&inp, &empty_book()).half_spread_final;
        assert!(with_reward <= no_reward, "{with_reward} vs {no_reward}");
    }

    #[test]
    fn competitor_liquidity_reduces_our_share() {
        let mut inp = base_inputs();
        inp.reward_pool_per_min = 100.0;
        // Carnet vide → grosse subvention.
        let alone = compute_quote(&inp, &empty_book()).expected_reward;
        // Carnet dense près du mid → notre part chute.
        let mut crowded = PolyBook::default();
        for i in 1..=4 {
            let off = i as f64 * 0.01;
            crowded.bids.push(Level { price: 0.50 - off, size: 5000.0 });
            crowded.asks.push(Level { price: 0.50 + off, size: 5000.0 });
        }
        let contested = compute_quote(&inp, &crowded).expected_reward;
        assert!(contested < alone, "contested={contested} alone={alone}");
    }
}
