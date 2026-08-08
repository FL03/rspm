/*
    Appellation: helpers <module>
    Created At: 2026.08.08:08:29:19
    Contrib: @FL03
*/
use rust_decimal::Decimal;

pub fn clob_price_decimal(price: f64) -> anyhow::Result<Decimal> {
    if !price.is_finite() || price <= 0.0 || price >= 1.0 {
        return Err(anyhow::anyhow!("invalid price: {price}"));
    }
    price
        .to_string()
        .parse()
        .map_err(|_| anyhow::anyhow!("invalid price: {price}"))
}

pub fn clob_share_decimal(size: f64) -> anyhow::Result<Decimal> {
    if !size.is_finite() {
        return Err(anyhow::anyhow!("invalid size: {size}"));
    }
    let size = size
        .to_string()
        .parse::<Decimal>()
        .map_err(|_| anyhow::anyhow!("invalid size: {size}"))?
        .trunc_with_scale(2);
    if size <= Decimal::ZERO {
        return Err(anyhow::anyhow!(
            "share size truncates to zero at venue precision"
        ));
    }
    Ok(size)
}

/// Normalize canonical shares to the CLOB SDK's two-decimal lot scale without
/// ever increasing the admitted quantity.
pub fn normalize_clob_share_count(size: f64) -> anyhow::Result<f64> {
    clob_share_decimal(size)?
        .to_string()
        .parse()
        .map_err(|_| anyhow::anyhow!("normalized share count is not representable"))
}
