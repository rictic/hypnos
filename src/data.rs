use std::collections::BTreeMap;

use poise::serenity_prelude as serenity;
use tokio::sync::Mutex;

// User data, which is stored and accessible in all command invocations
pub struct Data {
    cost_map: Mutex<CostMap>,
}
impl Data {
    pub async fn read_or_create() -> Result<Self, Error> {
        let data = std::fs::read_to_string("data.json").unwrap_or_else(|_| "{}".to_string());
        let cost_map = serde_json::from_str(&data).unwrap_or_default();
        Ok(Self {
            cost_map: Mutex::new(cost_map),
        })
    }
}
impl Default for Data {
    fn default() -> Self {
        Self {
            cost_map: Mutex::new(BTreeMap::new()),
        }
    }
}

type CostMap = BTreeMap<u64, CostRecord>;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CostRecord {
    cost: Cost,
    user: String,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestPermitted {
    Yes,
    No,
}

pub(crate) async fn report_cost(
    data: &Data,
    user: &serenity::User,
    cost: Cost,
) -> Result<RequestPermitted, Error> {
    let user_id = user.id.0;

    {
        let mut cost_map = data.cost_map.lock().await;

        let cost_record = cost_map.entry(user_id).or_insert(CostRecord {
            cost: Cost::zero(),
            user: format!("{}#{}", user.name, user.discriminator),
        });
        if cost_record.cost.over_limit() {
            return Ok(RequestPermitted::No);
        }
        cost_record.cost += cost;
        // serialize the cost map to a data.json
        let serialized = serde_json::to_string(&*cost_map)?;
        // write that to a file using tokio file io
        tokio::fs::write("data.json", serialized).await?;
    }

    Ok(RequestPermitted::Yes)
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

    fn over_limit(&self) -> bool {
        // 20 dollars is 20 million millicents
        let limit = 20 * 1000 * 1000;
        self.millicents > limit
    }

    fn zero() -> Cost {
        Cost { millicents: 0 }
    }
}

impl std::ops::AddAssign for Cost {
    fn add_assign(&mut self, rhs: Self) {
        self.millicents += rhs.millicents;
    }
}
