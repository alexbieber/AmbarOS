//! HTTP fingerprinting: status, title, server, technologies and missing security headers.

use std::time::Duration;

use anyhow::Result;
use reqwest::header::HeaderMap;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HttpInfo {
    pub url: String,
    pub final_url: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    pub technologies: Vec<String>,
    pub missing_security_headers: Vec<String>,
    /// Cookies sent without Secure or HttpOnly.
    pub weak_cookies: Vec<String>,
    pub bytes: usize,
}

const SECURITY_HEADERS: &[(&str, bool)] = &[
    // (header, only meaningful over HTTPS)
    ("strict-transport-security", true),
    ("content-security-policy", false),
    ("x-content-type-options", false),
    ("x-frame-options", false),
    ("referrer-policy", false),
    ("permissions-policy", false),
];

pub fn client(timeout: Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::limited(5))
        // Recon looks at misconfigured servers too; the TLS command reports certificate problems.
        .danger_accept_invalid_certs(true)
        .user_agent(concat!("ambar-recon/", env!("CARGO_PKG_VERSION")))
        .build()?)
}

pub async fn fingerprint(client: &reqwest::Client, url: &str) -> Result<HttpInfo> {
    let resp = client.get(url).send().await?;
    let final_url = resp.url().to_string();
    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let body = resp.text().await.unwrap_or_default();
    let https = final_url.starts_with("https://");
    Ok(HttpInfo {
        url: url.to_string(),
        final_url,
        status,
        title: title(&body),
        server: header(&headers, "server"),
        technologies: technologies(&headers, &body),
        missing_security_headers: SECURITY_HEADERS
            .iter()
            .filter(|(h, tls_only)| (https || !tls_only) && !headers.contains_key(*h))
            .map(|(h, _)| h.to_string())
            .collect(),
        weak_cookies: headers
            .get_all("set-cookie")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .filter(|c| {
                let l = c.to_ascii_lowercase();
                !l.contains("httponly") || (https && !l.contains("secure"))
            })
            .map(|c| c.split('=').next().unwrap_or(c).trim().to_string())
            .collect(),
        bytes: body.len(),
    })
}

fn header(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

pub fn title(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let end = lower[open_end..].find("</title>")? + open_end;
    let t = body
        .get(open_end..end)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    (!t.is_empty()).then(|| t.chars().take(120).collect())
}

/// Technologies from headers and page content. Each rule is cheap and specific.
pub fn technologies(h: &HeaderMap, body: &str) -> Vec<String> {
    let hv = |n: &str| header(h, n).unwrap_or_default().to_ascii_lowercase();
    let b = body.to_ascii_lowercase();
    let server = hv("server");
    let powered = hv("x-powered-by");
    let cookies: String = h
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect::<Vec<_>>()
        .join(";")
        .to_ascii_lowercase();
    let mut out = Vec::new();
    let mut add = |cond: bool, name: &str| {
        if cond && !out.iter().any(|x: &String| x == name) {
            out.push(name.to_string());
        }
    };
    add(server.contains("nginx"), "nginx");
    add(server.contains("apache"), "Apache httpd");
    add(server.contains("microsoft-iis"), "IIS");
    add(server.contains("caddy"), "Caddy");
    add(
        server.contains("cloudflare") || h.contains_key("cf-ray"),
        "Cloudflare",
    );
    add(
        h.contains_key("x-vercel-id") || server.contains("vercel"),
        "Vercel",
    );
    add(
        h.contains_key("x-amz-cf-id") || server.contains("cloudfront"),
        "Amazon CloudFront",
    );
    add(
        powered.contains("php") || cookies.contains("phpsessid"),
        "PHP",
    );
    add(powered.contains("express"), "Express");
    add(
        powered.contains("asp.net") || h.contains_key("x-aspnet-version"),
        "ASP.NET",
    );
    add(
        powered.contains("next.js") || b.contains("__next_data__") || b.contains("/_next/static"),
        "Next.js",
    );
    add(
        b.contains("wp-content/") || b.contains("wp-includes/"),
        "WordPress",
    );
    add(
        b.contains("drupal-settings-json") || hv("x-generator").contains("drupal"),
        "Drupal",
    );
    add(b.contains("/media/jui/") || b.contains("joomla"), "Joomla");
    add(b.contains("cdn.shopify.com"), "Shopify");
    add(
        b.contains("data-reactroot") || b.contains("react-dom"),
        "React",
    );
    add(b.contains("ng-version="), "Angular");
    add(b.contains("data-v-app") || b.contains("__vue__"), "Vue.js");
    add(cookies.contains("jsessionid"), "Java servlet");
    add(cookies.contains("laravel_session"), "Laravel");
    add(
        cookies.contains("csrftoken") && b.contains("csrfmiddlewaretoken"),
        "Django",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(
            title("<html><TITLE>\n  Hello   World </title>"),
            Some("Hello World".into())
        );
        assert_eq!(title("<title data-x=1>A</title>"), Some("A".into()));
        assert_eq!(title("<title></title>"), None);
        assert_eq!(title("no title"), None);
    }
}
