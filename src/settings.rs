use crate::navigation::normalize_address;
use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchEngine {
    Google,
    Bing,
    DuckDuckGo,
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::Google
    }
}

impl SearchEngine {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Bing => "bing",
            Self::DuckDuckGo => "duckduckgo",
        }
    }

    pub(crate) fn from_key(value: &str) -> Option<Self> {
        match value {
            "google" => Some(Self::Google),
            "bing" => Some(Self::Bing),
            "duckduckgo" => Some(Self::DuckDuckGo),
            _ => None,
        }
    }

    pub fn search_url(self, query: &str) -> String {
        let base = match self {
            Self::Google => "https://www.google.com/search",
            Self::Bing => "https://www.bing.com/search",
            Self::DuckDuckGo => "https://duckduckgo.com/",
        };
        let mut url = Url::parse(base).expect("valid search engine URL");
        url.query_pairs_mut().append_pair("q", query);
        url.to_string()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub restore_session: bool,
    pub lazy_restore: bool,
    pub search_engine: SearchEngine,
    pub confirm_close_multiple: bool,
    pub home_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            restore_session: true,
            lazy_restore: true,
            search_engine: SearchEngine::Google,
            confirm_close_multiple: false,
            home_url: "about:blank".to_string(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.home_url == "about:blank" {
            return Ok(());
        }
        let url = Url::parse(&self.home_url).map_err(|_| "Invalid home URL".to_string())?;
        if matches!(url.scheme(), "http" | "https") && url.host_str().is_some() {
            Ok(())
        } else {
            Err("Home URL must use HTTP or HTTPS, or be about:blank".to_string())
        }
    }
}

pub fn render(settings: &Settings, token: &str, saved: bool) -> String {
    let token = escape_html(token);
    let home_url = escape_html(&settings.home_url);
    let selected_engine = settings.search_engine.key();
    let saved_notice = if saved {
        "<p role=\"status\">Настройки сохранены.</p>"
    } else {
        ""
    };

    format!(
        "<!doctype html>\
         <html lang=\"ru\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; form-action 'self'\">\
         <title>Настройки</title><style>body{{font-family:'Segoe UI',sans-serif;margin:2rem auto;max-width:760px;padding:0 1rem}}label{{display:block;margin:1rem 0}}select,input{{display:block;margin-top:.35rem;padding:.4rem;max-width:100%}}button{{margin-top:1rem;padding:.5rem 1rem}}</style></head><body><main><h1>Настройки</h1>{saved_notice}\
         <form method=\"post\" action=\"/settings/save\">\
         <input type=\"hidden\" name=\"token\" value=\"{token}\">\
         <label>При запуске <select name=\"restore_session\">{}\
         </select></label>\
         <label>Загрузка восстановленных вкладок <select name=\"lazy_restore\">{}\
         </select></label>\
         <label>Поисковая система <select name=\"search_engine\">\
         <option value=\"google\"{}>Google</option>\
         <option value=\"bing\"{}>Bing</option>\
         <option value=\"duckduckgo\"{}>DuckDuckGo</option>\
         </select></label>\
         <label>Подтверждать закрытие нескольких вкладок <select name=\"confirm_close_multiple\">{}\
         </select></label>\
         <label>Страница новой вкладки (about:blank — встроенная) <input name=\"home_url\" type=\"text\" value=\"{home_url}\" required></label>\
         <p>Настройки запуска применятся после перезапуска. Поисковая система меняется сразу.</p>\
         <button type=\"submit\">Сохранить</button></form></main></body></html>",
        bool_options(settings.restore_session, "Восстанавливать сессию", "Новая вкладка"),
        bool_options(settings.lazy_restore, "При выборе вкладки", "Все вкладки сразу"),
        selected(selected_engine == "google"),
        selected(selected_engine == "bing"),
        selected(selected_engine == "duckduckgo"),
        bool_options(settings.confirm_close_multiple, "Да", "Нет"),
    )
}

pub fn parse_form(body: &[u8]) -> Result<(Settings, String), String> {
    if body.len() > 16_384 {
        return Err("Settings form is too large".to_string());
    }

    let mut fields = std::collections::HashMap::new();
    for (key, value) in url::form_urlencoded::parse(body) {
        let key = key.into_owned();
        if !matches!(
            key.as_str(),
            "restore_session"
                | "lazy_restore"
                | "search_engine"
                | "confirm_close_multiple"
                | "home_url"
                | "token"
        ) {
            return Err("Unexpected settings field".to_string());
        }
        if fields.insert(key, value.into_owned()).is_some() {
            return Err("Duplicate settings field".to_string());
        }
    }

    let get = |key: &str| {
        fields
            .get(key)
            .map(String::as_str)
            .ok_or_else(|| format!("Missing settings field: {key}"))
    };
    let parse_bool = |key: &str| -> Result<bool, String> {
        match get(key)? {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(format!("Invalid value for {key}")),
        }
    };

    let engine = SearchEngine::from_key(get("search_engine")?)
        .ok_or_else(|| "Invalid search engine".to_string())?;
    let home_url = get("home_url")?.trim();
    let home_url = if home_url == "about:blank" {
        "about:blank".to_string()
    } else {
        normalize_address(home_url)?
    };
    let settings = Settings {
        restore_session: parse_bool("restore_session")?,
        lazy_restore: parse_bool("lazy_restore")?,
        search_engine: engine,
        confirm_close_multiple: parse_bool("confirm_close_multiple")?,
        home_url,
    };
    settings.validate()?;
    let token = get("token")?.to_string();
    Ok((settings, token))
}

fn bool_options(value: bool, true_label: &str, false_label: &str) -> String {
    format!(
        "<option value=\"true\"{}>{}</option><option value=\"false\"{}>{}</option>",
        selected(value),
        escape_html(true_label),
        selected(!value),
        escape_html(false_label)
    )
}

fn selected(is_selected: bool) -> &'static str {
    if is_selected {
        " selected"
    } else {
        ""
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::{parse_form, render, SearchEngine, Settings};

    #[test]
    fn defaults_validate_and_search_url_encodes_unicode() {
        let settings = Settings::default();
        assert!(settings.restore_session);
        assert!(settings.lazy_restore);
        assert_eq!(settings.search_engine, SearchEngine::Google);
        assert!(!settings.confirm_close_multiple);
        assert_eq!(settings.home_url, "about:blank");
        settings.validate().unwrap();

        let url = settings.search_engine.search_url("кофе & чай");
        let parsed = Url::parse(&url).unwrap();
        assert_eq!(
            parsed.query_pairs().find(|(key, _)| key == "q").unwrap().1,
            "кофе & чай"
        );
        assert!(url.contains("%26"));
    }

    #[test]
    fn all_search_engines_build_encoded_search_urls_and_home_urls_validate() {
        for (engine, host) in [
            (SearchEngine::Google, "www.google.com"),
            (SearchEngine::Bing, "www.bing.com"),
            (SearchEngine::DuckDuckGo, "duckduckgo.com"),
        ] {
            let url = engine.search_url("rust & кофе");
            let parsed = Url::parse(&url).unwrap();
            assert_eq!(parsed.host_str(), Some(host));
            assert_eq!(
                parsed.query_pairs().find(|(key, _)| key == "q").unwrap().1,
                "rust & кофе"
            );
        }

        Settings {
            home_url: "file:///private/path".into(),
            ..Settings::default()
        }
        .validate()
        .expect_err("file URLs are not valid home pages");
    }

    #[test]
    fn parses_form_and_rejects_duplicate_or_invalid_fields() {
        let body = b"restore_session=false&lazy_restore=true&search_engine=bing&confirm_close_multiple=true&home_url=https%3A%2F%2Fexample.com%2Fhome&token=csrf";
        let (settings, token) = parse_form(body).unwrap();
        assert!(!settings.restore_session);
        assert!(settings.lazy_restore);
        assert_eq!(settings.search_engine, SearchEngine::Bing);
        assert!(settings.confirm_close_multiple);
        assert_eq!(settings.home_url, "https://example.com/home");
        assert_eq!(token, "csrf");

        let body = b"restore_session=true&lazy_restore=false&search_engine=google&confirm_close_multiple=false&home_url=example.com&token=csrf";
        assert_eq!(parse_form(body).unwrap().0.home_url, "https://example.com/");

        assert!(parse_form(b"restore_session=maybe").is_err());
        assert!(parse_form(b"restore_session=true&restore_session=false").is_err());
    }

    #[test]
    fn render_escapes_token_and_home_url() {
        let settings = Settings {
            home_url: "https://example.com/?a=\"<x>".to_string(),
            ..Settings::default()
        };
        let html = render(&settings, "<csrf&>", true);
        assert!(html.contains("name=\"token\" value=\"&lt;csrf&amp;&gt;\""));
        assert!(html.contains("&quot;&lt;x&gt;"));
        assert!(html.contains("/settings/save"));
        assert!(html.contains("Настройки сохранены."));
        assert!(html.contains("lang=\"ru\""));
    }

    use url::Url;
}
