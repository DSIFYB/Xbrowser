use url::Url;

/// Normalizes an address for direct navigation. Only HTTP and HTTPS are allowed.
pub fn normalize_address(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Address is empty".to_string());
    }

    let explicit_scheme = has_explicit_scheme(input);
    let scheme = input
        .split_once(':')
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    if explicit_scheme && !matches!(scheme.as_deref(), Some("http" | "https")) {
        return Err("Only HTTP and HTTPS addresses are allowed".to_string());
    }

    let candidate = if explicit_scheme {
        input.to_string()
    } else {
        let http_candidate = format!("http://{input}");
        let parsed = Url::parse(&http_candidate).map_err(|_| "Invalid address".to_string())?;
        if is_local_address(parsed.host_str()) {
            http_candidate
        } else {
            format!("https://{input}")
        }
    };

    let url = Url::parse(&candidate).map_err(|_| "Invalid address".to_string())?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("Only HTTP and HTTPS addresses are allowed".to_string());
    }
    if url.host_str().is_none() {
        return Err("Address must include a host".to_string());
    }

    Ok(url.to_string())
}

/// Resolves an omnibox value to either a web address or a Google search URL.
pub fn resolve_input(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Address is empty".to_string());
    }

    if has_explicit_scheme(input) {
        return normalize_address(input);
    }

    if looks_like_host(input) {
        return normalize_address(input);
    }

    let mut search = Url::parse("https://www.google.com/search").expect("valid Google URL");
    search.query_pairs_mut().append_pair("q", input);
    Ok(search.to_string())
}

fn has_explicit_scheme(input: &str) -> bool {
    let Some((prefix, _)) = input.split_once(':') else {
        return false;
    };
    !prefix.is_empty()
        && prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
        && prefix
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic())
        && !is_host_port(input)
}

fn is_host_port(input: &str) -> bool {
    let (host, rest) = if input.starts_with('[') {
        let Some(close) = input.find(']') else {
            return false;
        };
        (
            &input[..=close],
            input[close + 1..].strip_prefix(':').unwrap_or_default(),
        )
    } else {
        let Some((host, rest)) = input.split_once(':') else {
            return false;
        };
        (host, rest)
    };
    let port = rest.split(['/', '?', '#']).next().unwrap_or_default();
    is_known_host(host) && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit())
}

fn looks_like_host(input: &str) -> bool {
    if input.chars().any(char::is_whitespace) {
        return false;
    }
    let authority = input.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.starts_with('[') {
        return authority
            .find(']')
            .is_some_and(|end| authority[1..end].parse::<std::net::IpAddr>().is_ok());
    }

    let host = authority.split(':').next().unwrap_or_default();
    is_known_host(host)
}

fn is_known_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.contains('.')
        || host.parse::<std::net::IpAddr>().is_ok()
}

fn is_local_address(host: Option<&str>) -> bool {
    match host {
        Some(host) if host.eq_ignore_ascii_case("localhost") => true,
        Some(host) => host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_address, resolve_input};

    #[test]
    fn address_defaults_to_https_and_keeps_path() {
        assert_eq!(
            normalize_address("example.com/docs/start").unwrap(),
            "https://example.com/docs/start"
        );
        assert_eq!(
            normalize_address("http://example.com").unwrap(),
            "http://example.com/"
        );
    }

    #[test]
    fn localhost_and_loopback_use_http() {
        assert_eq!(
            normalize_address("localhost:3000/app").unwrap(),
            "http://localhost:3000/app"
        );
        assert_eq!(
            normalize_address("127.0.0.1:8080").unwrap(),
            "http://127.0.0.1:8080/"
        );
        assert_eq!(
            normalize_address("[::1]:8080/status").unwrap(),
            "http://[::1]:8080/status"
        );
        assert_eq!(normalize_address("localhost").unwrap(), "http://localhost/");
    }

    #[test]
    fn resolves_search_text_with_unicode_and_reserved_characters() {
        let resolved = resolve_input("кофе & чай").unwrap();
        let url = Url::parse(&resolved).unwrap();
        assert_eq!(url.host_str(), Some("www.google.com"));
        assert_eq!(url.path(), "/search");
        assert_eq!(
            url.query_pairs().find(|(key, _)| key == "q").unwrap().1,
            "кофе & чай"
        );
        assert!(resolved.contains("%26"));
    }

    #[test]
    fn resolves_words_as_search_and_domains_as_addresses() {
        for query in ["rust", "cats and dogs", "what is rust-lang.org"] {
            let resolved = resolve_input(query).unwrap();
            let url = Url::parse(&resolved).unwrap();
            assert_eq!(url.host_str(), Some("www.google.com"));
            assert_eq!(
                url.query_pairs().find(|(key, _)| key == "q").unwrap().1,
                query
            );
        }
        assert_eq!(
            resolve_input("example.com:8443/a").unwrap(),
            "https://example.com:8443/a"
        );
    }

    #[test]
    fn rejects_forbidden_scheme_with_numeric_payload() {
        assert!(resolve_input("javascript:123").is_err());
    }

    #[test]
    fn forbidden_schemes_and_empty_input_are_errors() {
        for input in [
            "",
            "  ",
            "javascript:alert(1)",
            "file:///tmp/page",
            "data:text/plain,hi",
        ] {
            assert!(resolve_input(input).is_err(), "accepted {input:?}");
            assert!(normalize_address(input).is_err(), "accepted {input:?}");
        }
    }

    use url::Url;
}
