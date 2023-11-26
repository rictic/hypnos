use crate::data::{Context, Error};
use base64::Engine;
use futures::future::join_all;
use poise::serenity_prelude as serenity;
use serde_json::json;

#[poise::command(slash_command)]
pub async fn gen(
    ctx: Context<'_>,
    #[description = "The description of the image"] description: String,
) -> Result<(), Error> {
    let reply = ctx.reply("Generating image...").await?;
    let reply_message = reply.message().await.ok();
    let images = OpenAIImageGen::new()?
        .create_image(ImageRequest {
            description,
            num: 4,
            dimensions: Dimensions::Square,
        })
        .await?;
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
struct ImageRequest {
    description: String,
    num: u8,
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
                            "quality": "hd",
                            "style": "vivid",
                        }))
                        .send()
                        .await?
                        .text()
                        .await?;

                    let json_response: OpenAIImages =
                        serde_json::from_str(&response).map_err(|op| {
                            format!("Failed to parse OpenAI response as JSON: {:?}", op).to_string()
                        })?;
                    let images = match json_response.data {
                        Some(images) => images,
                        None => return Err("OpenAI returned no images".into()),
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

pub struct Image {
    revised_prompt: Option<String>,
    bytes: Vec<u8>,
}

impl Image {
    pub fn from_open_ai(response: OpenAIImageData) -> Result<Self, Error> {
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
