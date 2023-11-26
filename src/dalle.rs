use crate::data::{Context, Error};
use base64::Engine;
use poise::serenity_prelude as serenity;

#[poise::command(slash_command)]
pub async fn gen(
    ctx: Context<'_>,
    #[description = "The description of the image"] description: String,
) -> Result<(), Error> {
    let reply = ctx.reply("Generating image...").await?;
    let images = OpenAIImageGen::new()?.create_image(ImageRequest {
        description,
        dimensions: Dimensions::Square,
    })?;
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
            |f| f,
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
pub struct OpenAIImages {
    pub created: u64,
    pub data: Option<Vec<OpenAIImageData>>,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct OpenAIImageData {
    pub revised_prompt: Option<String>,
    pub b64_json: String,
}
impl OpenAIImageData {}

pub struct OpenAIImageGen {
    key: String,
}

impl OpenAIImageGen {
    pub fn new() -> Result<Self, String> {
        let key = std::env::var("OPENAI_API_KEY")
            .or_else(|_| Err("missing OPENAI_API_KEY env variable".to_string()))?;

        Ok(Self { key })
    }
}

#[derive(Debug, Clone)]
pub struct ImageRequest {
    pub description: String,
    dimensions: Dimensions,
}

#[derive(Debug, Clone, Copy)]
pub enum Dimensions {
    Square,
}
impl Dimensions {
    fn to_size(&self) -> &'static str {
        match self {
            Dimensions::Square => "1024x1024",
            // Dimensions::Wide => "1792x1024",
            // Dimensions::Tall => "1024x1792",
        }
    }
}

impl OpenAIImageGen {
    fn create_image(&self, request: ImageRequest) -> Result<Vec<Result<Image, String>>, String> {
        let response = ureq::post(OPENAI_IMAGE_GEN_URL)
            .set("Authorization", &format!("Bearer {}", self.key))
            .set("Content-Type", "application/json")
            .send_json(ureq::json!({
              "model": "dall-e-3",
              "n": 1,
              "response_format": "b64_json",
              "size": request.dimensions.to_size(),
              "prompt": request.description,
              "quality": "hd",
              "style": "vivid",
            }))
            .map_err(|e| match e {
                ureq::Error::Status(code, response) => {
                    format!(
                        "OpenAI returned status code {}: {}",
                        code,
                        response
                            .into_string()
                            .unwrap_or("unable to get response".to_string())
                    )
                }
                ureq::Error::Transport(e) => format!("Transport error: {}", e.to_string()),
            })?
            .into_string()
            .map_err(|e| e.to_string())?;
        println!("OpenAI response: {}", response);

        let images: OpenAIImages = serde_json::from_str(&response).map_err(|op| {
            format!(
                "Failed to parse OpenAI response as JSON: {:?}\n\nResponse: {}",
                op, response
            )
        })?;
        let images = match images.data {
            None => return Err("OpenAI returned no images".to_string()),
            Some(images) => images,
        };
        Ok(images
            .into_iter()
            .map(|image| Image::from_open_ai(image))
            .collect())
    }
}

pub struct Image {
    revised_prompt: Option<String>,
    bytes: Vec<u8>,
}

impl Image {
    pub fn from_open_ai(response: OpenAIImageData) -> Result<Self, String> {
        let bytes = match base64::engine::general_purpose::STANDARD.decode(response.b64_json) {
            Ok(bytes) => bytes,
            Err(_) => return Err("failed to decode base64 image from OpenAI".to_string()),
        };
        Ok(Self {
            revised_prompt: response.revised_prompt,
            bytes,
        })
    }
}
