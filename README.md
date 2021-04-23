<p align="center">
  <img width="250" height="250" src="https://raw.githubusercontent.com/codetheweb/aoede/main/.github/logo.png">
</p>

Aoede is a Discord music bot that **directly** streams from **Spotify to Discord**. The only interface is Spotify itself.

![Demo](https://raw.githubusercontent.com/codetheweb/aoede/main/.github/demo.gif)

## 💼 Usecases

- Small servers with friends
- Discord Stages, broadcast music to your audience

## 🏗 Usage

(Images are available for x86 and arm64.)

**Docker Compose**:

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
