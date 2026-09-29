//! Valve's KeyValues text format, as in Steam's `libraryfolders.vdf` and `appmanifest_*.acf`:
//! nested `"key" "value"` pairs and `"key" { … }` blocks, with `//` comments.

use std::iter::Peekable;
use std::str::Chars;

/// Deeper nesting than Steam's files ever have means a broken file.
const MAX_DEPTH: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Map(Vec<(String, Value)>),
}

impl Value {
    /// A child by key, ignoring case (Steam's files are not consistent about it).
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries()
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// A child's text.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key)?.as_str()
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            Value::Map(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Value)] {
        match self {
            Value::Map(entries) => entries,
            Value::Str(_) => &[],
        }
    }
}

/// A whole file as a map of its top-level pairs; `None` when it is not KeyValues.
pub fn parse(text: &str) -> Option<Value> {
    let mut tokens = Tokens {
        chars: text.chars().peekable(),
    };
    entries(&mut tokens, 0).map(Value::Map)
}

enum Token {
    Str(String),
    Open,
    Close,
}

/// Pairs up to the closing brace (or the end, at the top level).
fn entries(tokens: &mut Tokens, depth: usize) -> Option<Vec<(String, Value)>> {
    if depth > MAX_DEPTH {
        return None;
    }
    let mut out = Vec::new();
    loop {
        match tokens.next() {
            None => return (depth == 0).then_some(out),
            Some(Token::Close) => return (depth > 0).then_some(out),
            Some(Token::Open) => return None,
            Some(Token::Str(key)) => match tokens.next()? {
                Token::Str(value) => out.push((key, Value::Str(value))),
                Token::Open => out.push((key, Value::Map(entries(tokens, depth + 1)?))),
                Token::Close => return None,
            },
        }
    }
}

struct Tokens<'a> {
    chars: Peekable<Chars<'a>>,
}

impl Iterator for Tokens<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        loop {
            let c = *self.chars.peek()?;
            if c.is_whitespace() {
                self.chars.next();
            } else if c == '/' {
                // A comment runs to the end of the line.
                self.chars.next();
                if self.chars.peek() != Some(&'/') {
                    return Some(Token::Str(format!("/{}", self.bare())));
                }
                for c in self.chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        match self.chars.next()? {
            '{' => Some(Token::Open),
            '}' => Some(Token::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = self.chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match self.chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        c => s.push(c),
                    }
                }
                Some(Token::Str(s))
            }
            c => {
                let mut s = String::from(c);
                s.push_str(&self.bare());
                Some(Token::Str(s))
            }
        }
    }
}

impl Tokens<'_> {
    /// The rest of an unquoted word.
    fn bare(&mut self) -> String {
        let mut s = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                break;
            }
            s.push(c);
            self.chars.next();
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_steam_library_folders() {
        let text = r#"
"libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
		"apps"
		{
			"228980"		"181248418"
			"292030"		"50460532170"
		}
	}
	// A second drive.
	"1"
	{
		"path"		"D:\\SteamLibrary"
		"apps" { }
	}
}
"#;
        let root = parse(text).unwrap();
        let folders = root.get("LibraryFolders").unwrap();
        let paths: Vec<&str> = folders
            .entries()
            .iter()
            .filter_map(|(_, f)| f.str("path"))
            .collect();
        assert_eq!(paths, [r"C:\Program Files (x86)\Steam", r"D:\SteamLibrary"]);
        let apps = folders.get("0").unwrap().get("apps").unwrap();
        assert_eq!(apps.str("292030"), Some("50460532170"));
        assert!(
            folders
                .get("1")
                .unwrap()
                .get("apps")
                .unwrap()
                .entries()
                .is_empty()
        );
    }

    #[test]
    fn reads_app_manifests_and_bare_words() {
        let text = "\"AppState\"\n{\n\t\"appid\"\t\t\"292030\"\n\t\"name\"\t\t\"The Witcher 3: \\\"Wild\\\" Hunt\"\n\tStateFlags 4\n}\n";
        let state = parse(text).unwrap();
        let state = state.get("appstate").unwrap();
        assert_eq!(state.str("appid"), Some("292030"));
        assert_eq!(state.str("name"), Some("The Witcher 3: \"Wild\" Hunt"));
        assert_eq!(state.str("StateFlags"), Some("4"));
    }

    #[test]
    fn refuses_broken_files() {
        assert_eq!(parse("\"a\" {"), None, "unclosed");
        assert_eq!(parse("}"), None, "stray brace");
        assert_eq!(parse("\"a\""), None, "a key without a value");
        assert_eq!(parse(&"\"a\" {".repeat(100)), None, "too deep");
        assert_eq!(parse(""), Some(Value::Map(Vec::new())));
    }
}
