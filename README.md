# Crafting Table

Crafting Table is a Rust terminal app for media conversion, image conversion,
and downloads.

## Build and run

```sh
cargo run
```

Or build an executable with `cargo build --release`.

## Implementations and requirements

Image conversion and direct HTTP(S) downloads are implemented in Rust. Direct
downloads use HTTP byte ranges and up to 16 concurrent connections when the
server supports them. A configured speed limit uses a single connection.

Video/audio conversion and scaling still use FFmpeg. Broad video-site
extraction still uses yt-dlp, and general gallery-site extraction still uses
gallery-dl. Those projects contain codec support and site-specific extraction
logic that is not recreated by this initial Rust port. Install those programs
for the corresponding menu features; the other implemented Rust features do
not require aria2c, ImageMagick, or Python.

Native image conversion supports the formats enabled in `Cargo.toml` (AVIF,
GIF, JPEG, PNG, and WebP). Animated images are decoded as a still image.

The Settings menu lets you manage source-specific API keys, choose where their
store is kept, and set the default browser for downloader cookies. The browser
is used by yt-dlp and gallery-dl, and can be overridden for individual
yt-dlp downloads. Changing the key-store location moves the existing store
when the destination does not already contain a file. Saved settings are
scoped to a host and passed to matching gallery downloads through a temporary
config file; the saved values are stored locally with owner-only permissions
on Unix. The application settings file is stored in the user's configuration
directory.

Settings also includes a searchable, category-based catalog sourced from the
[public-apis repository](https://github.com/public-apis/public-apis). Selecting
an entry opens its listed documentation in the system browser. An internet
connection is required to fetch the latest catalog. The catalog provides API
listings and authentication requirements, not API keys; obtain credentials
from each API provider.
