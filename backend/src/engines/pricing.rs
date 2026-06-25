//! Moteur de pricing (J5) : modèle de Black-Scholes binaire.
//!
//! Sur un marché Up/Down, la probabilité que BTC finisse au-dessus du strike
//! (= "Up") est, sous BS avec taux nul sur l'horizon court :
//!     d2 = (ln(spot/strike) − ½·σ²·t) / (σ·√t)
//!     P(Up) = N(d2)
//! où σ est la volatilité annualisée (J4) et t l'horizon restant en années.

use statrs::distribution::{ContinuousCDF, Normal};

/// CDF de la loi normale standard.
fn normal_cdf(x: f64) -> f64 {
    // Normal::new(0,1) ne peut pas échouer ; valeurs finies garanties par les bornes.
    Normal::new(0.0, 1.0).unwrap().cdf(x)
}

/// Probabilité "Up" = N(d2). Robuste aux cas dégénérés (t→0, σ→0).
pub fn fair_up_probability(spot: f64, strike: f64, sigma_annual: f64, t_years: f64) -> f64 {
    if spot <= 0.0 || strike <= 0.0 {
        return 0.5;
    }
    // À l'échéance (ou vol nulle) : indicatrice du strike.
    if t_years <= 0.0 || sigma_annual <= 0.0 {
        return if spot > strike {
            1.0
        } else if spot < strike {
            0.0
        } else {
            0.5
        };
    }

    let d2 = ((spot / strike).ln() - 0.5 * sigma_annual * sigma_annual * t_years)
        / (sigma_annual * t_years.sqrt());
    normal_cdf(d2)
}

/// Convertit un horizon en secondes vers des années (base 365j).
pub fn years_from_secs(secs: f64) -> f64 {
    secs / (365.0 * 24.0 * 3600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const Y_5MIN: f64 = 300.0 / (365.0 * 24.0 * 3600.0);

    #[test]
    fn at_the_money_is_near_half() {
        let p = fair_up_probability(60000.0, 60000.0, 0.6, Y_5MIN);
        assert!((p - 0.5).abs() < 0.02, "p={p}");
    }

    #[test]
    fn deep_in_the_money_near_one() {
        // Spot très au-dessus du strike, peu de temps → quasi certain "Up".
        let p = fair_up_probability(61000.0, 60000.0, 0.6, Y_5MIN);
        assert!(p > 0.95, "p={p}");
    }

    #[test]
    fn deep_out_of_money_near_zero() {
        let p = fair_up_probability(59000.0, 60000.0, 0.6, Y_5MIN);
        assert!(p < 0.05, "p={p}");
    }

    #[test]
    fn expiry_is_indicator() {
        assert_eq!(fair_up_probability(60001.0, 60000.0, 0.6, 0.0), 1.0);
        assert_eq!(fair_up_probability(59999.0, 60000.0, 0.6, 0.0), 0.0);
    }

    #[test]
    fn higher_vol_pulls_toward_half() {
        // Plus de vol → plus d'incertitude → proba plus proche de 0.5.
        let low = fair_up_probability(60200.0, 60000.0, 0.3, Y_5MIN);
        let high = fair_up_probability(60200.0, 60000.0, 1.5, Y_5MIN);
        assert!((high - 0.5).abs() < (low - 0.5).abs(), "low={low} high={high}");
    }
}
