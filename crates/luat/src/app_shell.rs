// Copyright 2019-2026 Maravilla Labs, operated by SOLUTAS GmbH, Switzerland
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

//! Turning a [`LuatResponse`] into a transport-ready HTTP response.
//!
//! Page responses are wrapped in the app shell (`src/app.html`), except for
//! form-action fragments and HTMX-boosted navigation, which get the bare
//! body. JSON, raw bodies, redirects and errors get the right status and
//! `content-type`. Hosts only have to map [`HttpResponse`] onto their HTTP
//! library.

use crate::request::LuatRequest;
use crate::response::{Headers, LuatResponse};

/// The default app shell, used when a project has no `src/app.html`.
pub const DEFAULT_APP_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>%luat.title%</title>
    %luat.head%
</head>
<body>
    %luat.body%
</body>
</html>
"#;

/// The HTML document pages are rendered into. `%luat.title%`,
/// `%luat.head%` and `%luat.body%` mark where the title, extra head markup
/// and page body go.
#[derive(Debug, Clone)]
pub struct AppShell {
    template: String,
}

impl Default for AppShell {
    fn default() -> Self {
        Self::new(DEFAULT_APP_HTML)
    }
}

impl AppShell {
    /// Creates a shell from an `app.html` template.
    pub fn new(template: impl Into<String>) -> Self {
        Self {
            template: template.into(),
        }
    }

    /// Fills in the placeholders. The title is HTML-escaped; `head` and
    /// `body` are inserted as markup. Substitution is a single pass, so
    /// placeholder text inside the page content is left alone.
    pub fn render(&self, title: &str, head: &str, body: &str) -> String {
        let title = html_escape(title);
        let mut out = String::with_capacity(self.template.len() + body.len() + head.len());
        let mut rest = self.template.as_str();
        while let Some(start) = rest.find("%luat.") {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let (value, len) = if tail.starts_with("%luat.title%") {
                (title.as_str(), "%luat.title%".len())
            } else if tail.starts_with("%luat.head%") {
                (head, "%luat.head%".len())
            } else if tail.starts_with("%luat.body%") {
                (body, "%luat.body%".len())
            } else {
                ("%luat.", "%luat.".len())
            };
            out.push_str(value);
            rest = &tail[len..];
        }
        out.push_str(rest);
        out
    }
}

/// How pages are finalized.
#[derive(Debug, Clone)]
pub struct ShellOptions {
    /// Markup added to `%luat.head%` on every full page (stylesheets, scripts).
    pub head: String,
    /// Title used when the page did not set `view_title`.
    pub default_title: String,
}

impl Default for ShellOptions {
    fn default() -> Self {
        Self {
            head: String::new(),
            default_title: "Luat App".to_string(),
        }
    }
}

/// A response ready to be written to the wire.
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Headers, including `content-type`.
    pub headers: Headers,
    /// Body bytes.
    pub body: Vec<u8>,
    /// True when `body` is a full HTML document built from the app shell
    /// (hosts may inject scripts, e.g. for live reload).
    pub document: bool,
}

/// Finalizes `response` for `request`.
pub fn finalize(response: LuatResponse, request: &LuatRequest, shell: &AppShell, options: &ShellOptions) -> HttpResponse {
    match response {
        LuatResponse::Html {
            status,
            mut headers,
            body,
        } => {
            let is_fragment = headers.remove("x-luat-fragment").is_some();
            let title = headers.remove("x-luat-title");
            default_content_type(&mut headers, "text/html; charset=utf-8");

            if is_fragment {
                if let Some(title) = title {
                    headers.insert("x-luat-title", title);
                }
                return plain(status, headers, body.into_bytes());
            }
            let title = title.unwrap_or_else(|| options.default_title.clone());
            if request.header("hx-boosted") == Some("true") {
                headers.insert("hx-title", title);
                return plain(status, headers, body.into_bytes());
            }
            HttpResponse {
                status,
                headers,
                body: shell.render(&title, &options.head, &body).into_bytes(),
                document: true,
            }
        }
        LuatResponse::Json {
            status,
            mut headers,
            body,
        } => {
            default_content_type(&mut headers, "application/json");
            let bytes = serde_json::to_vec(&body).unwrap_or_else(|_| b"null".to_vec());
            plain(status, headers, bytes)
        }
        LuatResponse::Body { status, headers, body } => plain(status, headers, body),
        LuatResponse::Redirect {
            status,
            location,
            mut headers,
        } => {
            headers.insert("location", location);
            plain(status, headers, Vec::new())
        }
        LuatResponse::Error {
            status,
            message,
            mut headers,
        } => {
            headers.insert("content-type", "text/html; charset=utf-8");
            plain(status, headers, error_document(status, &message).into_bytes())
        }
    }
}

fn plain(status: u16, headers: Headers, body: Vec<u8>) -> HttpResponse {
    HttpResponse {
        status,
        headers,
        body,
        document: false,
    }
}

fn default_content_type(headers: &mut Headers, content_type: &str) {
    if !headers.contains("content-type") {
        headers.insert("content-type", content_type);
    }
}

/// A minimal standalone error page, used when there is no `+error.luat`.
fn error_document(status: u16, message: &str) -> String {
    let message = html_escape(message);
    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head><meta charset=\"UTF-8\"><title>{status}</title></head>\n\
         <body style=\"font-family: system-ui, sans-serif; padding: 2rem\"><h1>{status}</h1><p>{message}</p></body>\n</html>\n"
    )
}

/// Escapes text for use in HTML content and attribute values.
pub fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(r: &HttpResponse) -> &str {
        std::str::from_utf8(&r.body).unwrap()
    }

    #[test]
    fn shell_substitutes_once_and_escapes_title() {
        let shell = AppShell::new("<title>%luat.title%</title>%luat.head%|%luat.body%|%luat.other%");
        let out = shell.render("<b>Hi</b>", "<link>", "content mentions %luat.title%");
        assert_eq!(
            out,
            "<title>&lt;b&gt;Hi&lt;/b&gt;</title><link>|content mentions %luat.title%|%luat.other%"
        );
    }

    #[test]
    fn full_page_uses_view_title_and_shell() {
        let response = LuatResponse::html(200, "<p>x</p>").with_header("x-luat-title", "Post");
        let out = finalize(response, &LuatRequest::new("/", "GET"), &AppShell::default(), &ShellOptions::default());
        assert!(out.document);
        assert!(body(&out).contains("<title>Post</title>"));
        assert!(!out.headers.contains("x-luat-title"));
        assert_eq!(out.headers.get("content-type"), Some("text/html; charset=utf-8"));
    }

    #[test]
    fn fragments_and_boosted_requests_skip_the_shell() {
        let shell = AppShell::default();
        let opts = ShellOptions::default();
        let fragment = LuatResponse::html(200, "<li>new</li>").with_header("x-luat-fragment", "1");
        let out = finalize(fragment, &LuatRequest::new("/", "POST"), &shell, &opts);
        assert_eq!((body(&out), out.document), ("<li>new</li>", false));

        let boosted_request = LuatRequest::new("/", "GET")
            .with_headers([("HX-Boosted".to_string(), "true".to_string())].into());
        let page = LuatResponse::html(200, "<main/>").with_header("x-luat-title", "T");
        let out = finalize(page, &boosted_request, &shell, &opts);
        assert_eq!((body(&out), out.headers.get("hx-title")), ("<main/>", Some("T")));
    }

    #[test]
    fn errors_keep_status_and_escape_message() {
        let out = finalize(
            LuatResponse::error(404, "<script>"),
            &LuatRequest::new("/", "GET"),
            &AppShell::default(),
            &ShellOptions::default(),
        );
        assert_eq!(out.status, 404);
        assert!(body(&out).contains("&lt;script&gt;"));
    }

    #[test]
    fn redirects_keep_cookies() {
        let mut response = LuatResponse::redirect("/home");
        response.headers_mut().append("set-cookie", "a=1");
        let out = finalize(response, &LuatRequest::new("/", "POST"), &AppShell::default(), &ShellOptions::default());
        assert_eq!(out.status, 302);
        assert_eq!(out.headers.get("location"), Some("/home"));
        assert_eq!(out.headers.get("set-cookie"), Some("a=1"));
    }
}
