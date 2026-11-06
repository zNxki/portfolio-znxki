use std::collections::BTreeMap;

use url::Url;

use crate::error::{AppError, AppResult, FieldError};

#[derive(Default)]
pub struct Errors(Vec<FieldError>);

impl Errors {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn push(&mut self, field: &str, message: &str) {
        self.0.push(FieldError {
            field: field.to_string(),
            message: message.to_string(),
        });
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn finish(self) -> AppResult<()> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(AppError::Validation(self.0))
        }
    }
}

fn has_forbidden_chars(s: &str) -> bool {
    s.chars().any(|c| c == '<' || c == '>' || (c.is_control() && c != '\n' && c != '\t'))
}

pub fn plain(e: &mut Errors, field: &str, value: &str, min: usize, max: usize) -> String {
    let v = value.trim().to_string();
    let n = v.chars().count();
    if n < min {
        e.push(field, if min == 1 { "Campo obbligatorio" } else { "Testo troppo corto" });
    } else if n > max {
        e.push(field, "Testo troppo lungo");
    } else if has_forbidden_chars(&v) || v.contains('\n') || v.contains('\t') {
        e.push(field, "Caratteri non consentiti");
    }
    v
}

pub fn plain_opt(e: &mut Errors, field: &str, value: &Option<String>, max: usize) -> Option<String> {
    match value {
        Some(v) if !v.trim().is_empty() => Some(plain(e, field, v, 1, max)),
        _ => None,
    }
}

pub fn slug(e: &mut Errors, field: &str, value: &str) -> String {
    let v = value.trim().to_string();
    let ok = !v.is_empty()
        && v.len() <= 49
        && v.chars().enumerate().all(|(i, c)| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || (i > 0 && (c == '-' || c == '_'))
        });
    if !ok {
        e.push(field, "Slug non valido (a-z, 0-9, - e _, max 49 caratteri)");
    }
    v
}

pub fn email(e: &mut Errors, field: &str, value: &str) -> String {
    let v = value.trim().to_string();
    let valid = v.len() <= 254
        && v.matches('@').count() == 1
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "._%+-@".contains(c))
        && match v.split_once('@') {
            Some((local, domain)) => {
                !local.is_empty()
                    && local.len() <= 64
                    && domain.contains('.')
                    && !domain.starts_with('.')
                    && !domain.ends_with('.')
                    && !domain.contains("..")
            }
            None => false,
        };
    if !valid {
        e.push(field, "Email non valida");
    }
    v
}

fn has_ws_or_control(s: &str) -> bool {
    s.chars().any(|c| c.is_whitespace() || c.is_control())
}

pub fn href(e: &mut Errors, field: &str, value: &str) -> String {
    let v = value.trim().to_string();
    if v.is_empty() || v.len() > 2048 || has_ws_or_control(&v) || v.contains('<') || v.contains('>') {
        e.push(field, "Link non valido");
        return v;
    }
    match Url::parse(&v) {
        Ok(u) => {
            let scheme_ok = matches!(u.scheme(), "https" | "http" | "mailto" | "tel");
            let host_ok = !matches!(u.scheme(), "https" | "http") || u.host_str().is_some();
            if !scheme_ok || !host_ok {
                e.push(field, "Link non valido (schemi ammessi: https, http, mailto, tel)");
            }
        }
        Err(_) => e.push(field, "Link non valido"),
    }
    v
}

pub fn https_url_opt(e: &mut Errors, field: &str, value: &Option<String>) -> Option<String> {
    match value {
        Some(v) if !v.trim().is_empty() => {
            let v = v.trim().to_string();
            if v.len() > 2048 || has_ws_or_control(&v) {
                e.push(field, "URL non valido");
                return Some(v);
            }
            match Url::parse(&v) {
                Ok(u) if u.scheme() == "https" && u.host_str().is_some() => {}
                _ => e.push(field, "URL non valido (solo https)"),
            }
            Some(v)
        }
        _ => None,
    }
}

pub fn hex_color(e: &mut Errors, field: &str, value: &str) -> String {
    let v = value.trim().to_string();
    let ok = v.len() == 7 && v.starts_with('#') && v[1..].chars().all(|c| c.is_ascii_hexdigit());
    if !ok {
        e.push(field, "Colore non valido (#rrggbb)");
    }
    v
}

pub fn lang_code(e: &mut Errors, field: &str, value: &str) -> String {
    let v = value.trim().to_string();
    if v.len() != 2 || !v.chars().all(|c| c.is_ascii_lowercase()) {
        e.push(field, "Codice lingua non valido");
    }
    v
}

pub fn in_range(e: &mut Errors, field: &str, value: i64, min: i64, max: i64) {
    if value < min || value > max {
        e.push(field, "Valore fuori intervallo");
    }
}

fn normalize_tag(inner: &str) -> Option<String> {
    const ALLOWED: [&str; 7] = ["b", "strong", "i", "em", "kbd", "small", "br"];
    let trimmed = inner.trim();
    let (closing, rest) = match trimmed.strip_prefix('/') {
        Some(r) => (true, r.trim()),
        None => (false, trimmed),
    };
    let rest = match rest.strip_suffix('/') {
        Some(r) => r.trim(),
        None => rest,
    };
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let name = rest.to_ascii_lowercase();
    if !ALLOWED.contains(&name.as_str()) {
        return None;
    }
    if name == "br" {
        return if closing { None } else { Some("<br>".to_string()) };
    }
    if closing {
        Some(format!("</{name}>"))
    } else {
        Some(format!("<{name}>"))
    }
}

pub fn sanitize_inline_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    loop {
        match rest.find('<') {
            None => {
                out.push_str(rest);
                break;
            }
            Some(pos) => {
                out.push_str(&rest[..pos]);
                let after = &rest[pos..];
                if let Some(end) = after.find('>') {
                    if let Some(tag) = normalize_tag(&after[1..end]) {
                        out.push_str(&tag);
                        rest = &after[end + 1..];
                        continue;
                    }
                }
                out.push_str("&lt;");
                rest = &after[1..];
            }
        }
    }
    out.chars().filter(|c| !c.is_control() || *c == '\n').collect()
}

pub fn html_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

pub fn i18n_map(
    e: &mut Errors,
    field: &str,
    map: &BTreeMap<String, String>,
    required: bool,
    max: usize,
    html: bool,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for (lang, text) in map {
        let key = lang.trim().to_string();
        if key.len() != 2 || !key.chars().all(|c| c.is_ascii_lowercase()) {
            e.push(field, "Codice lingua non valido");
            continue;
        }
        let t = text.trim();
        if t.chars().count() > max {
            e.push(field, "Testo troppo lungo");
            continue;
        }
        if t.is_empty() {
            continue;
        }
        let cleaned = if html {
            sanitize_inline_html(t)
        } else {
            if has_forbidden_chars(t) {
                e.push(field, "Caratteri non consentiti");
                continue;
            }
            t.to_string()
        };
        out.insert(key, cleaned);
    }
    if required && out.is_empty() {
        e.push(field, "Almeno una lingua è obbligatoria");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer_keeps_safe_tags() {
        assert_eq!(sanitize_inline_html("a <b>b</b> <em>c</em>"), "a <b>b</b> <em>c</em>");
        assert_eq!(sanitize_inline_html("x<br/>y"), "x<br>y");
        assert_eq!(sanitize_inline_html("<kbd>/</kbd>"), "<kbd>/</kbd>");
    }

    #[test]
    fn sanitizer_neutralizes_dangerous_markup() {
        let s = sanitize_inline_html("<script>alert(1)</script>");
        assert!(!s.contains("<script"));
        assert!(!s.contains("</script"));
        let s = sanitize_inline_html("<img src=x onerror=alert(1)>");
        assert!(!s.contains("<img"));
        let s = sanitize_inline_html("<b onclick=\"x()\">t</b>");
        assert!(!s.contains("onclick=\"x()\">") || s.starts_with("&lt;"));
        assert!(!s.contains("<b onclick"));
        let s = sanitize_inline_html("<a href=\"javascript:1\">x</a>");
        assert!(!s.contains("<a "));
        let s = sanitize_inline_html("1 < 2 and <");
        assert!(!s.contains("< 2"));
    }

    #[test]
    fn sanitizer_keeps_entities() {
        assert_eq!(sanitize_inline_html("goto &lt;sezione&gt;"), "goto &lt;sezione&gt;");
    }

    #[test]
    fn href_accepts_only_safe_schemes() {
        let mut e = Errors::new();
        href(&mut e, "h", "https://github.com/x");
        href(&mut e, "h", "mailto:a@b.it");
        assert!(e.is_empty());
        for bad in ["javascript:alert(1)", "data:text/html,x", "ftp://x.it", "https://", "not a url", "//evil.com"] {
            let mut e = Errors::new();
            href(&mut e, "h", bad);
            assert!(!e.is_empty(), "{bad} doveva essere rifiutato");
        }
    }

    #[test]
    fn email_validation() {
        let mut e = Errors::new();
        email(&mut e, "e", "znxx.kii@gmail.com");
        assert!(e.is_empty());
        for bad in ["", "a@", "@b.it", "a b@c.it", "a@b", "a@@b.it", "<a>@b.it", "a@b..it"] {
            let mut e = Errors::new();
            email(&mut e, "e", bad);
            assert!(!e.is_empty(), "{bad} doveva essere rifiutata");
        }
    }

    #[test]
    fn plain_rejects_markup_and_length() {
        let mut e = Errors::new();
        plain(&mut e, "n", "<b>x</b>", 1, 50);
        assert!(!e.is_empty());
        let mut e = Errors::new();
        plain(&mut e, "n", "", 1, 50);
        assert!(!e.is_empty());
        let mut e = Errors::new();
        plain(&mut e, "n", "abcdef", 1, 3);
        assert!(!e.is_empty());
        let mut e = Errors::new();
        assert_eq!(plain(&mut e, "n", "  ok  ", 1, 10), "ok");
        assert!(e.is_empty());
    }

    #[test]
    fn slug_rules() {
        let mut e = Errors::new();
        slug(&mut e, "s", "my-project_1");
        assert!(e.is_empty());
        for bad in ["", "-a", "A", "a b", "a/b", "../x"] {
            let mut e = Errors::new();
            slug(&mut e, "s", bad);
            assert!(!e.is_empty(), "{bad}");
        }
    }

    #[test]
    fn i18n_requires_content_and_valid_langs() {
        let mut m = BTreeMap::new();
        m.insert("it".to_string(), "ciao <script>".to_string());
        let mut e = Errors::new();
        let out = i18n_map(&mut e, "t", &m, true, 100, true);
        assert!(e.is_empty());
        assert!(!out["it"].contains("<script"));

        let mut e = Errors::new();
        i18n_map(&mut e, "t", &BTreeMap::new(), true, 100, false);
        assert!(!e.is_empty());

        let mut m = BTreeMap::new();
        m.insert("xx1".to_string(), "a".to_string());
        let mut e = Errors::new();
        i18n_map(&mut e, "t", &m, false, 100, false);
        assert!(!e.is_empty());
    }

    #[test]
    fn colors() {
        let mut e = Errors::new();
        hex_color(&mut e, "c", "#9fd3ff");
        assert!(e.is_empty());
        let mut e = Errors::new();
        hex_color(&mut e, "c", "red");
        assert!(!e.is_empty());
    }
}