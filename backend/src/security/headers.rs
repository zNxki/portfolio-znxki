use base64::engine::general_purpose::STANDARD;
use base64::Engine;

use super::tokens::sha256;

pub const API_CSP: &str = "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; sandbox";

pub fn inline_script_hashes(html: &str) -> Vec<String> {
    let mut hashes = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<script>") {
        let body_start = start + "<script>".len();
        let after = &rest[body_start..];
        match after.find("</script>") {
            Some(end) => {
                let body = &after[..end];
                hashes.push(format!(
                    "'sha256-{}'",
                    STANDARD.encode(sha256(body.as_bytes()))
                ));
                rest = &after[end + "</script>".len()..];
            }
            None => break,
        }
    }
    hashes
}

pub fn public_csp(script_hashes: &[String]) -> String {
    let mut script_src = String::from("script-src 'self'");
    for h in script_hashes {
        script_src.push(' ');
        script_src.push_str(h);
    }
    format!(
        "default-src 'self'; {script_src}; style-src 'self' https://fonts.googleapis.com; \
         style-src-attr 'unsafe-inline'; font-src 'self' https://fonts.gstatic.com; \
         img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; \
         form-action 'self'; frame-ancestors 'none'"
    )
}

pub fn admin_csp() -> String {
    "default-src 'none'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; \
     font-src https://fonts.gstatic.com; img-src 'self' data:; connect-src 'self'; \
     base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_only_inline_scripts() {
        let html = "<head><script>let a=1;</script><script src=\"x.js\" defer></script></head>";
        let hashes = inline_script_hashes(html);
        assert_eq!(hashes.len(), 1);
        assert!(hashes[0].starts_with("'sha256-"));
        assert!(hashes[0].ends_with("='"));
    }

    #[test]
    fn public_csp_blocks_framing_and_inline_by_default() {
        let csp = public_csp(&["'sha256-abc='".to_string()]);
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(csp.contains("script-src 'self' 'sha256-abc='"));
        assert!(!csp.contains("script-src 'self' 'unsafe-inline'"));
        assert!(csp.contains("object-src 'none'"));
    }

    #[test]
    fn admin_csp_is_strict() {
        let csp = admin_csp();
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("script-src 'self';"));
        assert!(!csp.contains("unsafe-inline"));
    }
}
