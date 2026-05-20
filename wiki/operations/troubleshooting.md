---
tags: [operation, troubleshooting]
sources: [src/trader.rs, src/bot.rs]
created: 2026-04-02
updated: 2026-05-17
---

# Troubleshooting

**Common issues and their fixes.**

## "Polymarket authentication failed (400 Bad Request)"

**Cause**: Account not activated for CLOB trading.

**Fix**: Go to Polymarket.com, sign in with this wallet, and make a small trade or deposit. The account needs to be "enabled" for CLOB API access — the bot's authentication request will fail 400 if this hasn't been done.

## "Polymarket authentication failed (403 Forbidden)"

**Cause**: Cloudflare blocking the API request.

**Fix**: You must first log in on Polymarket.com via your browser with this wallet. The Cloudflare challenge must be satisfied in a real browser session. After that, the bot may work — or you may need a `POLYMARKET_GEO_TOKEN`.

## Balance shows $0.00 on startup

**Possible causes (in order of likelihood)**:

1. **pUSD migration (V2)** — On **April 28, 2026** Polymarket migrated from USDC.e to **pUSD** as collateral. If you deposited USDC.e before this date, your balance is now tracked as pUSD. The bot checks all three tokens (USDC, USDC.e, pUSD) automatically.
2. **No funds deposited on Polymarket CLOB** — On-chain balance ≠ exchange balance. You must deposit on Polymarket.com.
3. **Wrong signature type** — AUTO detection may have failed. Try setting `POLYMARKET_SIGNATURE_TYPE=EOA` or `GNOSIS_SAFE` explicitly.
4. **Balance-sync call failed** — The `update_balance_allowance` call at init failed. Usually harmless but can result in 0 balance for new accounts. Try running the bot again.
5. **Account not activated** — See the 400 error fix above.

## "Signature type is GnosisSafe but no proxy address"

**Cause**: AUTO-detect found a proxy wallet but the address was invalid or empty.

**Fix**: Check your wallet on Polymarket Gamma API: `https://gamma-api.polymarket.com/public-profile?address=YOUR_ADDRESS`. Look for the `proxyWallet` field. If it's `0x000...0000`, you have a regular EOA — set `POLYMARKET_SIGNATURE_TYPE=EOA`.

## Geoblock bypass not working

**Cause**: The `POLYMARKET_GEO_TOKEN` approach is fragile. Blocked markets may still fail.

**Fix**: The best workaround is to use a VPN or run the bot from a non-restricted region. The geoblock check at init is best-effort.

## Stale orders not being cancelled

**Cause**: `ORDER_TIMEOUT_MINUTES` too high, or the order was placed by a different session.

**Fix**: Reduce `ORDER_TIMEOUT_MINUTES` (default 10). Orders placed by other sessions/clients won't be cancelled by this bot.

## "Sizing model unavailable" warnings

**Cause**: The Data API (`/value` endpoint) for the whale's balance is returning errors or 0.

**Fix**: The bot falls back to `POSITION_MULTIPLIER`. This is usually fine — the warning is informational.

## General Debugging

- Check `bot_output.log` for the full startup sequence
- Set `RUST_LOG=debug` or `RUST_LOG=trace` for verbose logging
- Use `SIMULATION_MODE=true` to test without real funds
