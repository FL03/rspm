//! One-shot diagnostic: authenticate the CLOB client exactly as the node does,
//! refresh the exact protocol + collateral authority, then read the bound
//! balance snapshot. Moves NO funds and places NO order.
//!
//! Run: `cargo run -p rspm --example refresh_balance --features clob`
//! (with POLYMARKET_PRIVATE_KEY + POLYMARKET_WALLET_ADDRESS in the env).

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = rspm::ClobClient::from_env(None).await?;
    let authority = client.refresh_authenticated_capital_authority().await?;
    let snapshot = client
        .authenticated_balance_allowance_for(authority)
        .await?;
    println!(
        "protocol={} generation={} executable_capital={}",
        authority.version().as_u32(),
        authority.generation(),
        snapshot.executable_capital()?
    );

    Ok(())
}
