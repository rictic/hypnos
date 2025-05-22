use hypnos::dalle::{ImageRequest, Dimensions, Style, Quality};

#[test]
fn test_image_request_cost_standard() {
    let req = ImageRequest::new(
        "a sunset".to_string(),
        2,
        Dimensions::Square,
        Style::Vivid,
        Quality::Standard,
    );
    let cost = req.cost();
    let v = serde_json::to_value(cost).unwrap();
    assert_eq!(v["millicents"], serde_json::json!(8000));
    assert_eq!(req.num_images(), 2);
}

#[test]
fn test_image_request_cost_hd() {
    let req = ImageRequest::new(
        "a castle".to_string(),
        1,
        Dimensions::Wide,
        Style::Natural,
        Quality::HD,
    );
    let cost = req.cost();
    let v = serde_json::to_value(cost).unwrap();
    assert_eq!(v["millicents"], serde_json::json!(12000));
}
