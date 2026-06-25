# Polymarket MM Bot — Monolithe Rust (BTC Up/Down 5 min)

Bot de market-making / sniping HFT sur les marchés Polymarket **« BTC Up/Down 5 min »**,
en architecture distribuée à deux nœuds. **Mode paper par défaut** (`DRY_RUN=true`) — aucun
ordre réel n'est envoyé tant que le jalon signing (J11) n'est pas validé.

## Architecture

```
[Binance ~Tokyo] --depth20@100ms--> NŒUD RADAR (ap-northeast-1)
                                    OBI + vélocité @10Hz, carnet L2 BTreeMap
                                    flash-crash → signal "KILL" (1 octet UDP)
                                             │  AWS Global Accelerator
                                             ▼
                                    NŒUD EXÉCUTEUR (eu-west-1 Dublin)
                                    pricing BS + quotes A-S reward-adjusted
                                    paper trading + fusion CTF → CLOB (Londres)
```

Un **binaire unique** `polymarket_mm_bot`. Le rôle est choisi par `BOT_ROLE`
(`radar` | `executor` | `combined`) ou dérivé de `AWS_REGION`.

## Lancer en local (un seul Mac)

```bash
cd backend
cp .env.example .env          # DRY_RUN=true par défaut
cargo build
BOT_ROLE=combined cargo run    # radar + exécuteur reliés par loopback in-process
```

Dashboard : http://127.0.0.1:8767

### Deux terminaux (transport UDP réel)

```bash
# Terminal 1 (exécuteur) :
BOT_ROLE=executor SIGNAL_ADDR=127.0.0.1:9001 cargo run
# Terminal 2 (radar) :
BOT_ROLE=radar SIGNAL_TARGET=127.0.0.1:9001 cargo run
```

## Tests

```bash
cd backend && cargo test     # moteurs pricing / volatilité / risk / radar / inventaire
```

## Modules (jalons)

| Module | Rôle |
|---|---|
| `connectors/binance.rs` | WS L2 `depth20@100ms` + strike via klines (J1) |
| `engines/radar.rs` | OBI + vélocité @10Hz → KILL (J2) |
| `signal.rs` | transport loopback ⇄ UDP (J3) |
| `engines/volatility.rs` | sigma glissant 2s + floor (J4) |
| `engines/pricing.rs` | Black-Scholes binaire N(d2) (J5) |
| `connectors/polymarket.rs` | marché 5min + carnet CLOB (J6) |
| `engines/risk.rs` | Avellaneda-Stoikov + reward-adjusted spread (J7) |
| `inventory.rs` | paper, fusion CTF, persistance atomique (J8) |
| `dashboard.rs` | API locale + frontend monitoring (J9) |
| `signer.rs` | EIP-712 pré-computé (J11, à venir) |

## Déploiement AWS

- **Radar** : EC2 `c7in.xlarge` en `ap-northeast-1` (Tokyo). `BOT_ROLE=radar`,
  `SIGNAL_TARGET=<ip_executeur>:9001`.
- **Exécuteur** : EC2 `c6in.large` en `eu-west-1` (Dublin). `BOT_ROLE=executor`.
- Build léger : pour une `t3.micro`, cross-compiler en local
  (`cargo build --release --target x86_64-unknown-linux-gnu`) et n'envoyer que le binaire.
- **Security group** : ouvrir 22 (SSH équipe), 80/443 (Nginx dashboard), 9001/udp
  (signal inter-régions, restreint aux IP des nœuds). Sortant libre vers Binance/Polymarket.

```bash
./deploy.sh   # git pull → cargo build --release → frontend nginx → systemd restart
```

## ⚠️ Avant tout passage en LIVE
- **Calibration** (`AS_GAMMA`, `AS_KAPPA`, `VOLATILITY_FLOOR`, seuils OBI/vélocité) : à régler
  en paper. Avec les valeurs par défaut, le fair BS peut diverger du mid (vol 5 min minuscule)
  → le bot prend des positions directionnelles plutôt que du MM équilibré.
- **Signing (J11)** : EIP-712 sig_type 3 non encore implémenté. `DRY_RUN=false` ne fonctionnera
  qu'une fois `signer.rs` en place et le type de signature confirmé.
- **Conformité** : vérifier l'accès Polymarket selon la juridiction avant tout ordre réel.
