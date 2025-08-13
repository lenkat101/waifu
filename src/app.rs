use clap::{Args, Parser, Subcommand, ValueHint};
use is_terminal::IsTerminal;
use std::error::Error;
use std::path::PathBuf;
use viuer::{print, print_from_file};

const MAX_IMAGE_BYTES: usize = 20 * 1024 * 1024; // 20 MiB hard cap to avoid OOM

#[derive(Parser, Debug)]
#[command(about = "View random anime fanart in your terminal")]
struct Cli {
    /// Resize the image to a provided height
    #[arg(short = 'H', long)]
    height: Option<u32>,

    /// Resize the image to a provided width
    #[arg(short = 'W', long)]
    width: Option<u32>,

    #[command(subcommand)]
    subcommand: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(name = "safe")]
    Safebooru(Safebooru),

    #[command(name = "dan")]
    Danbooru(Danbooru),

    #[command(name = "url")]
    Url(Url),

    #[command(name = "file")]
    File(File),
}

/// Look at random images from Safebooru
#[derive(Args, Debug)]
pub struct Safebooru {
    /// Show data related to image (url, rating, dimensions, tags)
    #[arg(short, long)]
    pub details: bool,

    /// Only display images with suggestive content
    #[arg(short, long)]
    pub questionable: bool,

    /// Search for an image based on Safebooru tags.
    /// Pass as a string separated by spaces or commas.         
    /// Look at Safebooru's cheatsheet for a full list of search options
    #[arg(short, long)]
    pub tags: Option<String>,
}

/// Look at random images from Danbooru
#[derive(Args, Debug)]
pub struct Danbooru {
    /// Show data related to image (artist, source, character, url, rating, dimensions, tags)
    #[arg(short, long)]
    pub details: bool,

    /// Only display images lacking sexual content. Includes lingerie,
    /// swimsuits, innocent romance, etc. NOTE: this doesn't mean "safe
    /// for work."
    #[arg(short, long, conflicts_with_all = ["questionable", "explicit"])]
    pub safe: bool,

    /// Only display images with some nox-explicit nudity or sexual content
    #[arg(short, long, conflicts_with_all = ["safe", "explicit"])]
    pub questionable: bool,

    /// Only display images with explicit sexual content
    #[arg(short, long, conflicts_with_all = ["safe", "questionable"])]
    pub explicit: bool,

    /// Search for an image based on Danbooru tags.
    /// Pass as a string separated by spaces or commas.         
    /// Look at Danbooru's cheatsheet for a full list of search options
    #[arg(short, long)]
    pub tags: Option<String>,

    /// Pass your Danbooru username for authentication.
    /// NOTE: This doesn't set a persistent environmental variable and
    /// instead only works for one session
    #[arg(short, long, requires = "key")]
    pub username: Option<String>,

    /// Pass your Danbooru API key for authentication.
    /// NOTE: This doesn't set a persistent environmental variable and
    /// instead only works for one session
    #[arg(short, long, requires = "username")]
    pub key: Option<String>,
}

/// View an image from a url
#[derive(Args, Debug)]
struct Url {
    /// The URL of an image (e.g. https://i.redd.it/7tycieudz3c61.png)
    image_url: String,
}

/// View an image from your file system
#[derive(Args, Debug)]
struct File {
    /// The path to an image file (e.g. ~/Pictures/your-image.jpg)
    #[arg(value_hint = ValueHint::FilePath)]
    file_path: PathBuf,
}

pub fn run() -> Result<(), Box<dyn Error>> {
    let args = Cli::parse();
    let result: Result<(), Box<dyn Error>>;

    let Cli { width, height, .. } = args;

    let config = viuer::Config {
        width,
        height,
        absolute_offset: false,
        ..Default::default()
    };

    // Read from stdin when data is actually present
    if !std::io::stdin().is_terminal() {
        use std::io::{stdin, Read};
        let mut buf = Vec::new();
        let _ = stdin().read_to_end(&mut buf)?;
        if !buf.is_empty() {
            if buf.len() > MAX_IMAGE_BYTES {
                return Err(format!(
                    "Input image too large ({} bytes > {} bytes)",
                    buf.len(),
                    MAX_IMAGE_BYTES
                )
                .into());
            }
            let image = image::load_from_memory(&buf)?;
            print(&image, &config)?;
            return Ok(());
        }
        // If stdin is empty, fall through to normal subcommand handling
    }

    if let Some(subcommand) = args.subcommand {
        match subcommand {
            Commands::Danbooru(args) => {
                let dan_args = Danbooru { ..args };
                let dan_args = Commands::Danbooru(dan_args);
                result = show_random_image(dan_args, config);
            }
            Commands::Safebooru(args) => {
                let safe_args = Safebooru { ..args };
                let safe_args = Commands::Safebooru(safe_args);
                result = show_random_image(safe_args, config);
            }
            Commands::File(file) => {
                result = show_image_with_path(file.file_path, config);
            }
            Commands::Url(url) => {
                result = show_image_with_url(url.image_url, config);
            }
        };
    } else {
        let default_options = Safebooru {
            details: false,
            questionable: false,
            tags: None,
        };

        let default = Commands::Safebooru(default_options);

        result = show_random_image(default, config);
    }

    result
}

fn show_random_image(args: Commands, config: viuer::Config) -> Result<(), Box<dyn Error>> {
    use crate::api::{danbooru, safebooru};

    let image_url = match args {
        Commands::Danbooru(args) => danbooru::grab_random_image(args),
        Commands::Safebooru(args) => safebooru::grab_random_image(args),
        _ => panic!(
            "Invalid subcommand passed to show_random_image. \
                Only valid ones are 'Danbooru' and 'Safebooru'."
        ),
    };

    show_image_with_url(image_url, config)
}

fn show_image_with_url(image_url: String, config: viuer::Config) -> Result<(), Box<dyn Error>> {
    use reqwest::blocking::Client;
    use reqwest::header;
    use std::fs::File;
    use std::io::Write;
    use std::time::Duration;

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(20))
        .build()?;

    // Simple retry for transient errors
    #[allow(unused_assignments)]
    let mut last_err: Option<String> = None;
    let bytes = {
        let mut attempts = 0;
        loop {
            attempts += 1;
            let resp = client.get(&image_url).send();
            match resp {
                Ok(resp) => {
                    let status = resp.status();
                    let ct = resp
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("")
                        .to_string();

                    if !status.is_success() || (!ct.is_empty() && !ct.starts_with("image/")) {
                        let mut path = std::env::temp_dir();
                        path.push("waifu_fetch_error.bin");
                        if let Ok(mut f) = File::create(&path) {
                            if let Ok(buf) = resp.bytes() {
                                let _ = f.write_all(&buf);
                            }
                        }
                        return Err(format!(
                            "Failed to fetch image: HTTP {} (content-type: {}). Saved bytes to {}",
                            status,
                            if ct.is_empty() { "unknown" } else { &ct },
                            path.display()
                        )
                        .into());
                    }

                    if let Some(len) = resp.headers().get(header::CONTENT_LENGTH) {
                        if let Some(len) = len.to_str().ok().and_then(|s| s.parse::<usize>().ok()) {
                            if len > MAX_IMAGE_BYTES {
                                return Err(format!(
                                    "Image too large ({} bytes > {} bytes)",
                                    len, MAX_IMAGE_BYTES
                                )
                                .into());
                            }
                        }
                    }

                    let body = resp.bytes()?;
                    if body.len() > MAX_IMAGE_BYTES {
                        return Err(format!(
                            "Image too large ({} bytes > {} bytes)",
                            body.len(),
                            MAX_IMAGE_BYTES
                        )
                        .into());
                    }
                    break body;
                }
                Err(e) => {
                    last_err = Some(e.to_string());
                    if attempts >= 3 {
                        return Err(format!(
                            "Failed to fetch image after {} attempts: {}",
                            attempts,
                            last_err.unwrap_or_else(|| "unknown error".into())
                        )
                        .into());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200 * attempts as u64));
                }
            }
        }
    };

    let image = match image::load_from_memory(&bytes) {
        Ok(img) => img,
        Err(e) => {
            let mut path = std::env::temp_dir();
            path.push("waifu_fetch_error.bin");
            if let Ok(mut f) = File::create(&path) {
                let _ = f.write_all(&bytes);
            }
            return Err(format!(
                "Failed to decode image: {}. Saved bytes to {}",
                e,
                path.display()
            )
            .into());
        }
    };

    print(&image, &config)?;

    Ok(())
}

fn show_image_with_path(image_path: PathBuf, config: viuer::Config) -> Result<(), Box<dyn Error>> {
    print_from_file(image_path, &config)?;

    Ok(())
}

// Removed old stdin helper; stdin is handled inline in run()
