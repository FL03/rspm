# rspm

[![crates.io](https://img.shields.io/crates/v/rspm?style=for-the-badge&logo=rust)](https://crates.io/crates/rspm)
[![docs.rs](https://img.shields.io/docsrs/rspm?style=for-the-badge&logo=docs.rs)](https://docs.rs/rspm)
[![GitHub License](https://img.shields.io/github/license/FL03/rspm?style=for-the-badge&logo=github)](LICENSE)

***

_**Warning: The library is currently in the early stages of development and is not yet ready for production use.**_

`rspm` is a Rust native Polymarket client supporting realtime streams, secure trading, and more.

## Features

To enable access to the various APIs exposed by Polymarket, toggle one or more of the following features:

- [ ] `clob`: enables the CLOB api
- [ ] `data`: enables the Polymarket data-api
- [ ] `gamma`: enables the Polymarket Gamma API (default)

### Optional Dependencies

- [x] `serde` - enables serialization using [`serde`](https://serde.rs/)

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies.rspm]
features = []
version = "0.0.x"
```

### Examples

#### _Example #1:_ Basic Usage

```rust
    extern crate rspm;

    fn main() -> anyhow::Result<()> {

        Ok(())
    }

```

## Getting Started

### Prerequisites

Ensure you have the latest version of Rust installed. You can install Rust using [rustup](https://rustup.rs/).

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After installation, I always recommend ensuring that rustup is updated to the latest version:

```bash
rustup update
```

And to add the latest nightly toolchain, which is often useful for development:

```bash
rustup toolchain install nightly
```

Additionally, you may wish to install the `cargo-binstall` utility to streamline the installation of Rust binaries:

```bash
cargo install cargo-binstall
```

If necessary, add the `wasm32-*` target(s) if you plan to compile for WebAssembly:

```bash
rustup target add wasm32-unknown-unknown wasm32-p1 wasm32-p2
```

### Building from the source

Start by cloning the repository

```bash
git clone https://github.com/FL03/rspm.git -b main --depth 1
```

Then, navigate to the project directory:

```bash
cd rspm
```

Once you're in the project directory, you can build the project using `cargo`:

```bash
cargo build --workspace --release --all-features
```

Or, if you want to run the tests, you can use:

```bash
cargo test --workspace --release --all-features
```

### Upstream SDK patch

The workspace activates `patches/polymarket_client_sdk_v2` through
`[patch.crates-io]`. This makes standalone clones use the same authenticated
submission and WebSocket lifecycle contracts as downstream consumers. See
`patches/polymarket_client_sdk_v2/AXIOM_PATCH.md` for provenance and removal
criteria.

## Contributing

Pull requests are welcome. For major changes, please open an issue first
to discuss what you would like to change.

Please make sure to update tests as appropriate.
