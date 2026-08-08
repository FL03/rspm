/*
    Appellation: venue <module>
    Contrib: @FL03
*/
//! Pure venue-constraint helpers.
//!
//! This module deliberately has no HTTP, async-runtime, database, or SDK
//! dependency. Strategy components may use these deterministic calculations
//! without acquiring a data-plane capability.

/// Bump a below-minimum Kelly notional to the venue minimum only when the
/// caller's edge clears its strong-edge threshold.
///
/// The function is unit-agnostic. Callers must express `kelly_usd` and
/// `venue_min_usd` in the same units, and `edge` and
/// `strong_edge_threshold` on the same scale.
///
/// Returns:
///
/// - `Some(kelly_usd)` when the Kelly notional already clears the minimum.
/// - `Some(venue_min_usd)` when a strong edge permits the minimum-size bump.
/// - `None` when a weak edge should abstain instead of inflating the order.
#[must_use]
pub fn venue_min_bump(
    kelly_usd: f64,
    venue_min_usd: f64,
    edge: f64,
    strong_edge_threshold: f64,
) -> Option<f64> {
    if kelly_usd >= venue_min_usd {
        return Some(kelly_usd);
    }
    if edge >= strong_edge_threshold {
        Some(venue_min_usd)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::venue_min_bump;

    #[test]
    fn kelly_already_clears_venue_minimum() {
        assert_eq!(venue_min_bump(10.0, 5.0, 0.0, 0.05), Some(10.0));
    }

    #[test]
    fn strong_edge_bumps_to_venue_minimum() {
        assert_eq!(venue_min_bump(2.0, 5.0, 0.10, 0.05), Some(5.0));
    }

    #[test]
    fn weak_edge_abstains() {
        assert_eq!(venue_min_bump(2.0, 5.0, 0.01, 0.05), None);
    }

    #[test]
    fn edge_threshold_is_inclusive() {
        assert_eq!(venue_min_bump(2.0, 5.0, 0.05, 0.05), Some(5.0));
    }

    #[test]
    fn venue_minimum_is_inclusive() {
        assert_eq!(venue_min_bump(5.0, 5.0, 0.0, 0.05), Some(5.0));
    }
}
