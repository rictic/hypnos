use futures::future::FutureExt;
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    fmt::Display,
    sync::Arc,
};

use chrono::{DateTime, Duration, Local};
use chrono_english::{parse_date_string, Dialect};
use serenity::{
    async_trait,
    builder::ParseValue,
    futures::future::select_all,
    model::{
        channel::{Message, ReactionType},
        gateway::Ready,
        id::{ChannelId, GuildId, MessageId},
        prelude::{Activity, User},
    },
    prelude::*,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

struct GetTogether {
    message: MessageId,
    channel: ChannelId,
    title: String,
    time: Option<DateTime<Local>>,
    description: String,
    notified: bool,
}

impl GetTogether {
    fn is_initialized(&self) -> bool {
        !(self.time.is_none() || (self.title.is_empty() && self.description.is_empty()))
    }
}

impl Display for GetTogether {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.title.is_empty() {
            f.write_fmt(format_args!("**{}**\n\n", self.title))?;
        }
        if !self.description.is_empty() {
            f.write_fmt(format_args!("{}\n\n", self.description))?;
        }
        match self.time {
            Some(time) => {
                f.write_fmt(format_args!("On {}\n\n", time.format("%a %b %e %-I:%M%P")))?;
            }
            None => {
                f.write_str("**time not yet specified**\n\n")?;
            }
        }
        if self.is_initialized() {
            f.write_str("Give a :white_check_mark: if you're in, a :grey_question: if it's more maybe. I'll be sure to give you a ping either way when it's time.")?;
        } else {
            f.write_str("Reply to this message like `title: Overwatch` or `time: 8pm friday` or `description: ...` to set up this event!")?;
        }
        Ok(())
    }
}

struct Handler {
    get_togethers: Arc<RwLock<BTreeMap<MessageId, GetTogether>>>,
    notify_time_change: Arc<Mutex<UnboundedSender<()>>>,
    time_changed: Arc<Mutex<UnboundedReceiver<()>>>,
}

enum Reply {
    Message(String),
    Reaction(ReactionType),
}

impl Handler {
    fn new() -> Self {
        let (sender, receiver) = unbounded_channel();
        Self {
            get_togethers: Default::default(),
            notify_time_change: Arc::new(Mutex::new(sender)),
            time_changed: Arc::new(Mutex::new(receiver)),
        }
    }

    async fn handle_reply_command(
        &self,
        msg_content: &str,
        replying_to_id: MessageId,
    ) -> Option<Result<(String, Reply, bool), String>> {
        let mut msgs = self.get_togethers.write().await;
        let get_together = match msgs.get_mut(&replying_to_id) {
            Some(get_together) => get_together,
            None => return None,
        };
        if let Some(new_title) = msg_content.strip_prefix("title: ") {
            get_together.title = new_title.trim().to_string();
            return Some(Ok((
                format!("{}", get_together),
                Reply::Reaction(ReactionType::Unicode("👍".to_string())),
                get_together.is_initialized(),
            )));
        } else if let Some(new_description) = msg_content.strip_prefix("description: ") {
            get_together.description = new_description.trim().to_string();
            return Some(Ok((
                format!("{}", get_together),
                Reply::Reaction(ReactionType::Unicode("👍".to_string())),
                get_together.is_initialized(),
            )));
        } else if let Some(str_time) = msg_content.strip_prefix("time:") {
            let now = Local::now();
            let time = match parse_date_string(str_time, now, Dialect::Us) {
                Ok(date) => date,
                Err(e) => {
                    return Some(Err(format!("couldn't parse time: {}", e)));
                }
            };
            get_together.time = Some(time.into());
            self.notify_time_change.lock().await.send(()).unwrap();
            return Some(Ok((
                format!("{}", get_together),
                Reply::Message(format!(
                    "Time updated! Event happening in {}",
                    pretty_print_duration(time - now)
                )),
                get_together.is_initialized(),
            )));
        } else {
            return Some(Err(format!("I'm confused you know. Try starting your message with `title:` or `description:` or `time:`")));
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        println!("{} is connected!", ready.user.name);
        ctx.set_activity(Activity::listening("!event")).await;
    }

    // Set a handler for the `message` event - so that whenever a new message
    // is received - the closure (or function) passed will be called.
    //
    // Event handlers are dispatched through a threadpool, and so multiple
    // events can be dispatched simultaneously.
    async fn message(&self, ctx: Context, mut msg: Message) {
        if msg.author.name == "hypnos" {
            return; // always ignore self messages
        }
        if msg.content == "!event" {
            match msg.channel_id.say(&ctx.http, "New Event! Reply to this message with `time: 7:30pm` to set the time. Likewise for the title and description.").await {
                Ok(pong) => {
                    let mut msgs = self.get_togethers.write().await;
                    msgs.insert(pong.id, GetTogether {
                        message: pong.id,
                        channel: msg.channel_id,
                        title: "".to_string(),
                        description: "".to_string(),
                        time: None,
                        notified: false
                    });
                }
                Err(why) => {
                    println!("Error sending message: {:?}", why);
                }
            }
        } else if let Some(replying_to) = msg.message_reference.clone() {
            let replying_to_id = match replying_to.message_id {
                Some(m) => m,
                None => return,
            };
            // The typical message doesn't have to do with us.
            // Early exit in that case.
            {
                let msgs = self.get_togethers.read().await;
                if !msgs.contains_key(&replying_to_id) {
                    return;
                }
            }
            let our_reply = match self
                .handle_reply_command(&msg.content, replying_to_id)
                .await
            {
                None => return,
                Some(Ok((edit, reply, initialized))) => {
                    {
                        let our_message = msg.referenced_message.as_mut().unwrap();
                        our_message
                            .edit(ctx.http.clone(), |ed| ed.content(edit))
                            .await
                            .unwrap_or_else(|why| {
                                println!("Error editing original message: {:?}", why)
                            });
                        if initialized {
                            if let Err(why) = our_message
                                .react(ctx.http.clone(), ReactionType::Unicode("✅".to_string()))
                                .await
                            {
                                println!(
                                    "Error reacting to our message with checkmark message: {:?}",
                                    why
                                );
                            }
                            if let Err(why) = our_message
                                .react(ctx.http.clone(), ReactionType::Unicode("❔".to_string()))
                                .await
                            {
                                println!(
                                    "Error reacting to our message with checkmark message: {:?}",
                                    why
                                );
                            }
                        }
                    }
                    // update the original post here
                    reply
                }
                Some(Err(err)) => Reply::Message(err),
            };
            match our_reply {
                Reply::Message(our_reply) => {
                    if let Err(why) = msg.reply(ctx.http, our_reply).await {
                        println!("Error sending message: {:?}", why);
                    }
                }
                Reply::Reaction(reaction) => {
                    if let Err(why) = msg.react(ctx.http, reaction).await {
                        println!("Error sending message: {:?}", why);
                    }
                }
            };
        }
    }

    async fn cache_ready(&self, ctx: Context, _guilds: Vec<GuildId>) {
        let get_togethers = self.get_togethers.clone();
        let time_changed = self.time_changed.clone();
        tokio::spawn(async move {
            let mut time_changed = time_changed.lock().await;
            loop {
                let sleep_time = {
                    let mut get_togethers = get_togethers.write().await;
                    let now = Local::now();
                    let mut sleep_time = Duration::hours(1);
                    let mut to_remove = BTreeSet::new();
                    for g in get_togethers.values_mut() {
                        if g.notified {
                            to_remove.insert(g.message);
                            continue;
                        }
                        let time = match g.time {
                            None => continue,
                            Some(time) => time,
                        };
                        if time < now {
                            let yes_reactions = g
                                .channel
                                .reaction_users(
                                    ctx.http.clone(),
                                    g.message,
                                    ReactionType::Unicode("✅".to_string()),
                                    Some(100),
                                    None,
                                )
                                .await;
                            let maybe_reactions = g
                                .channel
                                .reaction_users(
                                    ctx.http.clone(),
                                    g.message,
                                    ReactionType::Unicode("❔".to_string()),
                                    Some(100),
                                    None,
                                )
                                .await;

                            let to_notify: HashSet<User> = match (yes_reactions, maybe_reactions) {
                                (Ok(yes), Ok(maybe)) => {
                                    println!("{:#?}", (&yes, &maybe));
                                    yes.into_iter()
                                        .chain(maybe.into_iter())
                                        .filter(|r| r.name != "hypnos")
                                        .collect()
                                }
                                _ => {
                                    println!("Failed to get reactions!!");
                                    g.notified = true; // assume it's a permanent error :/
                                    continue;
                                }
                            };
                            if let Err(why) = g
                                .channel
                                .send_message(ctx.http.clone(), |m| {
                                    println!("{:#?}", to_notify);
                                    m.allowed_mentions(|am| am.parse(ParseValue::Users));
                                    m.content(format!(
                                        "It's time! {}",
                                        to_notify
                                            .into_iter()
                                            .map(|r| format!("<@!{}>", r.id.as_u64()))
                                            .collect::<String>(),
                                    ));
                                    m.reference_message((g.channel, g.message));
                                    m
                                })
                                .await
                            {
                                println!("Failed to send notification message! {}", why);
                                g.notified = true; // assume it's a permanent error :/
                                continue;
                            }

                            g.notified = true; // assume it's a permanent error :/
                                               // g.event_message
                                               // the event passed, notify people!!!
                            continue;
                        }
                        let dur = time - now;
                        if dur < sleep_time {
                            sleep_time = dur;
                        }
                    }
                    for mid in to_remove.into_iter() {
                        get_togethers.remove(&mid);
                    }

                    println!("Sleeping for {}", sleep_time);
                    sleep_time.to_std().unwrap() // safe because it's limited size
                };

                select_all(vec![
                    time_changed.recv().map(|_v| ()).boxed(),
                    Box::pin(tokio::time::sleep(sleep_time.into())),
                ])
                .await;
            }
        });
    }
}

// inefficient but who cares
// efficient would be to implement Display for a wrapper type I think
fn pretty_print_duration(mut dur: Duration) -> String {
    let mut s = String::new();
    let is_negative = dur.num_seconds() < 0;
    if is_negative {
        dur = -dur;
    }
    if dur.num_days() > 0 {
        s += &format!("{} days ", dur.num_days());
    }
    let dur = dur - Duration::days(dur.num_days());
    if dur.num_hours() > 0 {
        s += &format!("{} hours ", dur.num_hours());
    }
    let dur = dur - Duration::hours(dur.num_hours());
    s += &format!("{} minutes ", dur.num_minutes());
    if is_negative {
        s += &format!("in the past!?!? Uh.. try specifying the day? Like `time: 2am tomorrow`?");
    }
    s.trim().to_string()
}

#[tokio::main]
async fn main() {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token)
        .event_handler(Handler::new())
        .await
        .expect("Err creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
}
