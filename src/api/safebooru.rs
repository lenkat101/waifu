use colored::Colorize;
use rand::distributions::{Distribution, Uniform};
use serde_json::Value;
use std::{error::Error, fmt};

use crate::api::reformat_search_tags;
use crate::app::Safebooru;

pub fn grab_random_image(args: Safebooru) -> String {
    let request_url = evaluate_arguments(&args);
    let data = match fetch_api_data(request_url) {
        Ok(json_data) => json_data,
        Err(error) => {
            eprintln!("{}\n", error);
            if args.questionable {
                println!(
                    "{}: Couldn't fetch API data. There's probably no questionable images associated with your tag(s).",
                    "help".green()
                );
            } else {
                println!(
                    "{}: Couldn't fetch API data. Try checking your tag(s) for errors.",
                    "help".green()
                );
            }

            std::process::exit(1);
        }
    };

    if data.is_empty() {
        eprintln!("No images found for the given tags.");
        std::process::exit(1);
    }

    let mut rng = rand::thread_rng();
    let random_number = Uniform::from(0..data.len());
    let index = random_number.sample(&mut rng);

    let image = &data[index];

    let image_url = format!(
        "https://safebooru.org//images/{dir}/{img}?{id}",
        dir = image.directory,
        img = image.image,
        id = image.id
    );

    if args.details {
        let ImageData {
            rating,
            width,
            height,
            tags,
            ..
        } = image;

        let details = ImageInfo {
            url: &image_url,
            rating,
            width: *width,
            height: *height,
            tags: tags.split(' ').collect(),
        };

        if let Err(error) = print_image_details(details) {
            eprintln!("{}\n", error);
            println!(
                "{}: There was an error when printing the tags. Please try again later.",
                "help".green()
            );
            std::process::exit(1);
        }
    }

    image_url
}

fn evaluate_arguments(args: &Safebooru) -> String {
    let Safebooru {
        questionable, tags, ..
    } = args;

    let tags = match tags {
        Some(search_items) => search_items,
        None => "",
    };

    let search_tags = String::from(tags);
    let mut tags = reformat_search_tags(search_tags);

    if *questionable {
        tags.push_str("%20rating:questionable");
    }

    let tags = format!("&tags={}", tags);
    // No key needed for access
    let mut api =
        String::from("https://safebooru.org/index.php?page=dapi&s=post&q=index&limit=100&json=1");
    api.push_str(&tags);

    api
}

#[derive(Debug)]
struct ImageData {
    directory: String,
    image: String,
    id: u32,
    rating: String,
    width: u32,
    height: u32,
    tags: String,
}

#[derive(Debug)]
struct ResponseError(String);

impl fmt::Display for ResponseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Error for ResponseError {}

fn parse_u32(value: Option<&Value>) -> u32 {
    match value {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0) as u32,
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

fn fetch_api_data(url: String) -> Result<Vec<ImageData>, Box<dyn Error>> {
    use reqwest::blocking::Client;
    use std::time::Duration;

    let client = Client::builder().timeout(Duration::from_secs(15)).build()?;
    let response = client.get(&url).send()?;
    let status = response.status();
    let text = response.text()?;

    if text.trim_start().starts_with('<') {
        let message = "Safebooru returned HTML or an unexpected response.";
        return Err(Box::new(ResponseError(message.into())));
    }

    if !status.is_success() {
        let message = format!("{}: Safebooru returned non-success status.", status);
        return Err(Box::new(ResponseError(message)));
    }

    let raw: Value = serde_json::from_str(&text)
        .map_err(|e| ResponseError(format!("Failed to parse JSON: {}", e)))?;
    let arr = raw
        .as_array()
        .ok_or_else(|| ResponseError("Unexpected JSON structure".into()))?;

    let mut data = Vec::new();
    for item in arr {
        let directory = item
            .get("directory")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let image = item
            .get("image")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let id = parse_u32(item.get("id"));
        let rating = item
            .get("rating")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let width = parse_u32(item.get("width"));
        let height = parse_u32(item.get("height"));
        let tags = item
            .get("tags")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        data.push(ImageData {
            directory,
            image,
            id,
            rating,
            width,
            height,
            tags,
        });
    }

    Ok(data)
}

struct ImageInfo<'a> {
    url: &'a str,
    rating: &'a str,
    width: u32,
    height: u32,
    tags: Vec<&'a str>,
}

fn print_image_details(info: ImageInfo) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::{self, Write};

    let ImageInfo {
        url,
        rating,
        width,
        height,
        tags,
    } = info;

    println!("✉️ {title}: {}", url, title = "Link".cyan());
    println!("⚖️ {title}: {}", rating, title = "Rating".cyan());
    println!(
        "📐 {title}: {w} x {h}",
        title = "Dimensions".cyan(),
        w = width,
        h = height
    );

    let stdout = io::stdout();
    let lock = stdout.lock();
    let mut buffer = io::BufWriter::new(lock);

    write!(buffer, "🏷️ {}:", "Tags".cyan())?;
    tags.iter().try_for_each(|tag| write!(buffer, " {}", tag))?;

    writeln!(buffer)?;

    Ok(())
}
