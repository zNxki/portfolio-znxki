use axum::http::{header, HeaderMap};

pub fn session_name(secure: bool) -> &'static str {
    if secure {
        "__Host-session"
    } else {
        "session"
    }
}

pub fn state_name(secure: bool) -> &'static str {
    if secure {
        "__Host-oauth_state"
    } else {
        "oauth_state"
    }
}

pub fn read(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(header::COOKIE) {
        if let Ok(raw) = value.to_str() {
            for part in raw.split(';') {
                if let Some((k, v)) = part.trim().split_once('=') {
                    if k == name {
                        return Some(v.to_string());
                    }
                }
            }
        }
    }
    None
}

pub fn build(name: &str, value: &str, max_age: i64, secure: bool) -> String {
    let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={max_age}");
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

pub fn clear(name: &str, secure: bool) -> String {
    build(name, "", 0, secure)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn reads_named_cookie() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; session=abc; b=2"));
        assert_eq!(read(&h, "session").as_deref(), Some("abc"));
        assert_eq!(read(&h, "missing"), None);
        assert_eq!(read(&h, "ession"), None);
    }

    #[test]
    fn cookie_has_security_attributes() {
        let c = build("__Host-session", "tok", 600, true);
        assert!(c.contains("HttpOnly"));
        assert!(c.contains("Secure"));
        assert!(c.contains("SameSite=Lax"));
        assert!(c.contains("Path=/"));
        assert!(!c.contains("Domain"));
        let insecure = build("session", "tok", 600, false);
        assert!(!insecure.contains("Secure"));
        assert!(clear("session", false).contains("Max-Age=0"));
    }
}