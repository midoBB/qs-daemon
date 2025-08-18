<!--
SPDX-License-Identifier: MPL-2.0
-->

# qs-daemon

A fast file search system with fuzzy matching capabilities, consisting
of a Rust daemon backend and a QML-based GUI frontend using Quickshell.

## Overview

The system provides two main components:

- **qs-daemon**: A Rust-based daemon that indexes files and provides fast fuzzy search
- **file-picker**: A QML GUI that connects to the daemon for interactive file searching

## Architecture

### Backend (Rust Daemon)

- **Fast Fuzzy Search**: Uses `nucleo-matcher` (same engine as Neovim's telescope)
- **Unix Socket Communication**: Dual-socket architecture for request/response handling
- **Background Indexing**: Automatically refreshes file index every 5 minutes
- **Concurrent Client Handling**: Multiple clients supported simultaneously
- **Home Directory Scanning**: Recursively indexes all files using the `fd` command

### Frontend (QML GUI)

- **Real-time Search**: Live fuzzy search with 100ms debounce
- **Keyboard Navigation**: Arrow keys for selection, Enter to open, Escape to close
- **Match Highlighting**: Visual highlighting of matched characters in filenames
- **Status Display**: Shows connection status and result counts
- **Dark Theme**: Modern dark UI with clean typography

## Installation

### Prerequisites

- Rust toolchain (for building the daemon)
- `fd` command-line tool (for file discovery)
- Quickshell (for running the QML GUI)

### Build

``` bash
cargo build --release
```

## Usage

### Start the Daemon

``` bash
cargo run
```

The daemon will:

- Create Unix sockets at `/tmp/quickfile-daemon.sock` (requests)
  and `/tmp/quickfile-response.sock` (responses)
- Index all files in your home directory
- Run with structured logging output
- Automatically refresh the index every 5 minutes

### Launch the GUI

``` bash
quickshell file-picker/shell.qml
```

The GUI provides:

- **Search**: Type to fuzzy search through all indexed files
- **Navigation**: Use arrow keys to navigate results
- **Open Files**: Press Enter or click to open selected file with `xdg-open`
- **Quick Exit**: Press Escape to close the picker

### Command-Line Client

Use the client script for programmatic access:

``` bash
./quickfile-client.sh search "filename"
./quickfile-client.sh status
./quickfile-client.sh refresh
```

## Communication Protocol

The daemon accepts JSON requests over Unix sockets:

### Search Request

``` json
{
  "type": "Search",
  "query": "search_term",
  "limit": 100
}
```

### Response Format

``` json
{
  "type": "SearchResults",
  "results": [
    {
      "path": "/absolute/path/to/file",
      "display_path": "~/relative/path/to/file",
      "matches": [{"char_index": 5}],
      "score": 85
    }
  ],
  "results_count": 1,
  "total_files": 15420
}
```

## Development

### Build Commands

``` bash
cargo build --release    # Build optimized binary
cargo run                # Run daemon in development
cargo check              # Fast syntax checking
cargo clippy             # Linting
cargo fmt                # Code formatting
```

### Key Dependencies

- **tokio**: Async runtime for socket handling and periodic tasks
- **nucleo-matcher**: High-performance fuzzy matching engine
- **serde**: JSON serialization for client-daemon communication
- **tracing**: Structured logging throughout the application
- **anyhow**: Error handling with context

## Features

- **Dual Socket Architecture**: Separate sockets for requests and responses
  enable better GUI integration
- **Smart Match Highlighting**: Character-level highlighting shows exactly
  which parts of filenames matched
- **Tilde Path Display**: Clean `~/` notation for better readability
- **Automatic Reconnection**: GUI automatically handles daemon reconnections
- **Performance Optimized**: Nucleo matcher provides sub-millisecond search times
- **Background Updates**: File index stays current without user intervention

## File Structure

├── src/main.rs              \# Rust daemon implementation
├── file-picker/shell.qml    \# QML GUI interface
├── quickfile-client.sh      \# Command-line client script
└── Cargo.toml               \# Rust dependencies and config
