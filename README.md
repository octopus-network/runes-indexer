# Runes Indexer (formerly Ord Canister)

![GitHub License](https://img.shields.io/github/license/octopus-network/runes-indexer)
![Crates.io Version](https://img.shields.io/crates/v/runes-indexer-interface)


An onchain runes indexer on the Internet Computer

## Overview

Runes Indexer is a canister deployed on the [IC](https://internetcomputer.org/) that continuously fetches Bitcoin blocks through [HTTPS outcalls](https://internetcomputer.org/https-outcalls) from Bitcoin RPC. The blocks are verified using IC's [Bitcoin integration](https://internetcomputer.org/docs/current/references/bitcoin-how-it-works). Once verified, the indexer parses and indexes runes information within each block. The implementation aligns with [ord 0.22.1](https://github.com/ordinals/ord/releases/tag/0.22.1).

**Deployment Status:**
- New Mainnet Deployment: [`kzrva-ziaaa-aaaar-qamyq-cai`](https://dashboard.internetcomputer.org/canister/kzrva-ziaaa-aaaar-qamyq-cai) (Maintained by [Omnity Network](https://omnity.network/))

## Repository Components

The repository consists of two main components:

### canister
The core implementation of the runes indexer that:
- Fetches blocks via RPC
- Validates blocks
- Indexes rune information
- Handles blockchain reorgs
- Provides query interfaces for services

### interface
Provides Rust type definitions for interacting with the Runes Indexer canister. These types enable both Rust applications and other canisters to handle API responses in a type-safe manner.

## API Reference

The Runes Indexer canister provides several query methods to access indexed rune data. All methods are query calls, which means they are fast and do not consume cycles.

### get_latest_block
Returns the latest indexed block height and hash.

Type signature:
```candid
get_latest_block : () -> (nat32, text) query;
```

Parameters:
- None

Returns:
- `nat32`: The block height
- `text`: The block hash

Example:
```bash
dfx canister call runes-indexer get_latest_block --ic
# Returns:
(
  879_823 : nat32,
  "00000000000000000001aa3e25bf07fee9bacb44e78506b158f6928fd41331d2",
)
```

### get_etching
Retrieves the rune_id that was etched in a specific transaction.

Type signature:
```candid
get_etching : (text) -> (opt GetEtchingResult) query;
```

Parameters:
- `text`: Transaction ID (txid)

Returns:
- `opt GetEtchingResult`: Optional record containing:
  - `confirmations`: `nat32` - Number of confirmations
  - `rune_id`: `text` - The etched rune identifier

Example:
```bash
dfx canister call runes-indexer get_etching '("d66de939cb3ddb4d94f0949612e06e7a84d4d0be381d0220e2903aad68135969")' --ic
# Returns:
(opt record {
  confirmations = 39_825 : nat32;
  rune_id = "840000:846"
})
```

### get_rune
Retrieves detailed information about a rune using its spaced rune name.

Type signature:
```candid
get_rune : (text) -> (opt RuneEntry) query;
```

Parameters:
- `text`: Spaced rune name (e.g., "HOPE•YOU•GET•RICH")

Returns:
- `opt RuneEntry`: Optional record containing comprehensive rune information:
  - `confirmations`: `nat32` - Number of confirmations
  - `rune_id`: `text` - Unique rune identifier

Example:
```bash
dfx canister call runes-indexer get_rune '("HOPE•YOU•GET•RICH")' --ic
# Returns:
(
  opt record {
    confirmations = 39_825 : nat32;
    mints = 81_000 : nat;
    terms = opt record {
      cap = opt (81_000 : nat);
      height = record { opt (840_001 : nat64); opt (844_609 : nat64) };
      offset = record { null; null };
      amount = opt (10_000_000 : nat);
    };
    etching = "d66de939cb3ddb4d94f0949612e06e7a84d4d0be381d0220e2903aad68135969";
    turbo = true;
    premine = 0 : nat;
    divisibility = 2 : nat8;
    spaced_rune = "HOPE•YOU•GET•RICH";
    number = 431 : nat64;
    timestamp = 1_713_571_767 : nat64;
    block = 840_000 : nat64;
    burned = 48_537_380 : nat;
    rune_id = "840000:846";
    symbol = opt "🧧";
  },
)
```

### get_rune_by_id
Similar to `get_rune`, but uses the rune_id as identifier instead of the spaced rune name.

Type signature:
```candid
get_rune_by_id : (text) -> (opt RuneEntry) query;
```

Parameters:
- `text`: Rune ID (e.g., "840000:846")

Returns:
- Same as `get_rune`

### get_rune_balances_for_outputs
Retrieves rune balances for a list of transaction outputs.

Type signature:
```candid
get_rune_balances_for_outputs : (vec text) -> (Result) query;
```

Parameters:
- `vec text`: Array of outpoints in format "txid:vout"

Returns:
- `Result`: Variant containing either:
  - `Ok`: Vector of optional rune balance records:
    - `confirmations`: `nat32`
    - `divisibility`: `nat8`
    - `amount`: `nat`
    - `rune_id`: `text`
    - `symbol`: `opt text`
  - `Err`: Error information if the query fails

Example:
```bash
dfx canister call runes-indexer get_rune_balances_for_outputs '(vec {
  "8f6ebbc114872da3ba105ce702e4793bacc1cf199940f217b38c0bd8d9bfda3a:0";
  "f43158badf8866da0b859de4bffe73c2a910996310927c72431cf486e25dd3ab:1"
})' --ic
# Returns:
(
  variant {
    Ok = vec {
      opt vec {
        record {
          confirmations = 112 : nat32;
          divisibility = 2 : nat8;
          amount = 19_000_000 : nat;
          rune_id = "840000:846";
          symbol = opt "🧧";
        };
      };
      opt vec {
        record {
          confirmations = 61 : nat32;
          divisibility = 2 : nat8;
          amount = 2_092_100 : nat;
          rune_id = "840000:846";
          symbol = opt "🧧";
        };
      };
    }
  },
)
```

## Local Development
Refer to [development-guide.md](./development-guide.md)

## License
[MIT](LICENSE).