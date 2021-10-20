<p align="center">
  <img width="250" height="250" src="https://raw.githubusercontent.com/codetheweb/aoede/main/.github/logo.png">
</p>

Aoede is a Discord music bot that **directly** streams from **Spotify to Discord**. The only interface is Spotify itself.

**Note**: a Spotify Premium account is currently required. This is a limitation of librespot, the Spotify library Aoede uses.

![Demo](https://raw.githubusercontent.com/codetheweb/aoede/main/.github/demo.gif)

## 💼 Usecases

- Small servers with friends
- Discord Stages, broadcast music to your audience

## 🏗 Usage

(Images are available for x86 and arm64.)

**Docker Compose (recommended)**:

```yaml
version: '3.4'

services:
  aoede:
    image: codetheweb/aoede
    restart: always
    volumes:
      - ./aoede:/data
    environment:
      - DISCORD_TOKEN=
      - SPOTIFY_USERNAME=
      - SPOTIFY_PASSWORD=
      - DISCORD_USER_ID= # Discord user ID of the user you want Aoede to follow
```

**Prebuilt Binaries**:

Prebuilt binaries are available on the [releases page](https://github.com/codetheweb/aoede/releases). Download the binary for your platform, then inside a terminal session:

1. Configuration:
	- Rename the `config.sample.toml` file to `config.toml` and update the config keys <br>
	**or**
	- use env variables (see docker-compose section above)
		- On Windows, you can use `setx DISCORD_TOKEN my-token`
		- On Linux / macOS, you can use `export DISCORD_TOKEN=my-token`
2. Run the binary:
	- For Linux / macOS, `./platform-latest-aoede` after navigating to the correct directory
	- For Windows, execute `windows-latest-aoede.exe` after navigating to the correct directory

**Building from source**:

Requirements:

- automake
- autoconf
- cmake
- libtool
- Rust
- Cargo

Run `cargo build --release`. This will produce a binary in `target/release/aoede`. Set the required environment variables (see the Docker Compose section), then run the binary.
