use hypnos::dalle::{ImageRequest, Dimensions, Style, Quality, OpenAIImageGen};
use hyper::{Body, Request, Response, Server};
use hyper::service::{make_service_fn, service_fn};

#[tokio::test]
async fn test_create_image_uses_fake_server() {
    // start a simple hyper server that returns a fixed response
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, hyper::Error>(service_fn(|_req: Request<Body>| async move {
            let body = "{\"data\": [{\"revised_prompt\": \"hi\", \"b64_json\": \"aGVsbG8=\"}]}";
            Ok::<_, hyper::Error>(
                Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
        }))
    });

    let server = Server::bind(&([127,0,0,1], 0).into()).serve(make_svc);
    let addr = server.local_addr();
    let handle = tokio::spawn(server);

    std::env::set_var("OPENAI_API_KEY", "test-key");
    std::env::set_var("OPENAI_IMAGE_GEN_URL", format!("http://{}/v1/images/generations", addr));

    let gen = OpenAIImageGen::new().unwrap();
    let req = ImageRequest::new(
        "hello".to_string(),
        1,
        Dimensions::Square,
        Style::Vivid,
        Quality::Standard,
    );

    let images = gen.create_image(req).await.unwrap();
    assert_eq!(images.len(), 1);
    assert!(images[0].is_ok());

    handle.abort();
}
