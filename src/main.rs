use chrono::{DateTime, Duration, Local};
use chrono_english::{parse_date_string, Dialect};
use futures::future::FutureExt;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::{
    async_trait,
    builder::{CreateEmbed, CreateMessage, EditMessage, ParseValue},
    futures::future::select_all,
    model::{
        channel::{Message, ReactionType},
        gateway::Ready,
        id::{ChannelId, GuildId, MessageId},
        interactions::{
            Interaction, InteractionApplicationCommandCallbackDataFlags, InteractionResponseType,
        },
        prelude::{Activity, User},
    },
    prelude::*,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    error::Error,
    fmt::Write,
    sync::Arc,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};

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
        Handler::from_state(get_togethers)
    }
}

fn notify(sender: &Sender<()>) {
    match sender.try_send(()) {
        Ok(()) => {}
        Err(_) => {
            // don't care, that just means it's already been notified
        }
    }
}

struct Handler {
    get_togethers: Arc<RwLock<BTreeMap<MessageId, GetTogether>>>,
    notify_time_change: Sender<()>,
    notify_serialize: Arc<Sender<()>>,
    time_changed: Arc<Mutex<Receiver<()>>>,
    should_serialize: Arc<Mutex<Receiver<()>>>,
}

enum Reply {
    Message(String),
    Reaction(ReactionType),
}

impl Handler {
    fn new() -> Self {
        Self::from_state(Default::default())
    }

    fn from_state(get_togethers: BTreeMap<MessageId, GetTogether>) -> Self {
        let (notify_time_change, time_changed) = channel(1);
        let (notify_serialize, should_serialize) = channel(1);
        Self {
            get_togethers: Arc::new(RwLock::new(get_togethers)),
            notify_time_change,
            notify_serialize: Arc::new(notify_serialize),
            time_changed: Arc::new(Mutex::new(time_changed)),
            should_serialize: Arc::new(Mutex::new(should_serialize)),
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
            notify(&self.notify_time_change);
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

fn handle_interaction(interaction: &Interaction) -> Option<Result<String, String>> {
    let data = interaction.data.as_ref()?;
    let data = match data {
        serenity::model::interactions::InteractionData::ApplicationCommand(cmd) => cmd,
        serenity::model::interactions::InteractionData::MessageComponent(_) => return None,
    };
    println!("Got an interaction: {:?}", data.id);
    if data.name != "roll" {
        return Some(Err("Internal error: unknown command".to_string()));
    }
    let dice = data
        .options
        .iter()
        .find(|opt| opt.name == "dice")?
        .value
        .as_ref()?
        .as_str()?;
    let roll = DiceRollRequest::parse(dice);
    let roll = match roll {
        Err(err) => {
            return Some(Err(err));
        }
        Ok(roll) => roll,
    };
    let mut roll = roll.roll();
    let resp = format!(
        "Rolling {}\n\nResult: {}",
        dice,
        roll.to_discord_markdown().trim()
    );
    if resp.len() > 1950 {
        Some(Ok(format!(
            "Roll {}?? hoo.. that's a lot. I don't wanna flood the chat here, so, uh, I'll give you the quick summary:\n\n{}",
            dice,
            roll.short_summary()
        )))
    } else {
        Some(Ok(resp))
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        ctx.set_activity(Activity::listening("!event")).await;
        let commands = ctx.http.get_global_application_commands().await;
        let commands = match commands {
            Ok(commands) => commands,
            Err(e) => panic!("Failed to get app commands: {}", e),
        };
        let roll_command = commands.iter().find(|cmd| cmd.name == "roll");
        match roll_command {
            Some(roll_command) => {
                println!("Found roll command, it has id: {}", roll_command.id);
            }
            None => {
                let create_command = serde_json::json!({
                    "name": "roll",
                    "description": "Rolls some dice and shows the results using the Cortex Prime system",
                    "options": [
                        {
                            "name": "dice",
                            "description": "The dice you want to roll, like: `d4` or `3d6 1d10` or even just `6 8 10`",
                            "type": 3,
                            "required": true,
                        },
                    ]
                });
                match ctx
                    .http
                    .create_global_application_command(&create_command)
                    .await
                {
                    Ok(cmd) => println!("roll command created, it has id: {}", cmd.id),
                    Err(e) => panic!("Error creating roll command: {}", e),
                }
            }
        }

        println!("{} is connected!", ready.user.name);
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        println!("Got an interaction");
        let result = handle_interaction(&interaction);
        let res = interaction
            .create_interaction_response(ctx.http, |r| {
                r.kind(InteractionResponseType::ChannelMessageWithSource);
                r.interaction_response_data(|r| {
                    match result {
                        Some(Ok(reply)) => r.content(reply),
                        Some(Err(err)) => {
                            r.content(err);
                            // send as a private reply, as it was user error most likely
                            r.flags(InteractionApplicationCommandCallbackDataFlags::EPHEMERAL)
                        }
                        None => r.content("Huh, had a bit of trouble following you there chap."),
                    }
                })
            })
            .await;
        if let Err(e) = res {
            println!("Error creating interaction response: {}", e);
        } else {
            println!("Sent interaction response");
        }
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

                    notify(&self.notify_serialize);
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
            notify(&self.notify_serialize);
        }
    }

    async fn cache_ready(&self, ctx: Context, _guilds: Vec<GuildId>) {
        let get_togethers = self.get_togethers.clone();
        let time_changed = self.time_changed.clone();
        let notify_serialize = self.notify_serialize.clone();
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
                    let should_serialize = !to_remove.is_empty();
                    for mid in to_remove.into_iter() {
                        get_togethers.remove(&mid);
                    }
                    if should_serialize {
                        notify(&notify_serialize);
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

        let get_togethers = self.get_togethers.clone();
        let should_serialize = self.should_serialize.clone();
        tokio::spawn(async move {
            // Whenever asked to, write the state out to file
            let mut should_serialize = should_serialize.lock().await;
            loop {
                should_serialize.recv().await;
                let state = {
                    let get_togethers = get_togethers.read().await;
                    SerializableHandlerData::from_get_togethers(&get_togethers)
                };
                if let Err(why) = Self::serialize_state(state).await {
                    println!("failed to serialize! {}", why);
                }
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
        .application_id(800856792577736754)
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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Eq, Ord)]
struct Die {
    sides: u64,
}

impl Die {
    fn roll(self) -> Roll {
        let num = rand::thread_rng().gen_range(1..=self.sides);
        if num == 1 {
            Roll::Glitch(self)
        } else {
            Roll::Value(num, self)
        }
    }
}
impl std::fmt::Display for Die {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_char('d')?;
        f.write_fmt(format_args!("{}", self.sides))
    }
}

#[derive(Debug, Clone, Copy)]
enum Roll {
    Glitch(Die),
    Value(u64, Die),
}
impl Roll {
    fn is_glitch(self) -> bool {
        match self {
            Roll::Glitch(_) => true,
            _ => false,
        }
    }
}

struct DiceRollRequest {
    dice: Vec<Die>,
}

impl DiceRollRequest {
    fn parse(s: &str) -> Result<Self, String> {
        let mut dice = Vec::new();
        for s in s.split_whitespace() {
            if s.trim().is_empty() {
                continue;
            }
            let (count, die) = DiceRollRequest::get_die_count(s)
                .ok_or_else(|| format!("Expected {} to be like XdY, e.g. 3d6 or 1d8", s))?;
            if count > 1_000_000 {
                return Err(format!(
                    "Hey buddy, I'm just a demigod, that's too many dice!",
                ));
            }
            for _ in 0..count {
                dice.push(die);
            }
        }
        Ok(DiceRollRequest { dice })
    }

    fn get_die_count(s: &str) -> Option<(u64, Die)> {
        if let Ok(sides) = s.trim().parse() {
            return Some((1, Die { sides }));
        }
        let idx = s.find('d')?;
        let (count, sides) = (&s[..idx], &s[idx + 1..]);
        let count: u64 = if count.trim().is_empty() {
            1
        } else {
            count.trim().parse().ok()?
        };
        let sides = sides.trim().parse().ok()?;
        Some((count, Die { sides }))
    }

    fn roll(self) -> RollResult {
        let mut rolls = Vec::new();
        for die in self.dice {
            rolls.push(die.roll());
        }
        RollResult { rolled_die: rolls }
    }
}

struct RollResult {
    rolled_die: Vec<Roll>,
}
impl RollResult {
    fn is_botch(&self) -> bool {
        self.rolled_die.iter().all(|r| r.is_glitch())
    }

    fn to_discord_markdown(&mut self) -> String {
        let mut s = String::new();
        for roll in self.rolled_die.iter() {
            match roll {
                Roll::Glitch(die) => {
                    s.push_str(&format!("**1** (d{}) ", die.sides));
                }
                Roll::Value(value, die) => {
                    s.push_str(&format!("{} (d{}) ", value, die.sides));
                }
            }
        }
        s += "\n\n";
        s += &self.short_summary();
        s
    }

    fn short_summary(&mut self) -> String {
        let mut s = String::new();
        if self.is_botch() {
            s += "**BOTCH!**";
            return s;
        }
        let glitch_count = self.rolled_die.iter().filter(|r| r.is_glitch()).count();
        if glitch_count > 0 {
            s += &format!("{} Glitches!\n", glitch_count);
        }
        let highest_effect = self.get_highest_effect();
        let highest_total = self.get_highest_total();
        match (highest_effect, highest_total) {
            (CortexResult::Botch, _) => {
                return "Internal error, disagreement on botch??".to_string();
            }
            (_, CortexResult::Botch) => {
                return "Internal error, disagreement on botch??".to_string();
            }
            (
                CortexResult::Result {
                    total: etotal,
                    effect: eeffect,
                },
                CortexResult::Result {
                    total: ttotal,
                    effect: teffect,
                },
            ) => {
                if highest_effect == highest_total {
                    // There is one ideal interpretation
                    s += &format!("Total: {} (effect {})", etotal, eeffect);
                } else {
                    s.push_str(&format!("Best effect: {} (effect {})\n", etotal, eeffect));
                    s.push_str(&format!("Best total: {} (effect {})\n", ttotal, teffect));
                }
            }
        }
        s
    }

    fn get_highest_effect(&self) -> CortexResult {
        let non_glitches = self
            .rolled_die
            .iter()
            .filter_map(|roll| match roll {
                Roll::Glitch(_) => None,
                Roll::Value(value, die) => Some((*value, *die)),
            })
            .enumerate()
            .collect::<Vec<_>>();
        let effect = non_glitches
            .iter()
            .max_by_key(|(_, (value, die))| (die.sides, -((*value) as i128)));
        let (effect_idx, effect) = match effect {
            None => return CortexResult::Botch,
            Some((index, (_, die))) => (*index, *die),
        };
        if non_glitches.len() < 3 {
            // too few rolls to have an effect die, fall back to a d4
            return CortexResult::Result {
                effect: Die { sides: 4 },
                total: non_glitches.into_iter().map(|(_, (v, _))| v).sum(),
            };
        }
        let mut remaining_vals: Vec<_> = non_glitches
            .into_iter()
            .filter(|(idx, _)| *idx != effect_idx)
            .map(|(_, (value, _))| value)
            .collect();
        remaining_vals.sort();
        let val = remaining_vals.iter().rev().take(2).sum();
        CortexResult::Result { total: val, effect }
    }

    fn get_highest_total(&mut self) -> CortexResult {
        self.rolled_die.sort_by_key(|roll| match roll {
            Roll::Glitch(_) => (0, 0),
            Roll::Value(v, d) => (*v, -(d.sides as i128)),
        });
        let total = self
            .rolled_die
            .iter()
            .rev()
            .take(2)
            .filter_map(|roll| {
                if let Roll::Value(v, _) = roll {
                    Some(*v)
                } else {
                    None
                }
            })
            .sum();
        if total == 0 {
            return CortexResult::Botch;
        }
        let effect = self
            .rolled_die
            .iter()
            .rev()
            .skip(2)
            .filter_map(|d| {
                if let Roll::Value(_, d) = d {
                    Some(*d)
                } else {
                    None
                }
            })
            .max()
            .unwrap_or(Die { sides: 4 });
        CortexResult::Result { total, effect }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CortexResult {
    Botch,
    Result { total: u64, effect: Die },
}
