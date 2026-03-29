# Polymarket Copy Trading Bot (Rust)
 
A high-performance **Polymarket copy bot** written in Rust for copy trading on Polymarket prediction markets. It watches a list of target wallets and automatically copies `BUY` and `SELL` trades with sophisticated sizing, risk management, and 24/7 operation capabilities.
 
## 🚀 Key Features
 
### 1. Dual-Path Monitoring
- **REST Polling**: Continuously polls the Polymarket Data API for the latest trades from a list of **target wallets**.
- **WebSocket Ingestion**: Optional WebSocket subscription to the Polymarket CLOB for near-instant (low-latency) trade detection across all monitored wallets.
 
### 2. Intelligent Trade Execution
- **Order Types**: Supports `LIMIT`, `FOK` (Fill-or-Kill), and `FAK` (Fill-and-Kill) orders.
- **Slippage Control**: Configurable slippage tolerance to ensure you don't overpay for entries or under-receive for exits.
- **Automatic Approvals**: Checks and maintains necessary token approvals for `USDC.e` and `CTF` contracts.
 
### 3. Budget-Aware Adaptive Sizing (Enabled by Default)
The bot uses an benchmark-driven sizing model to mirror target conviction while protecting your wallet:
- **Whale Awareness**: If the target's balance is much larger than yours (Whale Case), the bot will attempt to mirror their **literal dollar amounts** instead of a tiny proportional fraction.
- **Safety Wall**: High-risk trades are automatically capped at a configurable percentage of **your current wallet balance** (Default: 10%).
- **Calculation**: `Final Size = min(Literal Amount, Your Balance * MAX_PERCENT_OF_BALANCE, MAX_TRADE_SIZE)`
- **Conviction Mirroring**: Mirrors the proportional size if the target bets a significant portion of their own funds.
 
### 4. Robust Risk Management
- **Max Trade Size**: Caps any single trade to a specific USDC amount.
- **Session Notional Cap**: Maximum total volume allowed in a single session.
- **Market Notional Cap**: Maximum exposure allowed per individual market.
- **Copy Sells**: Toggleable ability to copy `SELL` trades (requires holding the position).
 
### 5. 🧪 Simulation Mode (Paper Trading)
- Support for **PolySimulator.com** to test your bot without real funds.
- Set `SIMULATION_MODE=true` and provide a `POLYSIMULATOR_API_KEY` to trade against a paper balance mirroring the real CLOB mid-prices.
 
### 6. 24/7 Windows Daemon
- Includes a dedicated PowerShell setup script (`setup-daemon.ps1`) to:
  - Disable Windows sleep/hibernation when the laptop lid is closed.
  - Provide instructions for running as a permanent Windows Service.
 
---
 
## 🛠️ Setup & Installation
 
### Prerequisites
- [Rust](https://rustup.rs/) (latest stable)
- A Polygon wallet with `POL` (for gas) and `USDC.e` (collateral)
- A Polymarket account associated with your wallet
 
### Steps
1. **Clone & Configure**:
   ```bash
   cp .env.example .env
   ```
2. **Fill Required Variables** in `.env`:
   - `TARGET_WALLETS`: The wallet addresses to follow (comma-separated).
   - `PRIVATE_KEY`: Your wallet's private key (only if `SIMULATION_MODE=false`).
   - `RPC_URL`: Your Polygon RPC URL (QuickNode recommended).
   - `SIMULATION_MODE`: Set to `true` for paper trading.
   - `POLYSIMULATOR_API_KEY`: Required if `SIMULATION_MODE=true`.
 
3. **Build**:
   ```bash
   cargo build --release
   ```
 
4. **Power Configuration (Windows Laptops)**:
   Run PowerShell as Administrator:
   ```powershell
   .\setup-daemon.ps1
   ```
 
5. **Run**:
   ```bash
   ./target/release/polymarket-copy-bot-rust
   ```
 
---
 
## ⚙️ Configuration (.env)
 
| Variable | Description | Default |
| :--- | :--- | :--- |
| `SIMULATION_MODE` | Enable Paper Trading via PolySimulator | `false` |
| `POLYSIMULATOR_API_KEY` | API Key from PolySimulator.com | `` |
| `USE_SIZING_MODEL` | Enable proportional balance-based sizing | `true` |
| `SIZING_MULTIPLIER` | Multiplier for the sizing model output | `2.0` |
| `MAX_TRADE_SIZE` | Hard cap on any single order size (USDC) | `100` |
| `COPY_SELLS` | Whether to copy SELL orders | `true` |
| `ORDER_TYPE` | `LIMIT`, `FOK`, or `FAK` | `FOK` |
| `POLL_INTERVAL` | Milliseconds between REST poll checks | `2000` |
 
---
 
## 🛡️ Security & Best Practices
- **Dedicated Wallet**: Use a dedicated wallet with only the funds you intend to trade.
- **Never Share Private Keys**: Ensure your `.env` file is never committed or shared.
- **Dry Run**: Start with `SIMULATION_MODE=true` to verify behavior before switching to real funds.
 
## ⚖️ Disclaimer
Trading carries risk. Use this software at your own risk. The authors are not responsible for financial losses.
