use crate::data::{Context, Cost, Error};
use base64::Engine;
use futures::future::join_all;
use poise::serenity_prelude as serenity;
use serde_json::json;

#[poise::command(slash_command)]
pub async fn gen(
    ctx: Context<'_>,
    #[description = "The description of the image. DALL-E will automatically expand it."]
    description: String,
    #[description = "The number of images to generate"] num: Option<u8>,
    #[description = "The aspect ratio"] size: Option<Dimensions>,
    #[description = "Should the image be super colorful or are more muted colors ok?"]
    style: Option<Style>,
    #[description = "The quality of the image that will be generated."] quality: Option<Quality>,
) -> Result<(), Error> {
    let user = ctx.author();
    let num = num.unwrap_or(4);
    if num > 10 {
        ctx.reply("This mortal frame can't handle such treasures. Ten is the max at once, chum")
            .await?;
        return Ok(());
    }
    if num == 0 {
        ctx.reply("Getting philosophical with us eh? Here's zero images for you:")
            .await?;
        return Ok(());
    }
    let request = ImageRequest {
        description,
        num,
        dimensions: size.unwrap_or(Dimensions::Square),
        style: style.unwrap_or(Style::Vivid),
        quality: quality.unwrap_or(Quality::Standard),
    };
    let permitted = crate::data::debit_for_request(ctx.data(), user, &request).await?;
    if permitted == crate::data::RequestPermitted::No {
        ctx.send(|m| {
            m.content("Limit reached. Ping rictic and ask him to to update your limits.")
                .reply(true)
                .ephemeral(true)
        })
        .await?;
        return Ok(());
    }
    let reply = if num == 1 {
        ctx.reply("Generating image...").await?
    } else {
        ctx.reply(format!("Generating {} images...", num)).await?
    };
    let reply_message = reply.message().await.ok();
    let images = OpenAIImageGen::new()?.create_image(request).await?;
    let mut failures = 0;
    let mut actual_images = Vec::new();
    for image in images.into_iter() {
        match image {
            Ok(image) => {
                actual_images.push(image);
            }
            Err(err) => {
                failures += 1;
                println!("Failed to generate image: {}", err);
            }
        }
    }

    ctx.channel_id()
        .send_files(
            ctx.http(),
            actual_images
                .into_iter()
                .map(|image| serenity::AttachmentType::Bytes {
                    data: std::borrow::Cow::Owned(image.bytes),
                    filename: format!(
                        "{}.png",
                        image.revised_prompt.unwrap_or("image".to_string())
                    )
                    .to_string(),
                }),
            |f| match reply_message {
                Some(msg) => f.reference_message((ctx.channel_id(), msg.id)),
                None => f,
            },
        )
        .await?;
    reply
        .edit(ctx, |m| {
            let mut response = "Generated!".to_string();
            if failures > 0 {
                response = format!("{} ({} failed)", response, failures);
            }
            let m = m.content(response);
            // for (name, image) in files.iter() {
            //     m = m.attachment(serenity::AttachmentType::File {
            //         file: &image,
            //         filename: name.to_string(),
            //     })
            // }
            m
        })
        .await?;
    Ok(())
}

const OPENAI_IMAGE_GEN_URL: &'static str = "https://api.openai.com/v1/images/generations";

#[derive(Debug, serde::Deserialize, Clone)]
struct OpenAIImages {
    data: Option<Vec<OpenAIImageData>>,
}

#[derive(Debug, serde::Deserialize, Clone)]
struct OpenAIImageData {
    revised_prompt: Option<String>,
    b64_json: String,
}
impl OpenAIImageData {}

struct OpenAIImageGen {
    key: String,
}

impl OpenAIImageGen {
    fn new() -> Result<Self, String> {
        let key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| Err("missing OPENAI_API_KEY env variable".to_string()))?;

        Ok(Self { key })
    }
}

#[derive(Debug, Clone)]
pub struct ImageRequest {
    description: String,
    num: u8,
    dimensions: Dimensions,
    style: Style,
    quality: Quality,
}
impl ImageRequest {
    pub fn cost(&self) -> Cost {
        // https://openai.com/pricing#:~:text=Other%20models-,Image%20models,-Build%20DALL%C2%B7E%20directly
        let base_cents = match (self.dimensions, self.quality) {
            (Dimensions::Square, Quality::Standard) => 4,
            (Dimensions::Square, Quality::HD) => 8,
            (_, Quality::Standard) => 8,
            (_, Quality::HD) => 12,
        };
        return Cost::cents(base_cents * self.num as u64);
    }

    pub fn num_images(&self) -> u8 {
        self.num
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum Dimensions {
    #[name = "A wide landscape image, 1792x1024"]
    Wide,
    #[name = "A tall portrait image, 1024x1792"]
    Tall,
    #[name = "A square image, 1024x1024"]
    Square,
}
impl Dimensions {
    fn to_size(&self) -> &'static str {
        match self {
            Dimensions::Square => "1024x1024",
            Dimensions::Wide => "1792x1024",
            Dimensions::Tall => "1024x1792",
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum Style {
    #[name = "More natural, less hyper-real looking images"]
    Natural,
    #[name = "Generate hyper-real and dramatic images"]
    Vivid,
}
impl Style {
    fn to_str(&self) -> &'static str {
        match self {
            Style::Natural => "natural",
            Style::Vivid => "vivid",
        }
    }
}

#[derive(Debug, Clone, Copy, poise::ChoiceParameter)]
pub enum Quality {
    #[name = "The default"]
    Standard,
    #[name = "Finer details and greater consistency across the image"]
    HD,
}
impl Quality {
    fn to_str(&self) -> &'static str {
        match self {
            Quality::Standard => "standard",
            Quality::HD => "hd",
        }
    }
}

impl OpenAIImageGen {
    async fn create_image(
        &self,
        request: ImageRequest,
    ) -> Result<Vec<Result<Image, Error>>, Error> {
        let client = reqwest::Client::new();

        let mut tasks = vec![];
        for _ in 0..request.num {
            let client = client.clone();
            let key = self.key.clone();
            let request_clone = request.clone(); // Assuming ImageRequest is cloneable

            let task: tokio::task::JoinHandle<Result<Vec<Result<Image, Error>>, Error>> =
                tokio::spawn(async move {
                    let response = client
                        .post(OPENAI_IMAGE_GEN_URL)
                        .bearer_auth(&key)
                        .json(&json!({
                            "model": "dall-e-3",
                            "n": 1,
                            "response_format": "b64_json",
                            "size": request_clone.dimensions.to_size(),
                            "prompt": request_clone.description,
                            "quality": request_clone.quality.to_str(),
                            "style": request_clone.style.to_str(),
                        }))
                        .send()
                        .await?
                        .text()
                        .await?;

                    let json_response: OpenAIImages =
                        serde_json::from_str(&response).map_err(|op| {
                            format!(
                                "Failed to parse OpenAI response as JSON: {:?}. Full response: {}",
                                op, response
                            )
                            .to_string()
                        })?;
                    let images = match json_response.data {
                        Some(images) => images,
                        None => {
                            return Err(format!("OpenAI returned no images: {}", response).into())
                        }
                    };
                    let images: Vec<Result<Image, Error>> = images
                        .into_iter()
                        .map(|image| Image::from_open_ai(image))
                        .collect();
                    Ok(images)
                });
            tasks.push(task);
        }

        let responses = join_all(tasks)
            .await
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;

        let mut images = Vec::new();
        for response in responses.into_iter() {
            match response {
                Ok(mut r) => images.append(&mut r),
                Err(err) => images.push(Err(err)),
            }
        }

        Ok(images)
    }
}

struct Image {
    revised_prompt: Option<String>,
    bytes: Vec<u8>,
}

impl Image {
    fn from_open_ai(response: OpenAIImageData) -> Result<Self, Error> {
        let bytes = match base64::engine::general_purpose::STANDARD.decode(response.b64_json) {
            Ok(bytes) => bytes,
            Err(_) => return Err("failed to decode base64 image from OpenAI".into()),
        };
        Ok(Self {
            revised_prompt: response.revised_prompt,
            bytes,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dimensions_to_size() {
        assert_eq!(Dimensions::Square.to_size(), "1024x1024");
        assert_eq!(Dimensions::Wide.to_size(), "1792x1024");
        assert_eq!(Dimensions::Tall.to_size(), "1024x1792");
    }

    #[test]
    fn test_quality_to_str() {
        assert_eq!(Quality::Standard.to_str(), "standard");
        assert_eq!(Quality::HD.to_str(), "hd");
    }

    #[test]
    fn test_style_to_str() {
        assert_eq!(Style::Natural.to_str(), "natural");
        assert_eq!(Style::Vivid.to_str(), "vivid");
    }

    #[test]
    fn test_image_request_cost() {
        let req = ImageRequest {
            description: "desc".to_string(),
            num: 2,
            dimensions: Dimensions::Square,
            style: Style::Vivid,
            quality: Quality::Standard,
        };
        let cost = req.cost();
        let v = serde_json::to_value(cost).unwrap();
        assert_eq!(v["millicents"], serde_json::json!(8000));
        assert_eq!(req.num_images(), 2);
    }
}
