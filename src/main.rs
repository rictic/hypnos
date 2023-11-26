mod dalle;
mod data;
mod dice;
mod sparkle;
use poise::serenity_prelude as serenity;

#[tokio::main]
async fn main() {
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![dice::roll(), dalle::gen(), sparkle::shimmer()],
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
