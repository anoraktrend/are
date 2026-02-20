# ARE

ARE (Another Rust Editor) is a port of AEE (Another Easy Editor) to Rust. It aims to provide a high-performance, memory-safe implementation of the original editor while leveraging Rust's modern ecosystem.

## Features

- **Performance:** Fast text processing and rendering using Rust's zero-cost abstractions.
- **Safety:** Memory safety guaranteed by Rust's borrow checker.
- **Portability:** Compiles to multiple targets, including Linux, macOS, and Windows.
- **Modern Tooling:** Full integration with `cargo` for building and testing.

## Getting Started

### Prerequisites

Ensure you have the latest stable version of Rust installed:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

### Installation

Clone the repository and build the project:

```bash
git clone https://github.com/lucy/aee.git
cd aee
cargo build --release
```

### Usage

To start the editor:

```bash
cargo run -- path/to/file.txt
```

## Development

### Running Tests

```bash
cargo test
```

### Formatting

```bash
cargo fmt
```

## License

This project is licensed under Larry Walls' Artistic License - see the [LICENSE](LICENSE) file for details.
