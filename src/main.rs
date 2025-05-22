mod dalle;
mod data;
mod dice;
mod info;
mod sparkle;
use std::time::Duration;
use poise::serenity_prelude as serenity;
use poise::Event;

#[tokio::main]
async fn main() {
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![dice::roll(), dalle::gen(), sparkle::shimmer(), info::info()],
            event_handler: |_ctx, event, framework, data| {
                Box::pin(event_handler(_ctx, event, framework, data))
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

async fn event_handler(
    ctx: &serenity::Context,
    event: &Event<'_>,
    _framework: poise::FrameworkContext<'_, data::Data, data::Error>,
    data: &data::Data,
) -> Result<(), data::Error> {
    if let Event::Message { new_message } = event {
        if new_message.author.bot {
            return Ok(());
        }
        if data.low_traffic_channels.contains(&new_message.channel_id) {
            use std::time::Instant;
            let mut state = data.low_traffic_state.lock().await;
            let now = Instant::now();
            let entries = state.messages.entry(new_message.channel_id).or_default();
            entries.push(now);
            let limit = Duration::from_secs(5 * 60);
            entries.retain(|t| now.duration_since(*t) <= limit);
            if entries.len() > 3 {
                let warn = match state.last_warned.get(&new_message.channel_id) {
                    Some(last) if now.duration_since(*last) < limit => false,
                    _ => true,
                };
                if warn {
                    if let Err(err) = new_message
                        .channel_id
                        .say(
                            &ctx.http,
                            "This channel is meant to be low traffic. Please continue the conversation elsewhere.",
                        )
                        .await
                    {
                        println!("Failed to send low traffic warning: {}", err);
                    }
                    state.last_warned.insert(new_message.channel_id, now);
                }
            }
        }
    }
    Ok(())
}
