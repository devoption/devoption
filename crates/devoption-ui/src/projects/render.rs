//! Repeated project-page fragments; inputs are text, never trusted HTML.
use super::escape_into;

pub(super) fn code_row(html: &mut String, label: &str, value: &str) {
    html.push_str("<div><dt>");
    escape_into(html, label);
    html.push_str("</dt><dd><code>");
    escape_into(html, value);
    html.push_str("</code></dd></div>");
}

pub(super) fn action_status(html: &mut String, id: &str, message: &str) {
    html.push_str("<p id=\"");
    escape_into(html, id);
    html.push_str("\" role=\"status\" aria-live=\"polite\">");
    escape_into(html, message);
    html.push_str("</p>");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_attribute_inputs_cannot_escape_their_markup_boundaries() {
        let hostile = "<script>&\"' 雪";
        let mut html = String::new();
        code_row(&mut html, hostile, hostile);
        action_status(&mut html, hostile, hostile);
        assert_eq!(
            html,
            concat!(
                "<div><dt>&lt;script&gt;&amp;&quot;&#39; 雪</dt><dd><code>&lt;script&gt;&amp;&quot;&#39; 雪</code></dd></div>",
                "<p id=\"&lt;script&gt;&amp;&quot;&#39; 雪\" role=\"status\" aria-live=\"polite\">&lt;script&gt;&amp;&quot;&#39; 雪</p>"
            )
        );
    }
}
