How to use:

1. create a discord app: https://discord.com/developers/applications. take note of the client id of the app
2. create a bot for your app. take note of the token for the bot
3. invite your bot to a server by going to https://discord.com/oauth2/authorize?client_id=YOUR_CLIENT_ID_HERE&scope=bot
4. in your terminal do:

```bash
DISCORD_TOKEN=YOUR_TOKEN_HERE cargo run
```
