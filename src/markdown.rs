use ammonia::Builder;
use pulldown_cmark::{html, Options, Parser};
use std::sync::OnceLock;

/// Pre-configured ammonia sanitizer + pulldown-cmark option set. Cached in
/// a `OnceLock` so every render reuses the same configured `Builder` rather
/// than rebuilding the allowed-tag and allowed-attribute sets on each call.
static SANITIZER: OnceLock<Builder<'static>> = OnceLock::new();
static MARKDOWN_OPTIONS: OnceLock<Options> = OnceLock::new();

fn sanitizer() -> &'static Builder<'static> {
    SANITIZER.get_or_init(Builder::default)
}

fn markdown_options() -> Options {
    *MARKDOWN_OPTIONS.get_or_init(|| {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_FOOTNOTES);
        options
    })
}

pub fn render_markdown(markdown: &str) -> String {
    let parser = Parser::new_ext(markdown, markdown_options());
    let mut html_output = String::new();
    html::push_html(&mut html_output, parser);
    sanitizer().clean(&html_output).to_string()
}

/// Escape a string for safe insertion into HTML text / attribute context.
/// Single-pass, allocates once with a best-effort capacity estimate. Mirrors
/// the old five-chained-`.replace()` behaviour but avoids building four
/// throw-away intermediate strings per call.
pub fn escape_html(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_replaces_specials() {
        assert_eq!(
            escape_html("<a href=\"x\">O'Reilly & Sons</a>"),
            "&lt;a href=&quot;x&quot;&gt;O&#39;Reilly &amp; Sons&lt;/a&gt;"
        );
    }

    #[test]
    fn escape_html_passes_through_plain_text() {
        let s = "Hello, world!";
        assert_eq!(escape_html(s), s);
    }

    #[test]
    fn render_markdown_sanitizes_script_tags() {
        let raw = "Hello <script>alert('x')</script> world";
        let out = render_markdown(raw);
        assert!(!out.contains("<script"), "rendered output: {out}");
    }
}
