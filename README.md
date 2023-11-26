### How to use:

1. create a discord app: https://discord.com/developers/applications. take note of the client id (aka application id) of the app
2. create a bot for your app. create a `secrets.env` file in this directory with content like:

```bash
export DISCORD_TOKEN=paste bot token here
```

3. invite your bot to a server by going to https://discord.com/oauth2/authorize?client_id=YOUR_CLIENT_ID_HERE&scope=bot%20applications.commands
4. in your terminal do:

```bash
source secrets.env && cargo run
```

### Image generation

To support DALL-E 3 image generation, also add your OpenAI API key to secrets.env. Note, of course, that your bot's users can run up your OpenAI bill!

```bash
export OPENAI_API_KEY=paste API key here
```

### Prod

To run the prod build, run ./run_prod.sh, which will kill any previous prod hypnos processes and start hypnos as a daemon logging to `nohup.out`, then tail that file in your current terminal. Quitting the tail will not stop hypnos.
