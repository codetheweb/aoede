<p align="center">
  <img width="250" height="250" src="https://raw.githubusercontent.com/codetheweb/aoede/main/.github/logo.png">
</p>

Aoede is a Discord music bot that **directly** streams from **Spotify to Discord**. The only interface is Spotify itself.

> **⚠️ IMPORTANT: Authentication Update (2024)**  
> Spotify has deprecated username/password authentication. This fork includes support for **cached credentials** to fix authentication issues. See the [Authentication Setup](#authentication-setup) section below.

**Note**: a Spotify Premium account is currently required. This is a limitation of librespot, the Spotify library Aoede uses. Facebook logins [are not supported](https://github.com/librespot-org/librespot/discussions/635).

![Demo](https://raw.githubusercontent.com/codetheweb/aoede/main/.github/demo.gif)

## 💼 Usecases

- Small servers with friends
- Discord Stages, broadcast music to your audience

## 🏗 Usage

(Images are available for x86 and arm64.)

### Notes:
⚠️ Aoede only supports bot tokens. Providing a user token won't work.

Aoede will appear offline until you join a voice channel it has access it.

### Docker Compose (recommended):

There are a variety of image tags available:
- `:0`: versions >= 0.0.0
- `:0.5`: versions >= 0.5.0 and < 0.6.0
- `:0.5.1`: an exact version specifier
- `:latest`: whatever the latest version is

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
      - DISCORD_USER_ID=        # Discord user ID of the user you want Aoede to follow
      - SPOTIFY_BOT_AUTOPLAY=   # Autoplay similar songs when your music ends (true/false)
      - SPOTIFY_DEVICE_NAME=
```

### Docker:
```env
# .env
DISCORD_TOKEN=
SPOTIFY_USERNAME=
SPOTIFY_PASSWORD=
DISCORD_USER_ID=
SPOTIFY_BOT_AUTOPLAY=
SPOTIFY_DEVICE_NAME=
```

```bash
docker run --rm -d --env-file .env codetheweb/aoede
```

### Prebuilt Binaries:

Prebuilt binaries are available on the [releases page](https://github.com/codetheweb/aoede/releases). Download the binary for your platform, then inside a terminal session:

1. There are two options to make configuration values available to Aoede:
	1. Copy the `config.sample.toml` file to `config.toml` and update as necessary.
	2. Use environment variables (see the Docker Compose section above):
		- On Windows, you can use `setx DISCORD_TOKEN my-token`
		- On Linux / macOS, you can use `export DISCORD_TOKEN=my-token`
2. Run the binary:
	- For Linux / macOS, `./platform-latest-aoede` after navigating to the correct directory
	- For Windows, execute `windows-latest-aoede.exe` after navigating to the correct directory

### Building from source:

Requirements:

- automake
- autoconf
- cmake
- libtool
- Rust
- Cargo

Run `cargo build --release`. This will produce a binary in `target/release/aoede`. Set the required environment variables (see the Docker Compose section), then run the binary.

## 🔐 Authentication Setup

### Option 1: Cached Credentials (Recommended)

Due to Spotify deprecating username/password authentication in 2024, the recommended method is to use cached credentials:

1. **Download librespot-auth**:
   ```bash
   wget https://github.com/dspearson/librespot-auth/releases/download/v0.1.1/librespot-auth-x86_64-linux-musl-static.tar.xz
   tar -xf librespot-auth-x86_64-linux-musl-static.tar.xz
   ```

2. **Generate credentials**:
   ```bash
   ./librespot-auth-x86_64-linux-musl-static/librespot-auth --name "Aoede Bot"
   ```

3. **Select the device in Spotify**: Open Spotify on your phone/computer and select "Aoede Bot" from the device picker

4. **Set up cache directory**:
   ```bash
   mkdir -p aoede-cache
   cp credentials.json aoede-cache/
   ```

5. **Configure the bot**:
   ```bash
   cp config.sample.toml config.toml
   # Edit config.toml with your Discord token and user ID
   ```

6. **Run the bot**:
   ```bash
   cargo run
   ```

### Option 2: Username/Password (Legacy - May Not Work)

**Warning**: This method is deprecated by Spotify and may fail with "Bad credentials" error.

Create a config.toml file or use environment variables:
```bash
# With config.toml (recommended)
cp config.sample.toml config.toml
# Edit config.toml with your credentials
cargo run

# Or with environment variables
DISCORD_TOKEN=your_token SPOTIFY_USERNAME=your_username SPOTIFY_PASSWORD=your_password DISCORD_USER_ID=your_user_id cargo run
```

### Configuration Options

#### config.toml (Recommended)

```toml
# Required
discord_token = "your_discord_bot_token"
discord_user_id = 123456789

# Recommended for cached credentials
cache_dir = "aoede-cache"

# Optional (legacy auth - deprecated)
spotify_username = ""
spotify_password = ""

# Optional settings
spotify_bot_autoplay = false
spotify_device_name = "Aoede"
```

#### Environment Variables (Alternative)

| Variable | Required | Description |
|----------|----------|-------------|
| `DISCORD_TOKEN` | Yes | Your Discord bot token |
| `DISCORD_USER_ID` | Yes | Discord user ID to follow |
| `CACHE_DIR` | Recommended | Directory containing cached Spotify credentials |
| `SPOTIFY_USERNAME` | Optional* | Spotify username (legacy auth) |
| `SPOTIFY_PASSWORD` | Optional* | Spotify password (legacy auth) |
| `SPOTIFY_BOT_AUTOPLAY` | No | Enable autoplay (true/false) |
| `SPOTIFY_DEVICE_NAME` | No | Custom device name (default: "Aoede") |

*Required only if not using cached credentials. Environment variables override config.toml values.

### Migration from Username/Password

If you were using username/password authentication:

1. Follow the [cached credentials setup](#option-1-cached-credentials-recommended) above
2. Remove `SPOTIFY_USERNAME` and `SPOTIFY_PASSWORD` environment variables
3. Add `CACHE_DIR` environment variable pointing to your credentials directory

### Troubleshooting

- **"Bad credentials" error**: Use cached credentials instead of username/password
- **"No cached credentials found"**: Ensure `credentials.json` is in your cache directory
- **Device not showing in Spotify**: Make sure librespot-auth and Spotify are on the same network
- **Credentials expire**: Re-run the credential generation process
