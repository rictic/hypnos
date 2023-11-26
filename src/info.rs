use crate::data::{self, Context, Error};

#[poise::command(slash_command)]
pub async fn info(ctx: Context<'_>) -> Result<(), Error> {
    let account = data::get_account(ctx.data(), ctx.author()).await?;

    // need to format these numbers from millicents to just dollars and cents!
    // dividing by a million isn't right lol
    ctx.send(|m| {
    let m = if account.overdrafted() {
      m.content(format!("You should take rictic out to lunch! Or just ping him and venmo him like 20 bucks. He'll update your limits. Your credits stand at ${}, you've used ${} worth of credits all time, and generated {} images.", (account.credit as f64)  / 100_000.0, (account.total_cost as f64) / 10_000.0, account.images))
    } else {
      m.content(format!(
        "You've got ${} worth of rictic image generation credits left until you should take him out to lunch sometime. You've used ${} worth of credits all time, and generated {} images.",
        (account.credit as f64) / 100_000.0,
        (account.total_cost as f64) /  100_000.0, account.images
      ))
    };
    m.ephemeral(true)
  }).await?;
    Ok(())
}
