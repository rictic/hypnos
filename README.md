How to use:

1. create a discord app: https://discord.com/developers/applications. take note of the client id of the app
2. create a bot for your app. create a `secrets.env` file in this directory with content like:

```bash
DISCORD_TOKEN=paste bot token here
```

3. invite your bot to a server by going to https://discord.com/oauth2/authorize?client_id=YOUR_CLIENT_ID_HERE&scope=bot%20applications.commands
4. in your terminal do:

```bash
source secrets.env && cargo run
```

You can also do ./run_prod.sh, which will kill any previous prod hypnos processes and start hypnos as a daemon logging to `nohup.out`.
