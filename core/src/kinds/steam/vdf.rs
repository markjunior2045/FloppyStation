//! Reader for Valve's text KeyValues format ("VDF"), used by Steam's
//! `libraryfolders.vdf` and `appmanifest_*.acf` files:
//!
//! ```text
//! "AppState"
//! {
//!     "appid"     "730"
//!     "name"      "Counter-Strike 2"
//! }
//! ```
//!
//! Only what those files use: quoted keys and values, nested blocks and
//! `//` comments. Lenient: anything it can't make sense of ends the block.

/// A value: text or a block of keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    Text(String),
    Block(Block),
}

/// The keys of a block, in file order. Keys may repeat.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Block(Vec<(String, Value)>);

impl Block {
    /// The first value under `key`, ignoring case like Steam does.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, value)| value)
    }

    pub(crate) fn text(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            Value::Text(text) => Some(text),
            Value::Block(_) => None,
        }
    }

    pub(crate) fn block(&self, key: &str) -> Option<&Block> {
        match self.get(key)? {
            Value::Block(block) => Some(block),
            Value::Text(_) => None,
        }
    }

    /// The blocks inside this one, with their keys.
    pub(crate) fn blocks(&self) -> impl Iterator<Item = (&str, &Block)> {
        self.0.iter().filter_map(|(key, value)| match value {
            Value::Block(block) => Some((key.as_str(), block)),
            Value::Text(_) => None,
        })
    }
}

/// Parses a whole file: its top-level keys.
pub(crate) fn parse(text: &str) -> Block {
    let mut tokens = Tokens(text.chars().peekable());
    parse_block(&mut tokens)
}

fn parse_block(tokens: &mut Tokens) -> Block {
    let mut entries = Vec::new();
    while let Some(Token::Text(key)) = tokens.next() {
        match tokens.next() {
            Some(Token::Text(value)) => entries.push((key, Value::Text(value))),
            Some(Token::Open) => entries.push((key, Value::Block(parse_block(tokens)))),
            _ => break,
        }
    }
    Block(entries)
}

enum Token {
    Text(String),
    Open,
    Close,
}

struct Tokens<'a>(std::iter::Peekable<std::str::Chars<'a>>);

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            match self.0.next()? {
                c if c.is_whitespace() => {}
                '/' if self.0.peek() == Some(&'/') => {
                    for c in self.0.by_ref() {
                        if c == '\n' {
                            break;
                        }
                    }
                }
                '{' => return Some(Token::Open),
                '}' => return Some(Token::Close),
                '"' => return Some(Token::Text(self.quoted())),
                // Unquoted text, which these files don't use.
                c => {
                    let mut text = String::from(c);
                    while let Some(&c) = self.0.peek() {
                        if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                            break;
                        }
                        text.push(c);
                        self.0.next();
                    }
                    return Some(Token::Text(text));
                }
            }
        }
    }
}

impl Tokens<'_> {
    /// The rest of a quoted string, with `\\` and `\"` unescaped.
    fn quoted(&mut self) -> String {
        let mut text = String::new();
        while let Some(c) = self.0.next() {
            match c {
                '"' => break,
                '\\' => match self.0.next() {
                    Some('n') => text.push('\n'),
                    Some('t') => text.push('\t'),
                    Some(c) => text.push(c),
                    None => break,
                },
                c => text.push(c),
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_app_manifest() {
        let file = parse(
            "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\
             \t\"UserConfig\"\n\t{\n\t\t\"language\"\t\t\"english\"\n\t}\n}\n",
        );
        let app = file.block("AppState").unwrap();
        assert_eq!(app.text("appid"), Some("730"));
        assert_eq!(app.text("NAME"), Some("Counter-Strike 2"));
        assert_eq!(
            app.block("UserConfig").unwrap().text("language"),
            Some("english")
        );
    }

    #[test]
    fn unescapes_paths() {
        let file = parse(r#""0" { "path" "C:\\Program Files (x86)\\Steam" }"#);
        assert_eq!(
            file.block("0").unwrap().text("path"),
            Some(r"C:\Program Files (x86)\Steam")
        );
    }

    #[test]
    fn skips_comments_and_survives_garbage() {
        let file = parse("// comment\n\"a\" \"1\"\n\"b\" {\n\"c\" \"2\"\n");
        assert_eq!(file.text("a"), Some("1"));
        assert_eq!(file.block("b").unwrap().text("c"), Some("2"));
        assert_eq!(parse("}}}").text("a"), None);
    }
}
