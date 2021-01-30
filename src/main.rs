use chrono::{DateTime, Duration, Local};
use chrono_english::{parse_date_string, Dialect};
use futures::future::FutureExt;
use serde::{Deserialize, Serialize};
use serenity::{
    async_trait,
    builder::{CreateEmbed, CreateMessage, EditMessage, ParseValue},
    futures::future::select_all,
    model::{
        channel::{Message, ReactionType},
        gateway::Ready,
        id::{ChannelId, GuildId, MessageId},
        prelude::{Activity, User},
    },
    prelude::*,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    error::Error,
    sync::Arc,
};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

#[derive(Serialize, Deserialize, Clone)]
struct GetTogether {
    message: MessageId,
    channel: ChannelId,
    title: String,
    time: Option<DateTime<Local>>,
    description: String,
    notified: bool,
}

impl GetTogether {
    fn blank() -> Self {
        Self {
            message: MessageId::from(0),
            channel: ChannelId::from(0),
            title: "".to_string(),
            time: None,
            description: "".to_string(),
            notified: false,
        }
    }

    fn is_initialized(&self) -> bool {
        !(self.time.is_none() || (self.title.is_empty() && self.description.is_empty()))
    }

    fn create_message(&self, m: &mut CreateMessage) {
        m.embed(|e| {
            self.embed(e);
            e
        });
    }

    fn edit_message(&self, m: &mut EditMessage) {
        m.embed(|e| {
            self.embed(e);
            e
        });
    }

    fn embed(&self, e: &mut CreateEmbed) {
        e.title(if self.title.is_empty() {
            "title goes here lol"
        } else {
            &self.title
        });
        e.description(&self.description);
        e.footer(|f| {
            f.text(if self.is_initialized() {
                "Give a ✅ if you're in, a ❔ if it's more maybe. I'll be sure to give you a ping either way when it's time"
            } else {
                "Reply to this message like `title: Overwatch` or `time: 8pm friday` or `description: ...` to set up this event!"
            })
        });
        let time = match self.time {
            Some(time) => {
                format!("{}", time.format("%a %b %e %-I:%M%P"))
            }
            None => "Who knows man".to_string(),
        };
        e.field("When", time, false);
    }
}

#[derive(Serialize, Deserialize)]
struct SerializableHandlerData {
    get_togethers: BTreeMap<String, GetTogether>,
}
impl SerializableHandlerData {
    fn from_get_togethers(get_togethers: &BTreeMap<MessageId, GetTogether>) -> Self {
        Self {
            get_togethers: get_togethers
                .iter()
                .map(|(key, value)| (key.as_u64().to_string(), value.clone()))
                .collect(),
        }
    }

    fn into_handler(self) -> Handler {
        let get_togethers = self
            .get_togethers
            .into_iter()
            .map(|(key, value)| (MessageId::from(key.parse::<u64>().unwrap()), value))
            .collect();
        let (sender, receiver) = unbounded_channel();
        Handler {
            get_togethers: Arc::new(RwLock::new(get_togethers)),
            notify_time_change: Arc::new(Mutex::new(sender)),
            time_changed: Arc::new(Mutex::new(receiver)),
        }
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
    ) -> Option<Result<(GetTogether, Reply, bool), String>> {
        let mut msgs = self.get_togethers.write().await;
        let get_together = match msgs.get_mut(&replying_to_id) {
            Some(get_together) => get_together,
            None => return None,
        };
        if let Some(new_title) = msg_content.strip_prefix("title: ") {
            get_together.title = new_title.trim().to_string();
            return Some(Ok((
                get_together.clone(),
                Reply::Reaction(ReactionType::Unicode("👍".to_string())),
                get_together.is_initialized(),
            )));
        } else if let Some(new_description) = msg_content.strip_prefix("description: ") {
            get_together.description = new_description.trim().to_string();
            return Some(Ok((
                get_together.clone(),
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
                get_together.clone(),
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

    // TODO: move serialization into a green thread that's notified when
    //     state is mutated.
    async fn serialize_state(state: SerializableHandlerData) -> Result<(), Box<dyn Error>> {
        {
            let file = std::fs::File::create("data_writing.json")?;
            serde_json::to_writer_pretty(file, &state)?;
        }
        tokio::fs::rename("data_writing.json", "data.json").await?;
        Ok(())
    }

    async fn deserialize() -> Result<Option<Handler>, Box<dyn Error>> {
        let file = match std::fs::File::open("data.json") {
            Ok(file) => file,
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => {
                    println!("data.json doesn't exist, starting fresh");
                    return Ok(None);
                }
                _ => return Err(e.into()),
            },
        };
        let read: SerializableHandlerData = serde_json::from_reader(file)?;
        Ok(Some(read.into_handler()))
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
            match msg
                .channel_id
                .send_message(&ctx.http, |m| {
                    GetTogether::blank().create_message(m);
                    m
                })
                .await
            {
                Ok(pong) => {
                    let serializable = {
                        let mut msgs = self.get_togethers.write().await;
                        msgs.insert(
                            pong.id,
                            GetTogether {
                                message: pong.id,
                                channel: msg.channel_id,
                                title: "".to_string(),
                                description: "".to_string(),
                                time: None,
                                notified: false,
                            },
                        );
                        SerializableHandlerData::from_get_togethers(&msgs)
                    };

                    if let Err(why) = Self::serialize_state(serializable).await {
                        println!("failed to serialize! {}", why);
                    }
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
                            .edit(ctx.http.clone(), |ed| {
                                edit.edit_message(ed);
                                ed
                            })
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
            let get_togethers = self.get_togethers.read().await;
            let state = SerializableHandlerData::from_get_togethers(&get_togethers);
            if let Err(why) = Self::serialize_state(state).await {
                println!("failed to serialize! {}", why);
            }
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
                                (Ok(yes), Ok(maybe)) => yes
                                    .into_iter()
                                    .chain(maybe.into_iter())
                                    .filter(|r| r.name != "hypnos")
                                    .collect(),
                                _ => {
                                    println!("Failed to get reactions!!");
                                    g.notified = true; // assume it's a permanent error :/
                                    to_remove.insert(g.message);
                                    continue;
                                }
                            };
                            if let Err(why) = g
                                .channel
                                .send_message(ctx.http.clone(), |m| {
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
                                to_remove.insert(g.message);
                                continue;
                            }

                            g.notified = true;
                            to_remove.insert(g.message);
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

                    let state = SerializableHandlerData::from_get_togethers(&get_togethers);
                    if let Err(why) = Self::serialize_state(state).await {
                        println!("failed to serialize! {}", why);
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
async fn main() -> Result<(), Box<dyn Error>> {
    // Configure the client with your Discord bot token in the environment.
    let token = env::var("DISCORD_TOKEN").expect("Expected a token in the environment");

    let handler = Handler::deserialize()
        .await?
        .unwrap_or_else(|| Handler::new());

    // Create a new instance of the Client, logging in as a bot. This will
    // automatically prepend your bot token with "Bot ", which is a requirement
    // by Discord for bot users.
    let mut client = Client::builder(&token)
        .event_handler(handler)
        .await
        .expect("Err creating client");

    // Finally, start a single shard, and start listening to events.
    //
    // Shards will automatically attempt to reconnect, and will perform
    // exponential backoff until it reconnects.
    if let Err(why) = client.start().await {
        println!("Client error: {:?}", why);
    }
    Ok(())
}
