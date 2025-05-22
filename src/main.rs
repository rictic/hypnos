mod dalle;
mod data;
mod dice;
mod info;
mod sparkle;
use poise::serenity_prelude as serenity;
use data::Error;

async fn handle_event(
    ctx: &serenity::Context,
    event: &poise::Event<'_>,
    _framework: poise::FrameworkContext<'_, data::Data, Error>,
    data: &data::Data,
) -> Result<(), Error> {
    if let poise::Event::Message { new_message } = event {
        if new_message.author.bot {
            return Ok(());
        }
        if data.low_traffic_channels.contains(&new_message.channel_id) {
            let mut tracker = data.traffic.lock().await;
            if tracker.record(new_message.channel_id.0) {
                new_message
                    .channel_id
                    .say(
                        &ctx.http,
                        "This channel is intended to be low traffic. Please move this conversation to another channel.",
                    )
                    .await?;
            }
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![dice::roll(), dalle::gen(), sparkle::shimmer(), info::info()],
            event_handler: |ctx, event, framework, data| {
                Box::pin(handle_event(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .token(std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN env variable"))
        .intents(serenity::GatewayIntents::non_privileged())
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                println!("Registering commands...");
                let result =
                    poise::builtins::register_globally(ctx, &framework.options().commands).await;
                if let Err(err) = result {
                    println!("Failed to register commands: {}", err);
                } else {
                    println!(
                        "Registered {} commands successfully",
                        framework.options().commands.len()
                    );
                    for command in framework.options().commands.iter() {
                        println!(" - {}", command.name);
                    }
                }
                Ok(data::Data::read_or_create().await?)
            })
        });
    println!("Starting bot...");
    framework.run().await.unwrap();
}
