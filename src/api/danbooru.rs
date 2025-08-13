use colored::Colorize;
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::Value;
use std::error::Error;
use std::fmt;

use crate::api::reformat_search_tags;
use crate::app::Danbooru;

pub fn grab_random_image(args: Danbooru) -> String {
    let request_url = evaluate_arguments(&args);
    let data = match fetch_api_data(request_url) {
        Ok(json_data) => json_data,
        Err(error) => {
            eprintln!("{}\n", error);
            std::process::exit(1);
        }
    };

    let valid_data: Vec<&ImageData> = data
        .iter()
        .filter(|image| !image.file_url.is_empty())
        .collect();
    if valid_data.is_empty() {
        eprintln!("Danbooru returned no images with accessible URLs.");
        std::process::exit(1);
    }
    let image = &valid_data[0];
    let image_url = &image.file_url;

    if args.details {
        if let Err(error) = print_image_details(image) {
            eprintln!("{}\n", error);
            println!(
                "{}: There was an error when printing the tags. Please try again later.",
                "help".green()
            );
            std::process::exit(1);
        }
    }

    image_url.to_string()
}

fn check_env_variables() -> (Option<String>, Option<String>) {
    use std::env;

    let mut login_info = (None, None);

    for (key, value) in env::vars() {
        let key = key.as_str();
        match key {
            "DANBOORU_USERNAME" => {
                login_info.0 = Some(value);
            }
            "DANBOORU_API_KEY" => {
                login_info.1 = Some(value);
            }
            &_ => (),
        }
    }

    login_info
}

fn evaluate_arguments(args: &Danbooru) -> String {
    let mut api = String::from("https://danbooru.donmai.us/posts.json?random=true");

    if let Some(username) = &args.username {
        if let Some(api_key) = &args.key {
            let login_info = format!("&login={}&api_key={}", username, api_key);
            api.push_str(login_info.as_str());
        }
    } else if let (Some(username), Some(api_key)) = check_env_variables() {
        let login_info = format!("&login={}&api_key={}", username, api_key);
        api.push_str(login_info.as_str());
    }

    let Danbooru {
        safe,
        questionable,
        explicit,
        tags,
        ..
    } = args;

    let tags = match tags {
        Some(search_items) => search_items,
        None => "",
    };

    let search_tags = String::from(tags);
    let mut tags = reformat_search_tags(search_tags);

    if *safe {
        tags.push_str("%20rating:s");
    } else if *questionable {
        tags.push_str("%20rating:q");
    } else if *explicit {
        tags.push_str("%20rating:e");
    }

    let tags = format!("&tags={}", tags);
    api.push_str(&tags);

    api
}

#[derive(Debug)]
struct ImageData {
    source: String,
    pixiv_id: Option<u32>,
    file_url: String,
    tag_string_character: String,
    tag_string_artist: String,
    rating: char,
    image_width: u32,
    image_height: u32,
    tag_string: String,
}

#[derive(Deserialize, Debug)]
struct FailureResponse {
    message: String,
}

#[derive(Debug)]
struct ResponseError(String);

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for ResponseError {}

fn value_to_string(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn parse_u32(v: Option<&Value>) -> u32 {
    match v {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn parse_opt_u32(v: Option<&Value>) -> Option<u32> {
    match v {
        Some(Value::Number(n)) => n.as_u64().map(|v| v as u32),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

fn fetch_api_data(url: String) -> Result<Vec<ImageData>, Box<dyn Error>> {
    use reqwest::blocking::Client;
    use std::time::Duration;

    let client = Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 (compatible; waifu/1.0; +https://github.com/lenkat101/waifu)")
        .build()?;
    let response = client.get(&url).send()?;
    let status = response.status();
    let text = response.text()?;

    if text.trim_start().starts_with('<') {
        let message = format!("{}: API returned HTML or an unexpected response.", status);
        return Err(Box::new(ResponseError(message)));
    }

    if status != StatusCode::OK {
        if let Ok(err) = serde_json::from_str::<FailureResponse>(&text) {
            let message = format!("{}: {}", status, err.message);
            return Err(Box::new(ResponseError(message)));
        } else {
            let message = format!("{}: Unexpected response.", status);
            return Err(Box::new(ResponseError(message)));
        }
    }

    let raw: Value = serde_json::from_str(&text)
        .map_err(|e| ResponseError(format!("Failed to parse JSON: {}", e)))?;
    let arr = raw
        .as_array()
        .ok_or_else(|| ResponseError("Unexpected JSON structure".into()))?;

    let mut data = Vec::new();
    for item in arr {
        let source = value_to_string(item.get("source"));
        let pixiv_id = parse_opt_u32(item.get("pixiv_id"));
        let file_url_raw = item
            .get("file_url")
            .and_then(Value::as_str)
            .or_else(|| item.get("large_file_url").and_then(Value::as_str))
            .unwrap_or("");
        let mut file_url = file_url_raw.to_string();
        if file_url.starts_with("//") {
            file_url = format!("https:{}", file_url);
        }
        let tag_string_character = value_to_string(item.get("tag_string_character"));
        let tag_string_artist = value_to_string(item.get("tag_string_artist"));
        let rating = item
            .get("rating")
            .and_then(Value::as_str)
            .and_then(|s| s.chars().next())
            .unwrap_or('s');
        let image_width = parse_u32(item.get("image_width"));
        let image_height = parse_u32(item.get("image_height"));
        let tag_string = value_to_string(item.get("tag_string"));

        data.push(ImageData {
            source,
            pixiv_id,
            file_url,
            tag_string_character,
            tag_string_artist,
            rating,
            image_width,
            image_height,
            tag_string,
        });
    }

    if data.is_empty() {
        let message = format!(
            "{}: Although the request succeeded, there are no images associated with your tags.",
            status
        );
        return Err(Box::new(ResponseError(message)));
    }

    Ok(data)
}

fn print_image_details(info: &ImageData) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, Write};

    let ImageData {
        source,
        pixiv_id,
        file_url,
        tag_string_character,
        tag_string_artist,
        rating,
        image_height,
        image_width,
        tag_string,
    } = info;

    if !tag_string_character.is_empty() {
        println!(
            "✨ {title}: {}",
            tag_string_character,
            title = "Character".purple()
        );
    }

    if !source.is_empty() {
        if source.contains("pixiv") || source.contains("pximg") {
            if let Some(id) = pixiv_id {
                let pixiv_source = format!("https://pixiv.net/en/artworks/{}", id);
                println!("ℹ️ {title}: {}", pixiv_source, title = "Source".purple());
            } else {
                // Fallback to printing the provided source if no pixiv_id available
                println!("ℹ️ {title}: {}", source, title = "Source".purple());
            }
        } else {
            println!("ℹ️ {title}: {}", source, title = "Source".purple());
        }
    }

    if !tag_string_artist.is_empty() {
        println!(
            "🎨 {title}: {}",
            tag_string_artist,
            title = "Artist".purple()
        );
    }

    println!("✉️ {title}: {}", file_url, title = "Link".purple());

    match rating {
        's' => println!("⚖️ {title}: safe", title = "Rating".purple()),
        'q' => println!("⚖️ {title}: questionable", title = "Rating".purple()),
        'e' => println!("⚖️ {title}: explicit", title = "Rating".purple()),
        _ => (),
    }

    println!(
        "📐 {title}: {w} x {h}",
        title = "Dimensions".purple(),
        w = image_width,
        h = image_height
    );

    let tags: Vec<&str> = tag_string.split(' ').collect();
    let stdout = io::stdout();
    let lock = stdout.lock();
    let mut buffer = io::BufWriter::new(lock);

    write!(buffer, "🏷️ {}:", "Tags".purple())?;
    tags.iter().try_for_each(|tag| write!(buffer, " {}", tag))?;

    writeln!(buffer)?;

    Ok(())
}
