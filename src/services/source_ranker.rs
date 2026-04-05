use crate::providers::search::SearchResultItem;
use url::Url;

#[derive(Debug, Clone)]
pub struct RankedSearchResult {
    pub title: Option<String>,
    pub url: String,
    pub snippet: Option<String>,
    pub rank_score: f64,
    pub source_type: String,
}

pub fn rank_search_result(result: SearchResultItem) -> RankedSearchResult {
    let domain = Url::parse(&result.url)
        .ok()
        .and_then(|parsed| parsed.domain().map(|value| value.to_string()))
        .unwrap_or_default();
    let source_type = classify_source(&domain, result.title.as_deref().unwrap_or_default());
    let mut rank_score = result.score.unwrap_or(0.0);
    rank_score += match source_type.as_str() {
        "sec" => 5.0,
        "ir" => 4.0,
        "transcript" => 3.5,
        "press" => 3.0,
        "media" => 2.0,
        _ => 1.0,
    };

    RankedSearchResult {
        title: result.title,
        url: result.url,
        snippet: result.snippet,
        rank_score,
        source_type,
    }
}

pub fn classify_source(domain: &str, title: &str) -> String {
    let combined = format!("{} {}", domain.to_lowercase(), title.to_lowercase());
    if combined.contains("sec.gov") || combined.contains("10-k") || combined.contains("10-q") {
        "sec".to_string()
    } else if combined.contains("investor")
        || combined.contains("shareholder")
        || combined.contains("annual report")
    {
        "ir".to_string()
    } else if combined.contains("transcript") || combined.contains("earnings call") {
        "transcript".to_string()
    } else if combined.contains("press release")
        || combined.contains("news release")
        || combined.contains("businesswire")
    {
        "press".to_string()
    } else if combined.contains("bloomberg")
        || combined.contains("reuters")
        || combined.contains("wsj")
        || combined.contains("ft.com")
    {
        "media".to_string()
    } else {
        "other".to_string()
    }
}
