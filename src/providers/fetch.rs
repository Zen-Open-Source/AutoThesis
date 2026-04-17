use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, USER_AGENT};
use scraper::{Html, Selector};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use std::time::Duration;
use url::Url;

#[derive(Debug, Clone)]
pub struct FetchedPage {
    pub url: String,
    pub title: Option<String>,
    pub domain: Option<String>,
    pub text: String,
}

#[async_trait]
pub trait WebFetcher: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<FetchedPage>;
}

#[derive(Clone)]
pub struct ReqwestWebFetcher {
    client: reqwest::Client,
}

impl ReqwestWebFetcher {
    pub fn new() -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(
            USER_AGENT,
            HeaderValue::from_static("AutoThesis/0.1 (+https://autothesis.finance)"),
        );
        headers.insert(
            ACCEPT,
            HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to build reqwest client")?;

        Ok(Self { client })
    }
}

#[async_trait]
impl WebFetcher for ReqwestWebFetcher {
    async fn fetch(&self, url: &str) -> Result<FetchedPage> {
        validate_external_url(url)?;
        let response = self.client.get(url).send().await?.error_for_status()?;
        let body = response.text().await?;
        let document = Html::parse_document(&body);
        let title_selector = Selector::parse("title").ok();
        let title = title_selector
            .as_ref()
            .and_then(|selector| document.select(selector).next())
            .map(|node| collapse_whitespace(&node.text().collect::<Vec<_>>().join(" ")))
            .filter(|value| !value.is_empty());

        let text =
            collapse_whitespace(&document.root_element().text().collect::<Vec<_>>().join(" "));
        let domain = Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.domain().map(|value| value.to_string()));

        Ok(FetchedPage {
            url: url.to_string(),
            title,
            domain,
            text: truncate_chars(&text, 16_000),
        })
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// Validate an external URL before issuing a server-side fetch.
///
/// Blocks:
/// - non-http(s) schemes
/// - loopback / private / link-local / cloud metadata addresses
/// - literal IP hosts that fall into disallowed ranges
pub(crate) fn validate_external_url(url: &str) -> Result<()> {
    let parsed = Url::parse(url).with_context(|| format!("invalid url: {url}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => bail!("disallowed url scheme: {other}"),
    }

    let host = parsed
        .host()
        .ok_or_else(|| anyhow!("url is missing a host: {url}"))?;

    match host {
        url::Host::Ipv4(v4) => {
            if is_disallowed_v4(&v4) {
                bail!("disallowed host address: {v4}");
            }
        }
        url::Host::Ipv6(v6) => {
            if is_disallowed_v6(&v6) {
                bail!("disallowed host address: {v6}");
            }
        }
        url::Host::Domain(domain) => {
            let port = parsed.port_or_known_default().unwrap_or(80);
            if let Ok(iter) = (domain, port).to_socket_addrs() {
                for socket in iter {
                    if is_disallowed_ip(&socket.ip()) {
                        bail!("disallowed host address for {domain}: {}", socket.ip());
                    }
                }
            }
        }
    }

    Ok(())
}

fn is_disallowed_ip(addr: &IpAddr) -> bool {
    match addr {
        IpAddr::V4(v4) => is_disallowed_v4(v4),
        IpAddr::V6(v6) => is_disallowed_v6(v6),
    }
}

fn is_disallowed_v4(addr: &Ipv4Addr) -> bool {
    // Loopback, private, link-local, documentation, broadcast, multicast, unspecified.
    addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_broadcast()
        || addr.is_multicast()
        || addr.is_documentation()
        || addr.is_unspecified()
        // AWS/GCP/Azure metadata endpoint (169.254.169.254) is covered by link_local.
        // Carrier-grade NAT 100.64.0.0/10.
        || (addr.octets()[0] == 100 && (64..=127).contains(&addr.octets()[1]))
}

fn is_disallowed_v6(addr: &Ipv6Addr) -> bool {
    addr.is_loopback()
        || addr.is_unspecified()
        || addr.is_multicast()
        // unique-local fc00::/7
        || (addr.segments()[0] & 0xfe00) == 0xfc00
        // link-local fe80::/10
        || (addr.segments()[0] & 0xffc0) == 0xfe80
        // IPv4-mapped - check underlying v4
        || addr
            .to_ipv4_mapped()
            .map(|v4| is_disallowed_v4(&v4))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_http_schemes() {
        assert!(validate_external_url("file:///etc/passwd").is_err());
        assert!(validate_external_url("ftp://example.com").is_err());
        assert!(validate_external_url("gopher://example.com").is_err());
    }

    #[test]
    fn rejects_loopback_and_private_ips() {
        assert!(validate_external_url("http://127.0.0.1/").is_err());
        assert!(validate_external_url("http://10.0.0.1/").is_err());
        assert!(validate_external_url("http://192.168.1.1/").is_err());
        assert!(validate_external_url("http://169.254.169.254/").is_err());
        assert!(validate_external_url("http://[::1]/").is_err());
    }

    #[test]
    fn accepts_public_hosts() {
        assert!(validate_external_url("https://example.com/path").is_ok());
        assert!(validate_external_url("http://8.8.8.8/").is_ok());
    }
}
