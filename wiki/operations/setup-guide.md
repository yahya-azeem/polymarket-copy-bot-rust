---
tags: [operation]
sources: [README.md, src/config.rs, .env.example]
created: 2026-04-02
updated: 2026-05-17
---

# Setup Guide

**How to configure, build, and run the bot.**

## Prerequisites
- Rust toolchain (latest stable)
- A Polymarket account with funds deposited
- A Polygon RPC endpoint (Alchemy, QuickNode, etc.)
- For simulation mode: a PolySimulator API key

## Quick Start

### 1. Clone and build
```bash
git clone <repo>
cd polymarket-copy-bot-rust
cargo build --release
```

### 2. Configure `.env`
Copy `.env.example` to `.env` and fill in:

**Required:**
```env
PRIVATE_KEY=your_private_key_hex
RPC_URL=https://polygon-mainnet.g.alchemy.com/v2/YOUR_KEY
TARGET_WALLETS=0x123...,0x456...
```

**Account type (default AUTO):**
```env
POLYMARKET_SIGNATURE_TYPE=AUTO
```
AUTO detects your account type on startup. Set to `EOA`, `PROXY`, or `GNOSIS_SAFE` to force a type.

**Optional but useful:**
```env
POLYMARKET_GEO_TOKEN=your_geo_token  # For geo-restricted regions
USE_POLYWHALER_LEADERBOARD=true       # Auto-follow top traders
SIMULATION_MODE=true                   # Test without real funds
```

### 3. Run
```bash
cargo run --release
```

## Polymarket V2 Migration (April 28, 2026)

Polymarket migrated from USDC.e to **pUSD** as the CLOB collateral token. The bot automatically checks all three token types:
- **Native USDC** — `0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359`
- **USDC.e (legacy)** — `0x2791Bca1f2de4661ED88A30C99A7a9449Aa84174`
- **pUSD (V2)** — `0xC011a7E12a19f7B1f670d46F03B03f3342E82DFB`

Your balance carries over — no configuration changes needed. If you see $0 balance, the bot now fixes this by including pUSD in the CLOB balance audit.

## Simulation Mode
Set `SIMULATION_MODE=true` and provide `POLYSIMULATOR_API_KEY` to test without risking real funds. The PolySimulator API simulates order book matching and returns fill results.

## Environment Variables Reference
See [[wiki/entities/config]] for the full list of all env vars with defaults.

## First Run Checklist
- [ ] Wallet has funds deposited on Polymarket (USDC, USDC.e, or pUSD)
- [ ] Account is "activated" for CLOB trading (has made at least one trade or deposit on Polymarket.com)
- [ ] Target wallets are valid Polymarket addresses
- [ ] Geo token set if in a restricted region
