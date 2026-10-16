//! Bounded data-only object literals. No expressions, comments or evaluation.

use crate::settings::{AiAgentHeader, MAX_AI_AGENT_HEADERS};

pub(super) const MAX_OBJECT_BYTES: usize = 65_536;

pub(super) fn parse(source: &str) -> Result<Vec<AiAgentHeader>, ()> {
    if source.len() > MAX_OBJECT_BYTES {
        return Err(());
    }
    let mut parser = Parser { source, offset: 0 };
    parser.expect('{')?;
    let mut headers = Vec::new();
    while !parser.take('}') {
        let name = parser.key()?;
        parser.expect(':')?;
        if parser.take('[') {
            if parser.take(']') {
                return Err(());
            }
            loop {
                parser.push(&mut headers, &name)?;
                if parser.take(']') {
                    break;
                }
                parser.expect(',')?;
                if parser.take(']') {
                    break;
                }
            }
        } else {
            parser.push(&mut headers, &name)?;
        }
        if parser.take('}') {
            break;
        }
        parser.expect(',')?;
    }
    parser.whitespace();
    (parser.offset == source.len()).then_some(headers).ok_or(())
}

pub(super) fn format(headers: &[AiAgentHeader]) -> String {
    let mut entries: Vec<(&str, Vec<&str>)> = Vec::new();
    for header in headers.iter().filter(|header| header.enabled) {
        if let Some((_, values)) = entries.iter_mut().find(|(name, _)| *name == header.name) {
            values.push(&header.value);
        } else {
            entries.push((&header.name, vec![&header.value]));
        }
    }
    let lines = entries
        .into_iter()
        .map(|(name, values)| {
            let value = if values.len() == 1 {
                serde_json::to_string(values[0]).unwrap()
            } else {
                serde_json::to_string(&values).unwrap()
            };
            std::format!("  {}: {value}", serde_json::to_string(name).unwrap())
        })
        .collect::<Vec<_>>();
    std::format!("{{\n{}\n}}", lines.join(",\n"))
}

/// Keep disabled draft rows and preserve an untouched round trip exactly.
pub(super) fn merge(source: &str, previous: &[AiAgentHeader]) -> Result<Vec<AiAgentHeader>, ()> {
    let parsed = parse(source)?;
    if source == format(previous) {
        return Ok(previous.to_vec());
    }
    let mut enabled = parsed.into_iter();
    let mut merged = Vec::new();
    for old in previous {
        if !old.enabled {
            merged.push(old.clone());
        } else if let Some(header) = enabled.next() {
            merged.push(header);
        }
    }
    merged.extend(enabled);
    (merged.len() <= MAX_AI_AGENT_HEADERS)
        .then_some(merged)
        .ok_or(())
}

struct Parser<'a> {
    source: &'a str,
    offset: usize,
}

impl Parser<'_> {
    fn whitespace(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.next();
        }
    }
    fn peek(&self) -> Option<char> {
        self.source[self.offset..].chars().next()
    }
    fn next(&mut self) -> Option<char> {
        let value = self.peek()?;
        self.offset += value.len_utf8();
        Some(value)
    }
    fn take(&mut self, expected: char) -> bool {
        self.whitespace();
        if self.peek() == Some(expected) {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, expected: char) -> Result<(), ()> {
        self.take(expected).then_some(()).ok_or(())
    }
    fn key(&mut self) -> Result<String, ()> {
        self.whitespace();
        if matches!(self.peek(), Some('\'' | '"')) {
            return self.string();
        }
        let mut key = String::new();
        while let Some(c) = self.peek() {
            if c == '_'
                || c == '$'
                || c.is_ascii_alphabetic()
                || (!key.is_empty() && c.is_ascii_digit())
            {
                key.push(c);
                self.next();
            } else {
                break;
            }
        }
        (!key.is_empty()).then_some(key).ok_or(())
    }
    fn string(&mut self) -> Result<String, ()> {
        self.whitespace();
        let quote = self.next().ok_or(())?;
        if !matches!(quote, '\'' | '"') {
            return Err(());
        }
        let mut json = String::from("\"");
        while let Some(c) = self.next() {
            if c == quote {
                json.push('"');
                return serde_json::from_str(&json).map_err(|_| ());
            }
            if c == '\\' {
                match self.next().ok_or(())? {
                    '\'' if quote == '\'' => json.push('\''),
                    escaped => {
                        json.push('\\');
                        json.push(escaped);
                    }
                }
            } else if c == '"' {
                json.push_str("\\\"");
            } else {
                json.push(c);
            }
        }
        Err(())
    }
    fn push(&mut self, headers: &mut Vec<AiAgentHeader>, name: &str) -> Result<(), ()> {
        if headers.len() >= MAX_AI_AGENT_HEADERS {
            return Err(());
        }
        headers.push(AiAgentHeader {
            enabled: true,
            name: name.into(),
            value: self.string()?,
        });
        Ok(())
    }
}

#[cfg(test)]
#[path = "ai_agent_header_object_tests.rs"]
mod tests;
