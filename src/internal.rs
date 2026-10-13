use crate::{library, send, settings, storage, Action};
use std::{borrow::Cow, collections::HashMap, path::Path};
use wry::http::{Request, Response};

pub fn nonce() -> std::io::Result<String> {
    let mut bytes = [0u8; 32];
    let status = unsafe {
        windows_sys::Win32::Security::Cryptography::BCryptGenRandom(
            std::ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            windows_sys::Win32::Security::Cryptography::BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(std::io::Error::other(format!(
            "Cannot initialize form security: {status:#x}"
        )));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
pub fn navigation_action(uri: &str, token: &str) -> Option<Action> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "http" || url.host_str() != Some("axiom.localhost") {
        return None;
    }
    let query: HashMap<_, _> = url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    if query.get("token").map(String::as_str) != Some(token) {
        return None;
    }
    match url.path() {
        "/open" => {
            let target = query.get("url")?;
            storage::is_external(target).then(|| Action::OpenSaved(target.clone()))
        }
        "/open-bookmarks" => Some(Action::OpenAllBookmarks),
        _ => None,
    }
}
pub fn home(settings: &settings::Settings) -> String {
    let url = url::Url::parse(&settings.search_engine.search_url("")).expect("Static search URL");
    let action = format!(
        "{}://{}{}",
        url.scheme(),
        url.host_str().unwrap(),
        url.path()
    );
    let origin = format!("{}://{}", url.scheme(), url.host_str().unwrap());
    let name = match settings.search_engine {
        settings::SearchEngine::Google => "Google",
        settings::SearchEngine::Bing => "Bing",
        settings::SearchEngine::DuckDuckGo => "DuckDuckGo",
    };
    include_str!("../assets/new_tab.html")
        .replace("https://www.google.com/search", &action)
        .replace("https://www.google.com", &origin)
        .replace("Google", name)
}
pub fn respond(db: &Path, token: &str, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    match route(db, token, &request) {
        Ok(response) => response,
        Err(message) => Response::builder()
            .status(400)
            .header("Content-Type", "text/plain; charset=utf-8")
            .body(Cow::Owned(message.into_bytes()))
            .expect("Valid error response"),
    }
}
fn route(
    db: &Path,
    token: &str,
    request: &Request<Vec<u8>>,
) -> Result<Response<Cow<'static, [u8]>>, String> {
    if request.uri().host() != Some("localhost") {
        return Err("Неизвестная внутренняя страница".into());
    }
    let path = request.uri().path();
    let store = storage::Store::open(db).map_err(|err| err.to_string())?;
    let query: HashMap<_, _> =
        url::form_urlencoded::parse(request.uri().query().unwrap_or("").as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
    if request.method() == "POST" {
        if request
            .headers()
            .get("Origin")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|origin| origin != "http://axiom.localhost")
        {
            return Err("Источник запроса не разрешён".into());
        }
        match path {
            "/settings/save" => {
                let (settings, provided) = settings::parse_form(request.body())?;
                if provided != token {
                    return Err("Форма устарела. Откройте настройки заново.".into());
                }
                store
                    .save_settings(&settings)
                    .map_err(|err| err.to_string())?;
                send(Action::SettingsChanged);
                return redirect("/settings?saved=1");
            }
            "/bookmarks/save" | "/bookmarks/delete" => {
                let form = parse_bookmark_form(request.body(), token, path == "/bookmarks/save")?;
                let id = form["id"]
                    .parse::<i64>()
                    .map_err(|_| "Некорректный номер закладки")?;
                if store.bookmark(id).map_err(|err| err.to_string())?.is_none() {
                    return Err("Закладка уже удалена".into());
                }
                if path == "/bookmarks/save" {
                    let url = crate::navigation::normalize_address(&form["url"])?;
                    store
                        .update_bookmark(id, &url, &form["title"])
                        .map_err(|err| err.to_string())?;
                } else {
                    store.remove_bookmark(id).map_err(|err| err.to_string())?;
                }
                send(Action::BookmarksChanged);
                return redirect("/bookmarks");
            }
            _ => return Err("Неизвестное действие".into()),
        }
    }
    if request.method() != "GET" {
        return Err("Метод запроса не разрешён".into());
    }
    let body = match path {
        "/" => home(&store.settings().map_err(|err| err.to_string())?),
        "/settings" => settings::render(
            &store.settings().map_err(|err| err.to_string())?,
            token,
            query.get("saved").is_some_and(|value| value == "1"),
        ),
        "/history" | "/bookmarks" => {
            let mut entries = if path == "/history" {
                store.history()
            } else {
                store.bookmarks()
            }
            .map_err(|err| err.to_string())?;
            if let Some(q) = query.get("q") {
                let q = q.trim().to_lowercase();
                entries.retain(|entry| {
                    entry.title.to_lowercase().contains(&q) || entry.url.to_lowercase().contains(&q)
                });
            }
            let edit = query
                .get("edit")
                .and_then(|value| value.parse::<i64>().ok());
            library::render_manager(
                if path == "/history" {
                    library::Page::History
                } else {
                    library::Page::Bookmarks
                },
                &entries,
                token,
                edit,
            )
        }
        _ => return Err("Страница не найдена".into()),
    };
    Ok(Response::builder()
        .header("Content-Type", "text/html; charset=utf-8")
        .header("X-Content-Type-Options", "nosniff")
        .body(Cow::Owned(body.into_bytes()))
        .expect("Valid internal response"))
}
fn redirect(path: &str) -> Result<Response<Cow<'static, [u8]>>, String> {
    Ok(Response::builder()
        .status(303)
        .header("Location", format!("http://axiom.localhost{path}"))
        .body(Cow::Borrowed(b"".as_slice()))
        .expect("Valid redirect"))
}
fn parse_bookmark_form(
    body: &[u8],
    token: &str,
    editing: bool,
) -> Result<HashMap<String, String>, String> {
    if body.len() > 16384 {
        return Err("Форма слишком большая".into());
    }
    let mut form = HashMap::new();
    for (k, v) in url::form_urlencoded::parse(body) {
        if !matches!(k.as_ref(), "token" | "id" | "url" | "title")
            || form.insert(k.into_owned(), v.into_owned()).is_some()
        {
            return Err("Некорректная форма".into());
        }
    }
    if form.get("token").map(String::as_str) != Some(token)
        || !form.contains_key("id")
        || (editing && (!form.contains_key("url") || !form.contains_key("title")))
    {
        return Err("Форма устарела или повреждена".into());
    }
    Ok(form)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_actions_require_matching_nonce_and_safe_target() {
        assert!(navigation_action(
            "http://axiom.localhost/open?token=wrong&url=https%3A%2F%2Fexample.com",
            "secret"
        )
        .is_none());
        assert!(navigation_action(
            "http://axiom.localhost/open?token=secret&url=javascript%3Aalert(1)",
            "secret"
        )
        .is_none());
        assert!(matches!(
            navigation_action(
                "http://axiom.localhost/open?token=secret&url=https%3A%2F%2Fexample.com",
                "secret"
            ),
            Some(Action::OpenSaved(_))
        ));
    }
    #[test]
    fn bookmark_forms_reject_missing_token_and_duplicate_ids() {
        assert!(parse_bookmark_form(b"id=1", "secret", false).is_err());
        assert!(parse_bookmark_form(b"token=secret&id=1&id=2", "secret", false).is_err());
    }
    #[test]
    fn home_uses_selected_search_engine() {
        let settings = settings::Settings {
            search_engine: settings::SearchEngine::DuckDuckGo,
            ..Default::default()
        };
        let html = home(&settings);
        assert!(html.contains("action=\"https://duckduckgo.com/\""));
        assert!(!html.contains("Поиск в Google"));
    }
}
