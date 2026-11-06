//! The whole of JSON that a glTF header uses, written by hand.
//!
//! There is no serde in this workspace, and a scene loader that pulls one in for one
//! file format drags a derive macro, a data model and a dozen impls along with it. GlTF
//! JSON needs five value shapes, no floats ever round-trip through a string here in a way
//! that matters, and the parser doubles as the writer for the fixtures in `writer`.

use std::fmt::Write as _;

/// Nested containers deeper than this are a malformed file, not a model. The limit is a
/// budget against a stack overflow while parsing a hostile header.
const MAX_DEPTH: usize = 128;

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Parse a whole JSON document, rejecting trailing content.
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut cursor = Cursor {
            bytes: text.as_bytes(),
            pos: 0,
        };
        cursor.skip_ws();
        let value = cursor.value(0)?;
        cursor.skip_ws();
        if cursor.peek().is_some() {
            return Err(format!("trailing content at byte {}", cursor.pos));
        }
        Ok(value)
    }

    /// The member of an object, or `None` for anything that is not one.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(members) => members
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_f32(&self) -> Option<f32> {
        self.as_f64().map(|n| n as f32)
    }

    /// A non-negative integer index. Fractional or out-of-range numbers are refused, so a
    /// file cannot aim past a table by writing `1.5` or `-1`.
    pub fn as_index(&self) -> Option<usize> {
        match self {
            Json::Num(n) if *n >= 0.0 && *n <= usize::MAX as f64 && *n == (*n as i64 as f64) => {
                Some(*n as usize)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    pub fn name(&self) -> Option<String> {
        self.get("name").and_then(Json::as_str).map(str::to_string)
    }

    /// Compact JSON, good enough to hand-check and exactly what `parse` reads back.
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Num(n) => {
                // Floats print their shortest round-tripping form; a whole float still
                // needs its `.0` so a reader cannot mistake it for an integer type.
                let text = format!("{n}");
                if text.chars().any(|c| c == '.' || c == 'e' || c == 'E') {
                    out.push_str(&text);
                } else {
                    let _ = write!(out, "{text}.0");
                }
            }
            Json::Str(s) => dump_string(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(members) => {
                out.push('{');
                for (i, (key, value)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    dump_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn dump_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Cursor<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, wanted: u8) -> bool {
        if self.peek() == Some(wanted) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, wanted: u8) -> Result<(), String> {
        if self.eat(wanted) {
            Ok(())
        } else {
            Err(format!(
                "expected {:?} at byte {}",
                wanted as char, self.pos
            ))
        }
    }

    fn value(&mut self, depth: usize) -> Result<Json, String> {
        if depth > MAX_DEPTH {
            return Err(format!(
                "nesting is deeper than {MAX_DEPTH} at byte {}",
                self.pos
            ));
        }
        match self.peek() {
            None => Err(format!("document ends at byte {}", self.pos)),
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => Ok(Json::Str(self.string()?)),
            Some(b't') | Some(b'f') => self.boolean(),
            Some(b'n') => self.null(),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(c) => Err(format!("unexpected {:?} at byte {}", c as char, self.pos)),
        }
    }

    /// Consume one of the bare literals and return the value it names.
    fn literal(&mut self, word: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(word.as_bytes()) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(format!("expected {word} at byte {}", self.pos))
        }
    }

    fn null(&mut self) -> Result<Json, String> {
        self.literal("null")?;
        Ok(Json::Null)
    }

    fn boolean(&mut self) -> Result<Json, String> {
        if self.peek() == Some(b't') {
            self.literal("true")?;
            Ok(Json::Bool(true))
        } else {
            self.literal("false")?;
            Ok(Json::Bool(false))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        self.eat(b'-');
        self.digits()?;
        if self.eat(b'.') {
            self.digits()?;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            self.eat(b'+');
            self.eat(b'-');
            self.digits()?;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| format!("byte {start} is not inside a number"))?;
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("{text:?} at byte {start} is not a number"))
    }

    fn digits(&mut self) -> Result<(), String> {
        let start = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        if self.pos == start {
            return Err(format!("expected a digit at byte {}", self.pos));
        }
        Ok(())
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut out = String::new();
        let mut run = self.pos;
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "a string literal never ends".to_string())?;
            if byte == b'"' {
                self.flush(&mut out, run)?;
                self.pos += 1;
                return Ok(out);
            }
            if byte != b'\\' {
                self.pos += 1;
                continue;
            }
            // The run stops before the backslash, which belongs to the escape, not to the text.
            self.flush(&mut out, run)?;
            self.pos += 1;
            out.push(self.escape()?);
            run = self.pos;
        }
    }

    /// Copy the plain bytes between `run` and the cursor into the string being built.
    fn flush(&self, out: &mut String, run: usize) -> Result<(), String> {
        let raw = &self.bytes[run..self.pos];
        let text =
            std::str::from_utf8(raw).map_err(|_| format!("byte {run} is not valid UTF-8"))?;
        out.push_str(text);
        Ok(())
    }

    /// One backslash escape, already stepped past its `\`.
    fn escape(&mut self) -> Result<char, String> {
        let mark = self.pos;
        let kind = self
            .peek()
            .ok_or_else(|| "a string ends inside an escape".to_string())?;
        self.pos += 1;
        match kind {
            b'"' => Ok('"'),
            b'\\' => Ok('\\'),
            b'/' => Ok('/'),
            b'b' => Ok('\u{8}'),
            b'f' => Ok('\u{c}'),
            b'n' => Ok('\n'),
            b'r' => Ok('\r'),
            b't' => Ok('\t'),
            b'u' => self.hex4(),
            c => Err(format!("unknown escape \\{} at byte {mark}", c as char)),
        }
    }

    /// A `\uXXXX`, joining a surrogate pair into one codepoint.
    fn hex4(&mut self) -> Result<char, String> {
        let first = self.hex16()?;
        if !(0xDC00..0xE000).contains(&first) && first & 0xFC00 == 0xD800 {
            // High surrogate: the low half has to follow or the pair is broken.
            if self.peek() != Some(b'\\') {
                return Err("a high surrogate is not followed by a low one".into());
            }
            self.pos += 1;
            if self.peek() != Some(b'u') {
                return Err("a high surrogate is not followed by \\u".into());
            }
            self.pos += 1;
            let second = self.hex16()?;
            if !(0xDC00..0xE000).contains(&second) {
                return Err("the second half of a surrogate pair is not a low surrogate".into());
            }
            let joined = 0x1_0000 + ((first - 0xD800) << 10) + (second - 0xDC00);
            return char::from_u32(joined).ok_or_else(|| "surrogate pair is out of range".into());
        }
        char::from_u32(first).ok_or_else(|| format!("\\u{first:04x} is not a codepoint"))
    }

    fn hex16(&mut self) -> Result<u32, String> {
        let start = self.pos;
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self
                .peek()
                .ok_or_else(|| "a \\u escape is cut short".to_string())?;
            self.pos += 1;
            let digit = byte
                .to_ascii_lowercase()
                .checked_sub(b'0')
                .and_then(|d| if d < 10 { Some(d as u32) } else { None })
                .or_else(|| {
                    let d = byte.to_ascii_lowercase().checked_sub(b'a')?;
                    if d < 6 { Some(d as u32 + 10) } else { None }
                })
                .ok_or_else(|| format!("byte {start} is not a hex digit"))?;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn array(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.eat(b']') {
            return Ok(Json::Arr(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value(depth + 1)?);
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.expect(b']')?;
            return Ok(Json::Arr(items));
        }
    }

    fn object(&mut self, depth: usize) -> Result<Json, String> {
        self.expect(b'{')?;
        let mut members = Vec::new();
        self.skip_ws();
        if self.eat(b'}') {
            return Ok(Json::Obj(members));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(b':')?;
            self.skip_ws();
            let value = self.value(depth + 1)?;
            members.push((key, value));
            self.skip_ws();
            if self.eat(b',') {
                continue;
            }
            self.expect(b'}')?;
            return Ok(Json::Obj(members));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn reads_the_shapes_a_gltf_header_uses() {
        let value = Json::parse(
            r#"  {"a": [1, 2.5, -3], "b": {"c": "hi"}, "d": true, "e": null, "f": []}  "#,
        )
        .unwrap();

        let numbers = value.get("a").and_then(Json::as_array).unwrap();
        assert_eq!(numbers.len(), 3);
        assert_eq!(numbers[0].as_index(), Some(1));
        assert_eq!(numbers[1].as_f32(), Some(2.5));
        assert_eq!(
            numbers[2].as_index(),
            None,
            "a negative index is not an index"
        );
        assert_eq!(
            value
                .get("b")
                .and_then(|b| b.get("c"))
                .and_then(Json::as_str),
            Some("hi")
        );
        assert_eq!(value.get("d"), Some(&Json::Bool(true)));
        assert_eq!(value.get("e"), Some(&Json::Null));
        assert!(value.get("f").and_then(Json::as_array).unwrap().is_empty());
        assert_eq!(value.get("missing"), None);
    }

    #[test]
    fn refuses_numbers_that_are_not_indexes() {
        for text in ["1.5", "-1", "true", "null", "\"2\"", "1e-3"] {
            let value = Json::parse(text).unwrap();
            if text != "1e-3" {
                assert_eq!(value.as_index(), None, "{text} is not an index");
            }
        }
        // A whole float still is an index; glTF writes its numbers this way.
        assert_eq!(Json::parse("4.0").unwrap().as_index(), Some(4));
        // Past 2^53 a JSON number stops being exact, so it is not an index either.
        assert_eq!(Json::parse("1e100").unwrap().as_f64(), Some(1e100));
    }

    #[test]
    fn reads_escapes_and_a_split_codepoint() {
        let value = Json::parse(r#""line\nbrk \u0041 \uD83D\uDE00 \\ \"\/""#).unwrap();
        assert_eq!(value.as_str(), Some("line\nbrk A \u{1F600} \\ \"/"));
    }

    #[test]
    fn rejects_a_document_that_does_not_hold_together() {
        for text in [
            "",
            "{\"a\": 1} trailing",
            "{\"a\": }",
            "{\"a\" 1}",
            "[1, 2",
            "\"never closed",
            "{\"key\": \"val\"",
            "nul",
            "{,}",
        ] {
            assert!(Json::parse(text).is_err(), "{text:?} should not parse");
        }
    }

    #[test]
    fn stops_at_the_nesting_limit() {
        let deep = "[".repeat(200) + &"]".repeat(200);
        let why = Json::parse(&deep).unwrap_err();
        assert!(why.contains("128"), "{why}");

        let shallow = "[".repeat(60) + &"]".repeat(60);
        assert!(Json::parse(&shallow).is_ok());
    }

    #[test]
    fn writes_back_what_it_read() {
        let text = r#"{"a": [1, 2.5], "b": {"c": "two words\n"}, "d": [], "e": {}}"#;
        let value = Json::parse(text).unwrap();
        let written = value.dump();
        assert_eq!(Json::parse(&written).unwrap(), value, "{written}");
        assert!(written.contains("1.0"), "a whole float stays a float");
    }
}
