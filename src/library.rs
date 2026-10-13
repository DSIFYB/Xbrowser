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

#[cfg(test)]
pub fn render(page: Page, entries: &[Entry]) -> String {
    render_manager(page, entries, "", None, None)
}

pub fn render_manager(
    page: Page,
    entries: &[Entry],
    token: &str,
    edit: Option<i64>,
    delete: Option<i64>,
) -> String {
    let can_mutate = !token.is_empty();
    let mut html = String::with_capacity(2048 + entries.len() * 384);
    let page_title = page.title();

    html.push_str("<!doctype html><html lang=\"ru\"><head><meta charset=\"utf-8\">");
    html.push_str("<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">");
    html.push_str("<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'self'\">");
    html.push_str("<title>");
    html.push_str(page_title);
    html.push_str(" — Axiom Browser</title><style>");
    html.push_str(include_str!("../assets/internal.css"));
    html.push_str("</style></head><body><header><a class=\"brand\" href=\"/\">Axiom</a><nav aria-label=\"Библиотека\">");
    push_nav_link(&mut html, Page::History, page);
    push_nav_link(&mut html, Page::Bookmarks, page);
    html.push_str("<a href=\"/settings\">Настройки</a></nav></header><main><div class=\"eyebrow\">БИБЛИОТЕКА</div><h1>");
    html.push_str(page_title);
    html.push_str("</h1><p class=\"hint\">");
    html.push_str(page.hint());
    html.push_str("</p><form class=\"search\" action=\"");
    html.push_str(page.path());
    html.push_str("\" method=\"get\"><label class=\"sr-only\" for=\"library-query\">Поиск</label><input id=\"library-query\" type=\"search\" name=\"q\" placeholder=\"Поиск по названию и адресу\"><button type=\"submit\">Найти</button></form>");

    let safe_entries: Vec<_> = entries
        .iter()
        .filter_map(|entry| {
            let parsed = url::Url::parse(&entry.url).ok()?;
            matches!(parsed.scheme(), "http" | "https").then_some((entry, parsed))
        })
        .collect();

    html.push_str("<p class=\"entry-count\">Записей: ");
    html.push_str(&safe_entries.len().to_string());
    html.push_str("</p>");

    if page == Page::History && can_mutate && !safe_entries.is_empty() {
        push_clear_history_form(&mut html, token);
    }

    if page == Page::Bookmarks && can_mutate && !safe_entries.is_empty() {
        let action = query_action("/open-bookmarks", &[("token", token)]);
        html.push_str("<a class=\"open-all\" href=\"");
        push_escaped(&mut html, &action);
        html.push_str("\">Открыть все закладки</a>");
    }

    if page == Page::Bookmarks && can_mutate {
        if let Some(entry) = safe_entries
            .iter()
            .find_map(|(entry, _)| (Some(entry.id) == edit).then_some(*entry))
        {
            push_edit_form(&mut html, entry, token);
        }
        if let Some(entry) = safe_entries
            .iter()
            .find_map(|(entry, _)| (Some(entry.id) == delete).then_some(*entry))
        {
            push_confirm_delete_form(&mut html, entry, token);
        }
        push_add_form(&mut html, token);
    }

    if safe_entries.is_empty() {
        html.push_str(
            "<section class=\"empty\"><div class=\"empty-mark\" aria-hidden=\"true\">↗</div><h2>",
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
            let title_href = if page == Page::Bookmarks && can_mutate {
                query_action("/open", &[("token", token), ("url", href)])
            } else {
                href.to_string()
            };
            html.push_str("<li><div class=\"entry\"><span class=\"entry-icon\" aria-hidden=\"true\">↗</span><span class=\"entry-copy\"><a class=\"entry-title\" href=\"");
            push_escaped(&mut html, &title_href);
            html.push_str("\">");
            push_escaped(&mut html, display_title);
            html.push_str("</a><span class=\"entry-url\">");
            push_escaped(&mut html, href);
            html.push_str("</span></span><span class=\"entry-actions\">");
            if page == Page::Bookmarks {
                html.push_str("<a class=\"row-action\" href=\"");
                push_escaped(&mut html, href);
                html.push_str("\">В этой вкладке</a>");
                if can_mutate {
                    html.push_str("<a class=\"row-action\" href=\"/bookmarks?edit=");
                    html.push_str(&entry.id.to_string());
                    html.push_str("\">Изменить</a>");
                    html.push_str("<a class=\"row-action danger\" href=\"/bookmarks?delete=");
                    html.push_str(&entry.id.to_string());
                    html.push_str("\">Удалить</a>");
                }
            }
            html.push_str("</span></div></li>");
        }
        html.push_str("</ol>");
    }

    html.push_str("</main><footer>Быстрый переход: <kbd>");
    html.push_str(page.shortcut());
    html.push_str("</kbd></footer></body></html>");
    html
}

fn push_edit_form(html: &mut String, entry: &Entry, token: &str) {
    html.push_str("<form class=\"editor\" action=\"/bookmarks/save\" method=\"post\"><h2>Изменить закладку</h2>");
    push_hidden(html, "token", token);
    push_hidden(html, "id", &entry.id.to_string());
    html.push_str("<label>Название<input name=\"title\" value=\"");
    push_escaped(html, &entry.title);
    html.push_str("\" required></label><label>Адрес<input name=\"url\" type=\"url\" value=\"");
    push_escaped(html, &entry.url);
    html.push_str("\" required></label><div class=\"editor-actions\"><button type=\"submit\">Сохранить</button><a href=\"/bookmarks\">Отмена</a></div></form>");
}

fn push_add_form(html: &mut String, token: &str) {
    html.push_str("<details class=\"add-panel\"><summary>Добавить закладку</summary><form class=\"editor add-bookmark\" action=\"/bookmarks/add\" method=\"post\"><h2>Новая закладка</h2>");
    push_hidden(html, "token", token);
    html.push_str("<label>Название <span class=\"optional\">необязательно</span><input name=\"title\" placeholder=\"Название страницы\"></label><label>Адрес<input name=\"url\" type=\"url\" placeholder=\"https://example.com\" required></label><div class=\"editor-actions\"><button type=\"submit\">Добавить закладку</button></div></form></details>");
}

fn push_confirm_delete_form(html: &mut String, entry: &Entry, token: &str) {
    let title = if entry.title.trim().is_empty() {
        &entry.url
    } else {
        &entry.title
    };
    html.push_str("<section class=\"confirm-delete\"><h2>Удалить закладку?</h2><p>");
    push_escaped(html, title);
    html.push_str(
        "</p><form class=\"editor-actions\" action=\"/bookmarks/delete\" method=\"post\">",
    );
    push_hidden(html, "token", token);
    push_hidden(html, "id", &entry.id.to_string());
    html.push_str("<button class=\"danger\" type=\"submit\">Удалить</button><a href=\"/bookmarks\">Отмена</a></form></section>");
}

fn push_clear_history_form(html: &mut String, token: &str) {
    html.push_str("<form class=\"clear-history\" action=\"/history/clear\" method=\"post\">");
    push_hidden(html, "token", token);
    html.push_str("<button class=\"danger\" type=\"submit\">Очистить историю</button></form>");
}

fn push_hidden(html: &mut String, name: &str, value: &str) {
    html.push_str("<input type=\"hidden\" name=\"");
    push_escaped(html, name);
    html.push_str("\" value=\"");
    push_escaped(html, value);
    html.push_str("\">");
}

fn query_action(path: &str, pairs: &[(&str, &str)]) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    for (name, value) in pairs {
        query.append_pair(name, value);
    }
    format!("{path}?{}", query.finish())
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

#[cfg(test)]
mod tests {
    use super::{render, render_manager, Page};
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
        assert!(html.contains(">https://example.test/path</a>"));
    }

    #[test]
    fn bookmark_actions_encode_token_and_url_and_use_post_for_mutations() {
        let entries = [entry(
            7,
            "https://example.test/?q=one&x=two",
            "A \"quoted\" <title>",
        )];
        let token = "t&\"<script>";
        let html = render_manager(Page::Bookmarks, &entries, token, None, None);

        assert!(html.contains("href=\"/open?token=t%26%22%3Cscript%3E&amp;url=https%3A%2F%2Fexample.test%2F%3Fq%3Done%26x%3Dtwo\""));
        assert!(html.contains("href=\"/open-bookmarks?token=t%26%22%3Cscript%3E\""));
        assert!(html.contains("value=\"t&amp;&quot;&lt;script&gt;\""));
        assert!(html.contains("action=\"/bookmarks/add\" method=\"post\""));
        assert!(html.contains("name=\"title\" placeholder=\"Название страницы\""));
        assert!(html.contains("href=\"/bookmarks?delete=7\">Удалить</a>"));
        assert!(!html.contains("action=\"/bookmarks/delete\""));
        assert!(html.contains("href=\"https://example.test/?q=one&amp;x=two\">В этой вкладке</a>"));
        assert!(!html.contains("<title>\"quoted\""));
        assert!(!html.contains("<script>"));
    }

    #[test]
    fn edit_form_escapes_saved_values_and_legacy_render_has_no_mutation_controls() {
        let entries = [entry(
            8,
            "https://example.test/path?a=1&b=2",
            "<img src=x onerror=alert(1)> \"title\"",
        )];
        let html = render_manager(Page::Bookmarks, &entries, "token", Some(8), None);
        assert!(html.contains("action=\"/bookmarks/save\" method=\"post\""));
        assert!(html.contains(
            "name=\"title\" value=\"&lt;img src=x onerror=alert(1)&gt; &quot;title&quot;\""
        ));
        assert!(html
            .contains("name=\"url\" type=\"url\" value=\"https://example.test/path?a=1&amp;b=2\""));
        assert!(html.contains("href=\"/bookmarks\">Отмена</a>"));

        let legacy = render(Page::Bookmarks, &entries);
        assert!(!legacy.contains("method=\"post\""));
        assert!(!legacy.contains("Изменить</a>"));
        assert!(!legacy.contains("Удалить</button>"));
    }

    #[test]
    fn manager_filters_unsafe_urls_and_never_adds_mutations_to_history() {
        let entries = [
            entry(1, "javascript:alert(1)", "Unsafe"),
            entry(2, "https://safe.test/", "Safe"),
        ];
        let history = render_manager(Page::History, &entries, "token", None, None);
        assert!(history.contains("https://safe.test/"));
        assert!(!history.contains("javascript:alert(1)"));
        assert!(!history.contains("/bookmarks/delete"));
        assert!(!history.contains("/open?token="));
        assert!(history.contains("action=\"/history/clear\" method=\"post\""));
    }

    #[test]
    fn delete_confirmation_is_separate_and_escapes_the_selected_title() {
        let entries = [entry(9, "https://safe.test/", "<script>bad</script>")];
        let html = render_manager(Page::Bookmarks, &entries, "tok&en", None, Some(9));
        assert!(html.contains("Удалить закладку?"));
        assert!(html.contains("&lt;script&gt;bad&lt;/script&gt;"));
        assert!(html.contains("action=\"/bookmarks/delete\" method=\"post\""));
        assert!(html.contains("name=\"token\" value=\"tok&amp;en\""));
        assert!(html.contains("name=\"id\" value=\"9\""));
        assert!(html.contains("href=\"/bookmarks\">Отмена</a>"));
        assert!(!html.contains("<script>bad"));
    }
}
