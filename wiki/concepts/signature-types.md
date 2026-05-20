---
tags: [concept, design-decision]
sources: [src/trader.rs]
created: 2026-04-02
updated: 2026-04-02
---

# Signature Types

**Polymarket supports multiple wallet types, each with different authentication mechanisms. The bot auto-detects the correct type.**

## The Three Types

| Type | Polymarket API Value | Description | When Used |
|------|---------------------|-------------|-----------|
| EOA | 0 | Standard externally-owned wallet (MetaMask, etc.) | Regular wallets |
| Polymarket Proxy | 1 | Old Magic/Gmail-linked wallets | Legacy proxy accounts |
| Polymarket Gnosis Safe | 2 | — | (Not directly used by this bot) |
| Gnosis Safe | 3 | Smart contract wallet (Safe) | Proxy accounts detected via Gamma API |

## Auto-Detection Flow

When `POLYMARKET_SIGNATURE_TYPE=AUTO` (the default):

1. Query `https://gamma-api.polymarket.com/public-profile?address={your_address}`
2. If response contains a non-zero `proxyWallet` field → detected as Gnosis Safe
3. If no proxy → detected as EOA
4. Store result in `AtomicU8` for lock-free reads

## Critical Mapping

The bot maps detected types to SDK types like this:
- **Gnosis Safe (detected)** → SDK `SignatureType::GnosisSafe` (value 3)
- **Proxy (detected)** → SDK `SignatureType::Proxy` (value 1)
- **EOA (detected)** → SDK `SignatureType::Eoa` (value 0)

This mapping uses `unsafe { std::mem::transmute() }` because the SDK doesn't expose all enum variants directly.

## Maker Override

For Proxy/Safe accounts, the bot overrides the `maker` field on every order with the detected proxy address. This is essential — without it, the CLOB would use the EOA address as the maker and the order would fail with "insufficient balance" or "invalid signature."

## Why Not Just EOA?

Many Polymarket users log in via email (Magic link) or Google, which creates a **proxy wallet** behind the scenes. The actual private key only controls an EOA, but the CLOB expects trades to be signed with the proxy as the funder/maker. The auto-detection handles this transparently.

## Related

- [[wiki/entities/trade-executor]] — see the `discover_account_details` method
- [[wiki/operations/troubleshooting]] — "400 Bad Request" / "not activated" issues
