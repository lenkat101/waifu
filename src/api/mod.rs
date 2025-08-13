pub mod danbooru;
pub mod safebooru;
use regex::Regex;

pub fn reformat_search_tags(tags: String) -> String {
    let extra_spaces = Regex::new(r"\s{2,}").unwrap();
    let delimiters = Regex::new(r"[,\s]").unwrap();

    // Collapse runs of whitespace to a single space, then replace spaces/commas with %20
    let trimmed = tags.trim();
    let collapsed = extra_spaces.replace_all(trimmed, " ");
    let search_tags = delimiters.replace_all(&collapsed, "%20");

    search_tags.to_string()
}
