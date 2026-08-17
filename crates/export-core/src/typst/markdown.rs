use std::collections::HashMap;

use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};

use super::utils::escape_typst_string;

fn heading_level_to_equals(level: HeadingLevel) -> &'static str {
    match level {
        HeadingLevel::H1 => "=",
        HeadingLevel::H2 => "==",
        HeadingLevel::H3 => "===",
        HeadingLevel::H4 => "====",
        HeadingLevel::H5 => "=====",
        HeadingLevel::H6 => "======",
    }
}

const DEFAULT_IMAGE_WIDTH_PERCENT: u32 = 80;

// The editor stores its display width in the image title as
// `char-editor-width=NN` or `char-editor-width=NN|<real title>` (see
// packages/editor/src/image-markdown.ts). NN is a percentage of the editor
// width, clamped to 15..=100.
fn image_width_percent(title: &str) -> u32 {
    let Some(rest) = title.strip_prefix("char-editor-width=") else {
        return DEFAULT_IMAGE_WIDTH_PERCENT;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    match digits.parse::<u32>() {
        Ok(value) => value.clamp(15, 100),
        Err(_) => DEFAULT_IMAGE_WIDTH_PERCENT,
    }
}

pub fn markdown_to_typst(md: &str, images: &HashMap<String, String>) -> String {
    let parser = Parser::new(md);
    let mut result = String::new();
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    // Events between Start(Image) and End(Image) are the alt text; swallow
    // them so they never render as body text.
    let mut image_depth = 0usize;

    for event in parser {
        match event {
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) => {
                if image_depth == 0
                    && let Some(vpath) = images.get(dest_url.as_ref())
                {
                    result.push_str(&format!(
                        "#image(\"{}\", width: {}%)\n",
                        vpath,
                        image_width_percent(&title)
                    ));
                }
                image_depth += 1;
            }
            Event::End(TagEnd::Image) => {
                image_depth = image_depth.saturating_sub(1);
            }
            _ if image_depth > 0 => {}
            Event::Start(Tag::Heading { level, .. }) => {
                result.push_str(heading_level_to_equals(level));
                result.push(' ');
            }
            Event::End(TagEnd::Heading(_)) => {
                result.push_str("\n\n");
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                result.push_str("\n\n");
            }
            Event::Start(Tag::Strong) => result.push('*'),
            Event::End(TagEnd::Strong) => result.push('*'),
            Event::Start(Tag::Emphasis) => result.push('_'),
            Event::End(TagEnd::Emphasis) => result.push('_'),
            Event::Start(Tag::Strikethrough) => result.push_str("#strike["),
            Event::End(TagEnd::Strikethrough) => result.push(']'),
            Event::Start(Tag::Link { dest_url, .. }) => {
                result.push_str("#link(\"");
                result.push_str(&dest_url);
                result.push_str("\")[");
            }
            Event::End(TagEnd::Link) => result.push(']'),
            Event::Start(Tag::List(start_num)) => {
                list_stack.push(start_num);
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                let indent = "  ".repeat(list_stack.len().saturating_sub(1));
                if let Some(Some(num)) = list_stack.last_mut() {
                    result.push_str(&format!("{}{}. ", indent, num));
                    *num += 1;
                } else {
                    result.push_str(&format!("{}- ", indent));
                }
            }
            Event::End(TagEnd::Item) => {
                result.push('\n');
            }
            Event::Start(Tag::BlockQuote(_)) => {
                result.push_str("#quote(block: true)[\n");
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                result.push_str("]\n\n");
            }
            Event::Code(text) => {
                result.push('`');
                result.push_str(&text);
                result.push('`');
            }
            Event::Text(text) => {
                result.push_str(&escape_typst_string(&text));
            }
            Event::SoftBreak => result.push('\n'),
            Event::HardBreak => result.push_str("\\\n"),
            _ => {}
        }
    }

    result.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn images(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(src, vpath)| (src.to_string(), vpath.to_string()))
            .collect()
    }

    #[test]
    fn resolvable_image_becomes_typst_image() {
        let map = images(&[("attachments/photo.png", "/att-0.png")]);
        let out = markdown_to_typst(
            "before\n\n![alt text](attachments/photo.png)\n\nafter",
            &map,
        );
        assert!(out.contains("#image(\"/att-0.png\", width: 80%)"));
        assert!(!out.contains("alt text"));
    }

    #[test]
    fn image_width_metadata_is_applied() {
        let map = images(&[("attachments/photo.png", "/att-0.png")]);
        let out = markdown_to_typst(
            "![alt](attachments/photo.png \"char-editor-width=42|caption\")",
            &map,
        );
        assert!(out.contains("#image(\"/att-0.png\", width: 42%)"));
    }

    #[test]
    fn unresolvable_image_is_dropped_with_alt_text() {
        let out = markdown_to_typst("![alt text](https://example.com/x.png)", &HashMap::new());
        assert_eq!(out, "");
    }

    #[test]
    fn width_is_clamped() {
        let map = images(&[("a", "/att-0.png")]);
        let out = markdown_to_typst("![x](a \"char-editor-width=7\")", &map);
        assert!(out.contains("width: 15%"));
    }
}
