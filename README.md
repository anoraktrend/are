# ARE (Another Rust Editor)

ARE is a terminal-based text editor that is a port of the classic AEE (Another Easy Editor) to Rust. It is designed to be powerful yet simple enough for anyone to use immediately without instructions.

## Philosophy

- **Easy to use:** So intuitive it requires no instruction.
- **Easy to compile:** Simple setup and portable across Rust-supported platforms.
- **Minimal footprint:** A small number of files for compilation and installation.
- **Functional:** Feature-rich enough to be useful for daily tasks.

## Getting Started

### Compilation

You need a Rust toolchain installed. Build the project using:

```bash
cargo build --release
```

The resulting binary will be in `target/release/are`.

### Usage

Run the editor with an optional filename:

```bash
./target/release/are [filename]
```

Once inside the editor:
- **Esc** opens the main menu.
- **Ctrl-S** saves the current file.
- **Ctrl-Q** quits the editor.
- Use arrow keys to navigate and start typing to edit.
- A key binding bar at the top provides quick reminders of common commands.

## Features

- **Standard Key Bindings:** Familiar Ctrl-based shortcuts for common operations.
- **Menu System:** Access all functions through an easy-to-navigate menu (Esc).
- **Self-Contained:** Help system is embedded directly into the binary.
- **Syntax Highlighting:** Automatic highlighting for common programming languages.
- **Crash Recovery:** Journaling system helps recover work in case of a crash.

## Project Structure

- `src/`: The Rust source code for ARE.
- `aee/`: Reference implementation of the original C-based Another Easy Editor. This folder is provided for historical reference and is not required for building or running ARE.
- `LICENSE.MD`: Artistic License 2.0.
- `Cargo.toml`: Build configuration for ARE.

## License

This project is licensed under the Artistic License 2.0 - see the [LICENSE.MD](LICENSE.MD) file for details.
