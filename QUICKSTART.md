---
title: Quickstart
description: A quickstart guide to set up the environment and build the pzzld project from source.
---

Welcome to the quickstart guide for `pzzld`, a novel topological computing engine inspired by the neo-Riemannian theory of music. This guide will help you set up your environment and build the project from source.

## Table of Contents

- [Prerequisites](#prerequisites)
  - [Setup Rust](#setup-rust)
- [Getting Started](#getting-started)
  - [Build and Test](#build-and-test)

## Prerequisites

Before you begin, ensure you have the following prerequisites installed on your system:

- [Git](https://git-scm.com/) - For cloning the repository.
- [Rust](https://www.rust-lang.org/) (version 1.88 or later)
  - Includes `cargo`, Rust's package manager and build tool.

Optionally, you may also want to install the following tools:

- [cargo-binstall](https://github.com/cargo-bins/cargo-binstall) - A utility designed to streamline the installation of Rust binaries.
- [cargo-criterion](https://bheisler.github.io/criterion.rs/book/cargo_criterion/cargo_criterion.html) - A benchmarking tool for Rust projects.

### Setup Rust

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

#### *Adding additional targets*

Add the necessary `wasm32-*` targets for WebAssembly:

```bash
rustup target add wasm32-unknown-unknown wasm32-wasip1 wasm32-wasip2
```

#### *Optional: Installing cargo-binstall*

If you want to use `cargo-binstall` for easier installation of Rust binaries, you can install it with the following command:

```bash
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
```

or install it via `cargo`:

```bash
cargo install cargo-binstall
```

## Getting Started

Start by cloning the repository:

```bash
git clone git@github.com:FL03/pzzld --depth 1 --branch main
```

Then, navigate to the project directory:

```bash
cd pzzld
```

### Build and Test

Once you're in the project directory, you can build the project using `cargo`:

```bash
cargo build --workspace --release --all-features
```

Then, if you want, run the tests by:

```bash
cargo test --workspace --release --features full
```

or benchmark the project with:

```bash
cargo bench --workspace --release --verbose --features full --
```
