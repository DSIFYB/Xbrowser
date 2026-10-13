use url::Url;

/// Converts an address-bar value into an absolute HTTP(S) URL.
pub fn normalize_address(input: &str) -> Result<String, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Address is empty".to_string());
    }

    let is_localhost_with_port = input
        .strip_prefix("localhost:")
        .is_some_and(|port| port.chars().next().is_some_and(|c| c.is_ascii_digit()));

    let has_explicit_scheme = input.split_once(':').is_some_and(|(prefix, _)| {
        !prefix.is_empty()
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
            && prefix
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
    });

    let candidate = if is_localhost_with_port {
        format!("http://{input}")
    } else if has_explicit_scheme {
        input.to_string()
    } else {
        format!("https://{input}")
    };

    let url = Url::parse(&candidate).map_err(|_| "Invalid address".to_string())?;
    match url.scheme() {
        "http" | "https" => {}
        _ => return Err("Only HTTP and HTTPS addresses are allowed".to_string()),
    }

    if url.host_str().is_none() {
        return Err("Address must include a host".to_string());
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::normalize_address;

    #[test]
    fn adds_https_by_default() {
        assert_eq!(
            normalize_address("example.com").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn allows_http_and_https() {
        assert_eq!(
            normalize_address("http://example.com").unwrap(),
            "http://example.com/"
        );
        assert_eq!(
            normalize_address("https://example.com").unwrap(),
            "https://example.com/"
        );
    }

    #[test]
    fn localhost_with_port_uses_http() {
        assert_eq!(
            normalize_address("localhost:3000").unwrap(),
            "http://localhost:3000/"
        );
    }

    #[test]
    fn rejects_empty_and_other_schemes() {
        assert!(normalize_address("  ").is_err());
        assert!(normalize_address("javascript:alert(1)").is_err());
        assert!(normalize_address("file:///tmp/page").is_err());
    }
}
