/// Strips HTML tags from content (simple regex-free approach).
pub fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;

    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }

    // Collapse multiple whitespace
    let mut collapsed = String::with_capacity(result.len());
    let mut prev_space = false;
    for c in result.chars() {
        if c.is_whitespace() {
            if !prev_space {
                collapsed.push(if c == '\n' { '\n' } else { ' ' });
            }
            prev_space = true;
        } else {
            collapsed.push(c);
            prev_space = false;
        }
    }

    collapsed.trim().to_string()
}

/// Decodes common HTML entities.
pub fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_html_basic() {
        let html = "<html><body><p>Hello <b>world</b></p></body></html>";
        assert_eq!(strip_html_tags(html), "Hello world");
    }

    #[test]
    fn strip_html_preserves_text() {
        assert_eq!(strip_html_tags("plain text"), "plain text");
    }

    #[test]
    fn strip_html_collapses_whitespace() {
        let html = "<p>hello</p>   <p>world</p>";
        let result = strip_html_tags(html);
        assert!(!result.contains("   "));
    }

    #[test]
    fn decode_html_entities_all() {
        assert_eq!(
            decode_html_entities("A &amp; B &lt; C &gt; D &quot;E&quot; &#39;F&#39;"),
            "A & B < C > D \"E\" 'F'"
        );
    }
}
