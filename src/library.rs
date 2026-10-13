use crate::storage::Entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    History,
    Bookmarks,
}

impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::History => "История",
            Self::Bookmarks => "Закладки",
        }
    }

    fn path(self) -> &'static str {
        match self {
            Self::History => "/history",
            Self::Bookmarks => "/bookmarks",
        }
    }

    fn shortcut(self) -> &'static str {
        match self {
            Self::History => "Ctrl+H",
            Self::Bookmarks => "Ctrl+Shift+O",
        }
    }

    fn empty_message(self) -> &'static str {
        match self {
            Self::History => "Здесь пока нет посещённых страниц.",
            Self::Bookmarks => "Закладки появятся здесь, когда вы сохраните страницу.",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Self::History => "Открывайте нужную страницу, выбрав её в списке.",
            Self::Bookmarks => "Нажмите Ctrl+D на открытой странице, чтобы добавить её в закладки.",
        }
    }
}

pub fn render(page: Page, entries: &[Entry]) -> String {
    let mut html = String::with_capacity(2048 + entries.len() * 384);
    let page_title = page.title();

    html.push_str("<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    html.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">");
    html.push_str("<title>");
    html.push_str(page_title);
    html.push_str(" — Axiom Browser</title><style>");
    html.push_str(STYLES);
    html.push_str("</style></head><body><header><a class=\"brand\" href=\"/\">Axiom</a><nav aria-label=\"Библиотека\">");
    push_nav_link(&mut html, Page::History, page);
    push_nav_link(&mut html, Page::Bookmarks, page);
    html.push_str("</nav></header><main><div class=\"eyebrow\">БИБЛИОТЕКА</div><h1>");
    html.push_str(page_title);
    html.push_str("</h1><p class=\"hint\">");
    html.push_str(page.hint());
    html.push_str("</p>");

    let safe_entries: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let parsed = url::Url::parse(&entry.url).ok()?;
            matches!(parsed.scheme(), "http" | "https").then_some((entry, parsed))
        })
        .collect();

    if safe_entries.is_empty() {
        html.push_str(
            "<section class=\"empty\"><div class=\"empty-mark\" aria-hidden=\"true\">✦</div><h2>",
        );
        html.push_str(page.empty_message());
        html.push_str(
            "</h2><p>Используйте адресную строку, чтобы найти и открыть страницу.</p></section>",
        );
    } else {
        html.push_str("<ol class=\"entries\">");
        for (entry, url) in safe_entries {
            let display_title = if entry.title.trim().is_empty() {
                &entry.url
            } else {
                &entry.title
            };
            let href = url.as_str();
            html.push_str("<li><a class=\"entry\" href=\"");
            push_escaped(&mut html, href);
            html.push_str("\"><span class=\"entry-icon\" aria-hidden=\"true\">↗</span><span class=\"entry-copy\"><span class=\"entry-title\">");
            push_escaped(&mut html, display_title);
            html.push_str("</span><span class=\"entry-url\">");
            push_escaped(&mut html, href);
            html.push_str(
                "</span></span><span class=\"entry-arrow\" aria-hidden=\"true\">→</span></a></li>",
            );
        }
        html.push_str("</ol>");
    }

    html.push_str("</main><footer>Быстрый переход: <kbd>");
    html.push_str(page.shortcut());
    html.push_str("</kbd></footer></body></html>");
    html
}

fn push_nav_link(html: &mut String, target: Page, current: Page) {
    html.push_str("<a href=\"");
    html.push_str(target.path());
    html.push_str("\"");
    if target == current {
        html.push_str(" aria-current=\"page\"");
    }
    html.push('>');
    html.push_str(target.title());
    html.push_str("</a>");
}

fn push_escaped(output: &mut String, input: &str) {
    for character in input.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&quot;"),
            '\'' => output.push_str("&#39;"),
            _ => output.push(character),
        }
    }
}

const STYLES: &str = r#"
:root{color-scheme:light;--ink:#172033;--muted:#68748a;--line:#e3e8f0;--accent:#315fba;--paper:#fff;--wash:#f5f7fb}
*{box-sizing:border-box}body{margin:0;background:var(--wash);color:var(--ink);font:15px/1.5 "Segoe UI",system-ui,sans-serif}
header{height:64px;background:var(--paper);border-bottom:1px solid var(--line);display:flex;align-items:center;justify-content:space-between;padding:0 max(24px,calc((100vw - 920px)/2))}
.brand{font-size:19px;font-weight:700;letter-spacing:-.04em;color:var(--ink);text-decoration:none}nav{display:flex;gap:8px}nav a{padding:8px 13px;border-radius:9px;color:var(--muted);text-decoration:none}nav a[aria-current=page]{background:#edf2fc;color:var(--accent);font-weight:600}
main{width:min(760px,calc(100% - 40px));margin:54px auto 44px}.eyebrow{color:var(--accent);font-size:11px;font-weight:700;letter-spacing:.13em}h1{font-size:34px;line-height:1.15;letter-spacing:-.04em;margin:8px 0}.hint{color:var(--muted);margin:0 0 26px}
.entries{list-style:none;margin:0;padding:0;background:var(--paper);border:1px solid var(--line);border-radius:14px;overflow:hidden}.entries li+li{border-top:1px solid var(--line)}.entry{display:flex;align-items:center;gap:14px;padding:14px 18px;color:inherit;text-decoration:none;min-width:0}.entry:hover{background:#f9faff}.entry-icon{flex:0 0 34px;width:34px;height:34px;border-radius:10px;background:#edf2fc;color:var(--accent);display:grid;place-items:center;font-size:17px}.entry-copy{display:flex;flex-direction:column;min-width:0;gap:2px}.entry-title,.entry-url{overflow:hidden;text-overflow:ellipsis;white-space:nowrap}.entry-title{font-weight:600}.entry-url{font-size:12px;color:var(--muted)}.entry-arrow{margin-left:auto;color:#9aa5b7;font-size:18px}.empty{background:var(--paper);border:1px solid var(--line);border-radius:14px;padding:44px 28px;text-align:center}.empty-mark{margin:0 auto 12px;width:44px;height:44px;border-radius:14px;background:#edf2fc;color:var(--accent);display:grid;place-items:center;font-size:20px}.empty h2{font-size:18px;margin:0 0 6px}.empty p{margin:0;color:var(--muted)}footer{width:min(760px,calc(100% - 40px));margin:0 auto 32px;color:var(--muted);font-size:13px}kbd{font:600 12px "Segoe UI",sans-serif;border:1px solid #d6dce6;border-radius:5px;padding:3px 6px;background:#fff;color:var(--ink)}
@media(max-width:520px){header{padding:0 18px}main{margin-top:34px}h1{font-size:29px}}
"#;

#[cfg(test)]
mod tests {
    use super::{render, Page};
    use crate::storage::Entry;

    fn entry(id: i64, url: &str, title: &str) -> Entry {
        Entry {
            id,
            url: url.to_string(),
            title: title.to_string(),
            visited_at: 1_700_000_000,
        }
    }

    #[test]
    fn renders_helpful_empty_states_and_library_navigation() {
        let history = render(Page::History, &[]);
        assert!(history.contains("Здесь пока нет посещённых страниц."));
        assert!(history.contains("href=\"/bookmarks\""));
        assert!(history.contains("Ctrl+H"));

        let bookmarks = render(Page::Bookmarks, &[]);
        assert!(bookmarks.contains("Закладки появятся здесь"));
        assert!(bookmarks.contains("Ctrl+D"));
        assert!(bookmarks.contains("href=\"/history\""));
    }

    #[test]
    fn escapes_untrusted_title_and_url_and_rejects_unsafe_schemes() {
        let entries = [
            entry(
                1,
                "https://example.test/\"><script>alert(1)</script>",
                "<img src=x onerror=alert(1)> \"quoted\" & more",
            ),
            entry(2, "javascript:alert(2)", "Unsafe"),
        ];

        let html = render(Page::History, &entries);
        assert!(html.contains("&lt;img src=x onerror=alert(1)&gt; &quot;quoted&quot; &amp; more"));
        assert!(!html.contains("<img src=x"));
        assert!(!html.contains("<script"));
        assert!(!html.contains("javascript:alert(2)"));
        assert!(html.contains("href=\"https://example.test/"));
    }

    #[test]
    fn uses_url_as_the_title_when_a_saved_title_is_blank() {
        let html = render(
            Page::Bookmarks,
            &[entry(7, "https://example.test/path", "  ")],
        );
        assert!(html.contains("<span class=\"entry-title\">https://example.test/path</span>"));
    }
}
