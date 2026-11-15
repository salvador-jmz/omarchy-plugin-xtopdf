use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Article {
    pub title: String,
    pub byline: String,
    pub source: String,
    pub content: Vec<Block>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    pub kind: String,
    #[serde(default)]
    pub level: u8,
    #[serde(default)]
    pub ordered: bool,
    #[serde(default)]
    pub runs: Vec<Inline>,
    #[serde(default)]
    pub text: String,
    /// Image blocks: the pbs.twimg.com URL seen by the extractor.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub src: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub alt: String,
    /// Image blocks: a `data:image/...;base64,` copy filled in by the helper.
    /// The print renderer blocks all network requests, so remote URLs are never used.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data: String,
}

/// Per-image and per-document caps for embedded images (decoded bytes).
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGES_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_IMAGES: usize = 100;

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Inline {
    Text { text: String },
    Br,
    Strong { children: Vec<Inline> },
    Em { children: Vec<Inline> },
    S { children: Vec<Inline> },
    Code { children: Vec<Inline> },
    Sup { children: Vec<Inline> },
    Sub { children: Vec<Inline> },
    Link { href: String, children: Vec<Inline> },
}

pub fn article_url(input: &str) -> Result<String, String> {
    let u = Url::parse(input.trim()).map_err(|_| "Enter a complete https://x.com article URL.")?;
    if input.len() > 2048
        || u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
        || !matches!(
            u.host_str(),
            Some("x.com" | "www.x.com" | "twitter.com" | "www.twitter.com")
        )
    {
        return Err("Only HTTPS X/Twitter article URLs are supported.".into());
    }
    let parts: Vec<_> = u.path().trim_end_matches('/').split('/').skip(1).collect();
    let numeric = |s: &str| !s.is_empty() && s.len() <= 24 && s.bytes().all(|b| b.is_ascii_digit());
    let supported = match parts.as_slice() {
        [name, "status", id] => {
            !name.is_empty()
                && name.len() <= 15
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                && numeric(id)
        }
        ["i", "article", id] => numeric(id),
        _ => false,
    };
    if !supported {
        return Err("Use the article’s /name/status/123 or /i/article/123 URL.".into());
    }
    Ok(format!("https://x.com{}", u.path().trim_end_matches('/')))
}

/// Validates an article image URL and returns the copy to download: HTTPS
/// pbs.twimg.com only, with X's size hint raised so the PDF is not blurry.
pub fn image_url(src: &str) -> Option<String> {
    let mut u = Url::parse(src).ok()?;
    if src.len() > 2048
        || u.scheme() != "https"
        || u.host_str() != Some("pbs.twimg.com")
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some()
    {
        return None;
    }
    u.set_fragment(None);
    if u.path().starts_with("/media/") && u.query_pairs().any(|(k, _)| k == "name") {
        let pairs: Vec<(String, String)> = u
            .query_pairs()
            .map(|(k, v)| {
                let v = if k == "name" {
                    "large".into()
                } else {
                    v.into_owned()
                };
                (k.into_owned(), v)
            })
            .collect();
        u.query_pairs_mut().clear().extend_pairs(pairs);
    }
    Some(u.into())
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// Encodes downloaded bytes as a data URI. The type comes from the bytes
/// themselves (never from the server), and only raster formats are accepted.
pub fn image_data_uri(bytes: &[u8]) -> Option<String> {
    use base64::Engine;
    if bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    let mime = image_mime(bytes)?;
    Some(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// Decodes a data URI produced by `image_data_uri` (or by the browser) and
/// re-checks it. Returns the decoded length and a canonical data URI.
pub fn checked_data_uri(data: &str) -> Option<(usize, String)> {
    use base64::Engine;
    let (header, payload) = data.split_once(',')?;
    let declared = header.strip_prefix("data:")?.strip_suffix(";base64")?;
    if payload.len() > MAX_IMAGE_BYTES / 3 * 4 + 4 {
        return None;
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()?;
    let uri = image_data_uri(&bytes)?;
    // A declared type that disagrees with the bytes is rejected outright.
    if !declared.eq_ignore_ascii_case(image_mime(&bytes)?) {
        return None;
    }
    Some((bytes.len(), uri))
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn render_inline(items: &[Inline], depth: usize) -> Result<String, String> {
    if depth > 24 {
        return Err("Article formatting is too deeply nested.".into());
    }
    let mut out = String::new();
    for item in items {
        let (tag, children) = match item {
            Inline::Text { text } => {
                out.push_str(&escape(text));
                continue;
            }
            Inline::Br => {
                out.push_str("<br>");
                continue;
            }
            Inline::Strong { children } => ("strong", children),
            Inline::Em { children } => ("em", children),
            Inline::S { children } => ("s", children),
            Inline::Code { children } => ("code", children),
            Inline::Sup { children } => ("sup", children),
            Inline::Sub { children } => ("sub", children),
            Inline::Link { href, children } => {
                let inner = render_inline(children, depth + 1)?;
                let valid = Url::parse(href).ok().filter(|u| {
                    matches!(u.scheme(), "https" | "http")
                        && u.username().is_empty()
                        && u.password().is_none()
                });
                if let Some(u) = valid {
                    out.push_str(&format!(
                        "<a href=\"{}\" rel=\"noreferrer\">{inner}</a>",
                        escape(u.as_str())
                    ));
                } else {
                    out.push_str(&inner);
                }
                continue;
            }
        };
        out.push_str(&format!(
            "<{tag}>{}</{tag}>",
            render_inline(children, depth + 1)?
        ));
    }
    Ok(out)
}

impl Article {
    pub fn html(&self, paper: &str) -> Result<String, String> {
        let source = article_url(&self.source)?;
        if self.content.is_empty()
            || self.content.len() > 10000
            || self.title.len() > 4096
            || self.byline.len() > 4096
        {
            return Err("Article is empty or exceeds the supported size.".into());
        }
        if !matches!(paper, "Letter" | "A4") {
            return Err("Choose Letter or A4 paper.".into());
        }
        if self.content.iter().filter(|b| b.kind == "image").count() > MAX_IMAGES {
            return Err("Article has more images than the supported limit.".into());
        }
        let mut content = String::new();
        let mut list = "";
        let mut image_bytes = 0usize;
        for b in &self.content {
            let next = if b.kind == "list-item" {
                if b.ordered {
                    "ol"
                } else {
                    "ul"
                }
            } else {
                ""
            };
            if list != next {
                if !list.is_empty() {
                    content.push_str(&format!("</{list}>"));
                }
                if !next.is_empty() {
                    content.push_str(&format!("<{next}>"));
                }
                list = next;
            }
            if b.kind == "image" {
                let alt = escape(&b.alt);
                match checked_data_uri(&b.data) {
                    Some((len, uri)) if image_bytes + len <= MAX_IMAGES_BYTES => {
                        image_bytes += len;
                        content.push_str(&format!("<figure><img src=\"{uri}\" alt=\"{alt}\">"));
                        if !alt.is_empty() {
                            content.push_str(&format!("<figcaption>{alt}</figcaption>"));
                        }
                        content.push_str("</figure>\n");
                    }
                    _ => content.push_str(&format!(
                        "<figure class=\"missing\"><figcaption>[Image not captured{}{alt}]</figcaption></figure>\n",
                        if alt.is_empty() { "" } else { ": " }
                    )),
                }
                continue;
            }
            let tag = match b.kind.as_str() {
                "paragraph" => "p".into(),
                "list-item" => "li".into(),
                "quote" => "blockquote".into(),
                "code" => "pre".into(),
                "heading" => format!("h{}", b.level.clamp(2, 6)),
                _ => return Err("Unsupported article block.".into()),
            };
            let text = if b.kind == "code" {
                escape(&b.text)
            } else {
                render_inline(&b.runs, 0)?
            };
            content.push_str(&format!("<{tag}>{text}</{tag}>\n"));
        }
        if !list.is_empty() {
            content.push_str(&format!("</{list}>"));
        }
        // No raw article HTML enters this document. CSP is a second boundary.
        Ok(format!("<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\"><title>{}</title><style>{}\n@page {{ size: {paper}; }}\n</style></head><body><main id=\"document\"><header><h1>{}</h1><p id=\"byline\">{}</p></header><article id=\"content\">{content}</article><footer><a href=\"{}\">{}</a></footer></main></body></html>", escape(&self.title), include_str!("../assets/print.css"), escape(&self.title), escape(&self.byline), escape(&source), escape(&source)))
    }

    pub fn preview(&self) -> String {
        fn plain(items: &[Inline], out: &mut String) {
            for i in items {
                match i {
                    Inline::Text { text } => out.push_str(text),
                    Inline::Br => out.push('\n'),
                    Inline::Strong { children }
                    | Inline::Em { children }
                    | Inline::S { children }
                    | Inline::Code { children }
                    | Inline::Sup { children }
                    | Inline::Sub { children }
                    | Inline::Link { children, .. } => plain(children, out),
                }
            }
        }
        let mut s = String::new();
        for b in &self.content {
            if b.kind == "image" {
                s.push_str(match (b.data.is_empty(), b.alt.is_empty()) {
                    (true, _) => "[Image not captured",
                    (false, true) => "[Image",
                    (false, false) => "[Image: ",
                });
                s.push_str(&b.alt);
                s.push_str("]\n\n");
                continue;
            }
            plain(&b.runs, &mut s);
            s.push_str(&b.text);
            s.push_str("\n\n");
        }
        s.chars().take(16000).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn url_boundaries() {
        assert_eq!(
            article_url("https://twitter.com/tom/status/123?s=20").unwrap(),
            "https://x.com/tom/status/123"
        );
        assert!(article_url("https://x.com/i/article/123").is_ok());
        for s in [
            "file:///etc/passwd",
            "https://x.com.evil.test/a/status/1",
            "https://me@x.com/a/status/1",
            "https://x.com:444/a/status/1",
            "http://x.com/a/status/1",
            "https://x.com/home",
            "--disable-web-security",
            "https://x.com/a/status/%31",
        ] {
            assert!(article_url(s).is_err(), "{s}");
        }
    }
    #[test]
    fn html_is_data_and_lists_keep_order() {
        let a: Article =
            serde_json::from_str(include_str!("../demo/fixtures/article.json")).unwrap();
        let h = a.html("A4").unwrap();
        assert!(h.contains("<ol><li>"));
        assert!(h.contains("&lt;script&gt;"));
        assert!(!h.contains("href=\"javascript:"));
        assert!(!h.contains("<script>"));
        assert!(h.find("First paragraph").unwrap() < h.find("let answer").unwrap());
        assert!(h.find("let answer").unwrap() < h.find("After the code").unwrap());
        assert_eq!(h.matches("Repeated on purpose").count(), 2);
        assert!(a.html("A3; background:url(https://evil.test)").is_err());
    }
    #[test]
    fn images_are_embedded_in_place() {
        let a: Article =
            serde_json::from_str(include_str!("../demo/fixtures/article.json")).unwrap();
        let h = a.html("Letter").unwrap();
        assert!(h.contains("img-src data:;"));
        assert!(h.contains("<figure><img src=\"data:image/png;base64,iVBORw0KGgo"));
        assert!(h.contains("<figcaption>A fictional gradient figure</figcaption>"));
        assert!(
            !h.contains("pbs.twimg.com"),
            "remote image URLs never reach the renderer"
        );
        assert!(h.find("Links remain clickable").unwrap() < h.find("<figure>").unwrap());
        assert!(h.find("<figure>").unwrap() < h.find("let answer").unwrap());
        assert!(a.preview().contains("[Image: A fictional gradient figure]"));
    }
    #[test]
    fn image_data_is_verified() {
        let png = b"\x89PNG\r\n\x1a\n0000";
        let uri = image_data_uri(png).unwrap();
        assert!(uri.starts_with("data:image/png;base64,"));
        assert_eq!(checked_data_uri(&uri).unwrap().0, png.len());
        for bad in [
            "",
            "javascript:alert(1)",
            "https://pbs.twimg.com/media/a.jpg",
            "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=",
            "data:text/html;base64,PGgxPmhpPC9oMT4=",
            "data:image/jpeg;base64,iVBORw0KGgowMDAw",
            "data:image/png,iVBORw0KGgowMDAw",
            "data:image/png;base64,not base64!",
        ] {
            assert!(checked_data_uri(bad).is_none(), "{bad}");
        }
        assert!(image_data_uri(b"<svg onload=alert(1)>").is_none());
        let mut block: Article =
            serde_json::from_str(include_str!("../demo/fixtures/article.json")).unwrap();
        for b in block.content.iter_mut().filter(|b| b.kind == "image") {
            b.data = "data:image/svg+xml;base64,PHN2Zz48L3N2Zz4=".into();
            b.alt = "<b>cap</b>".into();
        }
        let h = block.html("A4").unwrap();
        assert!(!h.contains("<img"));
        assert!(h.contains("[Image not captured: &lt;b&gt;cap&lt;/b&gt;]"));
    }
    #[test]
    fn image_urls_are_x_media_only() {
        assert_eq!(
            image_url("https://pbs.twimg.com/media/Gx1?format=jpg&name=small").unwrap(),
            "https://pbs.twimg.com/media/Gx1?format=jpg&name=large"
        );
        assert!(image_url("https://pbs.twimg.com/media/Gx1.jpg").is_some());
        for s in [
            "http://pbs.twimg.com/media/a.jpg",
            "https://pbs.twimg.com.evil.test/media/a.jpg",
            "https://evil.test/media/a.jpg",
            "https://me@pbs.twimg.com/media/a.jpg",
            "https://pbs.twimg.com:8443/media/a.jpg",
            "file:///etc/passwd",
            "data:image/png;base64,AAAA",
        ] {
            assert!(image_url(s).is_none(), "{s}");
        }
    }
}
