use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};


use poise::serenity_prelude as serenity;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::dalle::ImageRequest;

// User data, which is stored and accessible in all command invocations
pub struct Data {
    accounts: Mutex<CostMap>,
    pub low_traffic_channels: HashSet<serenity::ChannelId>,
    traffic: Mutex<LowTrafficTracker>,
}
impl Data {
    pub async fn read_or_create() -> Result<Self, Error> {
        let data = std::fs::read_to_string("data.json").unwrap_or_else(|_| "{}".to_string());
        let cost_map = serde_json::from_str(&data).unwrap_or_default();
        let channels_env = std::env::var("LOW_TRAFFIC_CHANNELS").unwrap_or_default();
        let low_traffic_channels = channels_env
            .split(',')
            .filter_map(|s| s.trim().parse::<u64>().ok())
            .map(serenity::ChannelId)
            .collect();
        Ok(Self {
            accounts: Mutex::new(cost_map),
            low_traffic_channels,
            traffic: Mutex::new(LowTrafficTracker::new()),
        })
    }
}
impl Default for Data {
    fn default() -> Self {
        Self {
            accounts: Mutex::new(BTreeMap::new()),
            low_traffic_channels: HashSet::new(),
            traffic: Mutex::new(LowTrafficTracker::new()),
        }
    }
}

type CostMap = BTreeMap<u64, Account>;

struct ChannelTraffic {
    messages: VecDeque<Instant>,
    last_warn: Option<Instant>,
}

impl ChannelTraffic {
    fn new() -> Self {
        Self {
            messages: VecDeque::new(),
            last_warn: None,
        }
    }
}

pub struct LowTrafficTracker {
    map: HashMap<u64, ChannelTraffic>,
}

impl LowTrafficTracker {
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    /// Records a message in the given channel. Returns true if a warning should be sent.
    pub fn record(&mut self, channel_id: u64) -> bool {
        let now = Instant::now();
        let entry = self
            .map
            .entry(channel_id)
            .or_insert_with(ChannelTraffic::new);
        entry.messages.push_back(now);
        while let Some(&front) = entry.messages.front() {
            if now.duration_since(front) > Duration::from_secs(5 * 60) {
                entry.messages.pop_front();
            } else {
                break;
            }
        }

        if entry.messages.len() > 3 {
            match entry.last_warn {
                Some(t) if now.duration_since(t) < Duration::from_secs(5 * 60) => false,
                _ => {
                    entry.last_warn = Some(now);
                    true
                }
            }
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub user: String,
    pub images: u64,
    // in millicents
    pub credit: i64,
    pub total_cost: i64,
}
impl Account {
    pub fn overdrafted(&self) -> bool {
        self.credit < 0
    }

    fn account_for_request(&mut self, request: &ImageRequest) {
        let cost = request.cost();
        self.credit -= cost.millicents as i64;
        self.total_cost += cost.millicents as i64;
        self.images += request.num_images() as u64;
    }
}
impl Account {
    fn default_for_user(user: &serenity::User) -> Self {
        Account {
            images: 0,
            // erry body gets 20 bucks
            credit: 20 * 100 * 1000,
            total_cost: 0,
            user: format!("{}#{}", user.name, user.discriminator),
        }
    }
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum RequestPermitted {
    Yes,
    No,
}

pub(crate) async fn debit_for_request(
    data: &Data,
    user: &serenity::User,
    request: &ImageRequest,
) -> Result<RequestPermitted, Error> {
    let user_id = user.id.0;

    let mut accounts = data.accounts.lock().await;

    let account = accounts
        .entry(user_id)
        .or_insert(Account::default_for_user(user));
    if account.overdrafted() {
        return Ok(RequestPermitted::No);
    }
    account.account_for_request(request);
    // serialize the cost map to a data.json
    let serialized = serde_json::to_string(&*accounts)?;
    // write that to a file using tokio file io
    tokio::fs::write("data.json", serialized).await?;

    Ok(RequestPermitted::Yes)
}

pub(crate) async fn get_account(data: &Data, user: &serenity::User) -> Result<Account, Error> {
    let user_id = user.id.0;
    let cost_map = data.accounts.lock().await;

    match cost_map.get(&user_id) {
        None => Ok(Account::default_for_user(user)),
        Some(account) => Ok(account.clone()),
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct Cost {
    millicents: u128,
}
impl Cost {
    pub fn cents(cents: u64) -> Self {
        Cost {
            millicents: (cents as u128) * 1000,
        }
    }
}
