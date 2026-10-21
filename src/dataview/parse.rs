//! The Dataview Query Language: `LIST`/`TABLE`/`TASK`/`CALENDAR` queries
//! with `FROM`, `WHERE`, `SORT`, `GROUP BY`, `FLATTEN` and `LIMIT`, and the
//! expression language shared with inline queries (`= this.file.name`).

use super::value::{parse_duration, parse_link_literal, Value};
use crate::datefmt;

#[derive(Clone, Debug)]
pub enum Expr {
    Lit(Value),
    Field(String),
    This,
    Row,
    Index(Box<Expr>, Box<Expr>),
    Call(Box<Expr>, Vec<Expr>),
    Unary(char, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
    List(Vec<Expr>),
    Object(Vec<(String, Expr)>),
    Lambda(Vec<String>, Box<Expr>),
    /// `date(today)` and other keyword dates, resolved when evaluated.
    DateKeyword(String),
}

#[derive(Clone, Debug)]
pub enum Source {
    All,
    Tag(String),
    Folder(String),
    LinkedFrom(String),
    Outgoing(String),
    Not(Box<Source>),
    And(Box<Source>, Box<Source>),
    Or(Box<Source>, Box<Source>),
}

#[derive(Clone, Debug)]
pub enum Header {
    List { without_id: bool, expr: Option<Expr> },
    Table { without_id: bool, columns: Vec<(Expr, String)> },
    Task,
    Calendar(Expr),
}

#[derive(Clone, Debug)]
pub enum Op {
    Where(Expr),
    Sort(Vec<(Expr, bool)>),
    GroupBy(Expr, String),
    Flatten(Expr, String),
    Limit(usize),
}

#[derive(Clone, Debug)]
pub struct Query {
    pub header: Header,
    pub source: Source,
    pub ops: Vec<Op>,
}

pub type PResult<T> = Result<T, String>;

struct P<'a> {
    s: &'a str,
    i: usize,
}

const KEYWORDS: &[&str] = &["FROM", "WHERE", "SORT", "GROUP", "FLATTEN", "LIMIT", "AS", "ASC", "DESC", "ASCENDING", "DESCENDING", "AND", "OR", "BY", "WITHOUT"];

fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || is_emoji(c)
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || is_emoji(c)
}

fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F300..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x1F000..=0x1F2FF)
}

impl<'a> P<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.i..]
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn ws(&mut self) {
        loop {
            let r = self.rest();
            let t = r.trim_start();
            self.i += r.len() - t.len();
            // `//` comments run to the end of the line
            if t.starts_with("//") {
                self.i += t.find('\n').unwrap_or(t.len());
                continue;
            }
            break;
        }
    }

    fn eat(&mut self, tok: &str) -> bool {
        self.ws();
        if self.rest().starts_with(tok) {
            self.i += tok.len();
            true
        } else {
            false
        }
    }

    /// A case-insensitive keyword followed by a non-identifier character.
    fn peek_kw(&self, kw: &str) -> bool {
        let r = self.rest().trim_start();
        r.len() >= kw.len()
            && r.is_char_boundary(kw.len())
            && r[..kw.len()].eq_ignore_ascii_case(kw)
            && !r[kw.len()..].chars().next().is_some_and(is_ident_char)
    }

    fn kw(&mut self, kw: &str) -> bool {
        self.ws();
        if self.peek_kw(kw) {
            self.i += kw.len();
            true
        } else {
            false
        }
    }

    fn err<T>(&self, what: &str) -> PResult<T> {
        let near: String = self.rest().chars().take(30).collect();
        Err(if near.is_empty() { format!("{what} at the end of the query") } else { format!("{what} near \"{near}\"") })
    }

    fn ident(&mut self) -> Option<String> {
        self.ws();
        let r = self.rest();
        let mut chars = r.char_indices();
        let (_, c) = chars.next()?;
        if !is_ident_start(c) {
            return None;
        }
        let mut end = c.len_utf8();
        for (i, c) in chars {
            if is_ident_char(c) {
                end = i + c.len_utf8();
            } else {
                break;
            }
        }
        // a trailing `-` belongs to a subtraction (`a- 1` is rare, `a-1` reads as identifier like Dataview)
        let id = &r[..end];
        self.i += end;
        Some(id.to_string())
    }

    fn string(&mut self) -> PResult<String> {
        self.ws();
        let quote = self.peek().unwrap_or('"');
        self.i += 1;
        let mut out = String::new();
        let mut chars = self.rest().char_indices();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' => {
                    if let Some((_, n)) = chars.next() {
                        out.push(match n {
                            'n' => '\n',
                            't' => '\t',
                            other => other,
                        });
                    }
                }
                c if c == quote => {
                    self.i += i + 1;
                    return Ok(out);
                }
                c => out.push(c),
            }
        }
        self.err("unterminated string")
    }

    // ── expressions ────────────────────────────────────────────────────────

    fn expr(&mut self) -> PResult<Expr> {
        self.or()
    }

    fn or(&mut self) -> PResult<Expr> {
        let mut l = self.and()?;
        loop {
            if !(self.kw("or") || self.eat("||") || self.eat("|")) {
                return Ok(l);
            }
            let r = self.and()?;
            l = Expr::Binary("or".into(), Box::new(l), Box::new(r));
        }
    }

    fn and(&mut self) -> PResult<Expr> {
        let mut l = self.cmp()?;
        loop {
            if self.kw("and") || self.eat("&&") || self.eat("&") {
                let r = self.cmp()?;
                l = Expr::Binary("and".into(), Box::new(l), Box::new(r));
            } else {
                return Ok(l);
            }
        }
    }

    fn cmp(&mut self) -> PResult<Expr> {
        let mut l = self.add()?;
        loop {
            self.ws();
            let op = ["<=", ">=", "!=", "==", "=", "<", ">"].into_iter().find(|o| self.rest().starts_with(o) && !(o == &"=" && self.rest().starts_with("=>")));
            match op {
                Some(o) => {
                    self.i += o.len();
                    let r = self.add()?;
                    let o = if o == "==" { "=" } else { o };
                    l = Expr::Binary(o.into(), Box::new(l), Box::new(r));
                }
                None => return Ok(l),
            }
        }
    }

    fn add(&mut self) -> PResult<Expr> {
        let mut l = self.mul()?;
        loop {
            self.ws();
            let c = self.peek();
            if c == Some('+') || (c == Some('-') && !self.rest().starts_with("->")) {
                self.i += 1;
                let r = self.mul()?;
                l = Expr::Binary(c.unwrap().to_string(), Box::new(l), Box::new(r));
            } else {
                return Ok(l);
            }
        }
    }

    fn mul(&mut self) -> PResult<Expr> {
        let mut l = self.unary()?;
        loop {
            self.ws();
            let c = self.peek();
            if matches!(c, Some('*') | Some('/') | Some('%')) && !self.rest().starts_with("//") {
                self.i += 1;
                let r = self.unary()?;
                l = Expr::Binary(c.unwrap().to_string(), Box::new(l), Box::new(r));
            } else {
                return Ok(l);
            }
        }
    }

    fn unary(&mut self) -> PResult<Expr> {
        self.ws();
        match self.peek() {
            Some('-') => {
                self.i += 1;
                Ok(Expr::Unary('-', Box::new(self.unary()?)))
            }
            Some('!') => {
                self.i += 1;
                Ok(Expr::Unary('!', Box::new(self.unary()?)))
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            // no whitespace allowed before `.`, `[` and `(` (keeps `a (b)` apart)
            let r = self.rest();
            if let Some(r2) = r.strip_prefix('.') {
                if r2.chars().next().is_some_and(is_ident_start) {
                    self.i += 1;
                    let name = self.ident().unwrap();
                    e = Expr::Index(Box::new(e), Box::new(Expr::Lit(Value::str(name))));
                    continue;
                }
            }
            if r.starts_with('[') && !r.starts_with("[[") {
                self.i += 1;
                let idx = self.expr()?;
                if !self.eat("]") {
                    return self.err("expected ']'");
                }
                e = Expr::Index(Box::new(e), Box::new(idx));
                continue;
            }
            if r.starts_with('(') {
                self.i += 1;
                let args = self.args(')')?;
                e = Expr::Call(Box::new(e), args);
                continue;
            }
            return Ok(e);
        }
    }

    fn args(&mut self, close: char) -> PResult<Vec<Expr>> {
        let mut out = Vec::new();
        self.ws();
        if self.peek() == Some(close) {
            self.i += 1;
            return Ok(out);
        }
        loop {
            out.push(self.expr()?);
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some(c) if c == close => {
                    self.i += 1;
                    return Ok(out);
                }
                _ => return self.err(&format!("expected ',' or '{close}'")),
            }
        }
    }

    /// `(a, b) => body` starting at `(`; restores the position when it is not a lambda.
    fn try_lambda(&mut self) -> Option<Expr> {
        let save = self.i;
        self.i += 1;
        let mut params = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(')') {
                self.i += 1;
                break;
            }
            match self.ident() {
                Some(p) => params.push(p),
                None => {
                    self.i = save;
                    return None;
                }
            }
            self.ws();
            match self.peek() {
                Some(',') => self.i += 1,
                Some(')') => {
                    self.i += 1;
                    break;
                }
                _ => {
                    self.i = save;
                    return None;
                }
            }
        }
        if !self.eat("=>") {
            self.i = save;
            return None;
        }
        match self.expr() {
            Ok(body) => Some(Expr::Lambda(params, Box::new(body))),
            Err(_) => {
                self.i = save;
                None
            }
        }
    }

    /// The raw text up to the matching `)` (for `date(...)` / `dur(...)` literals).
    fn raw_parens(&self) -> Option<(String, usize)> {
        let r = self.rest();
        let mut depth = 0;
        for (i, c) in r.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((r[1..i].trim().to_string(), i + 1));
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn primary(&mut self) -> PResult<Expr> {
        self.ws();
        let r = self.rest();
        let Some(c) = self.peek() else { return self.err("expected a value") };
        if c == '"' || c == '\'' {
            return Ok(Expr::Lit(Value::String(self.string()?)));
        }
        if c.is_ascii_digit() {
            let n = r.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.').count();
            let text = &r[..n];
            self.i += n;
            return text.parse().map(|v| Expr::Lit(Value::Number(v))).or_else(|_| self.err("bad number"));
        }
        if r.starts_with("[[") || r.starts_with("![[") {
            let end = r.find("]]").map(|i| i + 2);
            if let Some(end) = end {
                if let Some(l) = parse_link_literal(&r[..end]) {
                    self.i += end;
                    return Ok(Expr::Lit(Value::Link(l)));
                }
            }
            return self.err("bad link");
        }
        if c == '[' {
            self.i += 1;
            return Ok(Expr::List(self.args(']')?));
        }
        if c == '{' {
            self.i += 1;
            let mut fields = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some('}') {
                    self.i += 1;
                    break;
                }
                let key = match self.peek() {
                    Some('"') | Some('\'') => self.string()?,
                    _ => self.ident().ok_or_else(|| "expected a key".to_string())?,
                };
                if !self.eat(":") {
                    return self.err("expected ':'");
                }
                fields.push((key, self.expr()?));
                self.ws();
                if self.peek() == Some(',') {
                    self.i += 1;
                }
            }
            return Ok(Expr::Object(fields));
        }
        if c == '(' {
            if let Some(l) = self.try_lambda() {
                return Ok(l);
            }
            self.i += 1;
            let e = self.expr()?;
            if !self.eat(")") {
                return self.err("expected ')'");
            }
            return Ok(e);
        }
        let save = self.i;
        if let Some(id) = self.ident() {
            let lower = id.to_lowercase();
            match lower.as_str() {
                "true" => return Ok(Expr::Lit(Value::Bool(true))),
                "false" => return Ok(Expr::Lit(Value::Bool(false))),
                "null" => return Ok(Expr::Lit(Value::Null)),
                "this" => return Ok(Expr::This),
                "row" => return Ok(Expr::Row),
                _ => {}
            }
            if KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(&id)) && !self.rest().starts_with('(') {
                self.i = save;
                return self.err("expected a value");
            }
            // single-parameter lambda `x => …`
            {
                let s2 = self.i;
                if self.eat("=>") {
                    let body = self.expr()?;
                    return Ok(Expr::Lambda(vec![id], Box::new(body)));
                }
                self.i = s2;
            }
            if (lower == "date" || lower == "dur") && self.rest().starts_with('(') {
                if let Some((raw, len)) = self.raw_parens() {
                    let unquoted = !raw.starts_with('"') && !raw.starts_with('\'');
                    if lower == "date" && unquoted {
                        let k = raw.to_lowercase();
                        if matches!(k.as_str(), "today" | "now" | "tomorrow" | "yesterday" | "sow" | "eow" | "som" | "eom" | "soy" | "eoy") {
                            self.i += len;
                            return Ok(Expr::DateKeyword(k));
                        }
                        if let Some(d) = datefmt::parse_iso(&raw) {
                            self.i += len;
                            return Ok(Expr::Lit(Value::Date(d)));
                        }
                    }
                    if lower == "dur" && unquoted {
                        if let Some(d) = parse_duration(&raw) {
                            self.i += len;
                            return Ok(Expr::Lit(Value::Duration(d)));
                        }
                    }
                }
            }
            return Ok(Expr::Field(id));
        }
        self.err("unexpected character")
    }

    // ── sources ────────────────────────────────────────────────────────────

    fn source(&mut self) -> PResult<Source> {
        let mut l = self.source_and()?;
        while self.kw("or") {
            let r = self.source_and()?;
            l = Source::Or(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn source_and(&mut self) -> PResult<Source> {
        let mut l = self.source_atom()?;
        while self.kw("and") {
            let r = self.source_atom()?;
            l = Source::And(Box::new(l), Box::new(r));
        }
        Ok(l)
    }

    fn source_atom(&mut self) -> PResult<Source> {
        self.ws();
        let r = self.rest();
        match self.peek() {
            Some('-') | Some('!') => {
                self.i += 1;
                Ok(Source::Not(Box::new(self.source_atom()?)))
            }
            Some('#') => {
                let n = r.char_indices().skip(1).find(|(_, c)| c.is_whitespace() || *c == ')' || *c == ',').map(|(i, _)| i).unwrap_or(r.len());
                self.i += n;
                Ok(Source::Tag(r[..n].to_string()))
            }
            Some('"') => Ok(Source::Folder(self.string()?)),
            Some('(') => {
                self.i += 1;
                let s = self.source()?;
                if !self.eat(")") {
                    return self.err("expected ')'");
                }
                Ok(s)
            }
            Some('[') if r.starts_with("[[") => {
                let end = r.find("]]").ok_or("unterminated link")? + 2;
                let l = parse_link_literal(&r[..end]).ok_or("bad link")?;
                self.i += end;
                Ok(Source::LinkedFrom(l.target()))
            }
            _ => {
                if self.kw("outgoing") {
                    if !self.eat("(") {
                        return self.err("expected '('");
                    }
                    self.ws();
                    let r = self.rest();
                    let end = r.find("]]").ok_or("expected a link")? + 2;
                    let l = parse_link_literal(&r[..end]).ok_or("bad link")?;
                    self.i += end;
                    if !self.eat(")") {
                        return self.err("expected ')'");
                    }
                    return Ok(Source::Outgoing(l.target()));
                }
                self.err("expected a source (#tag, \"folder\" or [[link]])")
            }
        }
    }

    // ── queries ────────────────────────────────────────────────────────────

    fn name_after_as(&mut self) -> PResult<Option<String>> {
        if !self.kw("as") {
            return Ok(None);
        }
        self.ws();
        match self.peek() {
            Some('"') | Some('\'') => Ok(Some(self.string()?)),
            _ => Ok(Some(self.ident().ok_or_else(|| "expected a name after AS".to_string())?)),
        }
    }

    fn at_clause(&self) -> bool {
        let r = self.rest().trim_start();
        r.is_empty() || ["FROM", "WHERE", "SORT", "GROUP", "FLATTEN", "LIMIT"].iter().any(|k| self.peek_kw(k))
    }

    fn query(&mut self) -> PResult<Query> {
        let header = if self.kw("list") {
            let without_id = self.kw("without") && self.kw("id");
            let expr = if self.at_clause() { None } else { Some(self.expr()?) };
            Header::List { without_id, expr }
        } else if self.kw("table") {
            let without_id = self.kw("without") && self.kw("id");
            let mut columns = Vec::new();
            while !self.at_clause() {
                let start = self.i;
                let e = self.expr()?;
                let text = self.s[start..self.i].trim().to_string();
                let name = self.name_after_as()?.unwrap_or(text);
                columns.push((e, name));
                if !self.eat(",") {
                    break;
                }
            }
            Header::Table { without_id, columns }
        } else if self.kw("task") {
            Header::Task
        } else if self.kw("calendar") {
            Header::Calendar(self.expr()?)
        } else {
            return self.err("a query starts with LIST, TABLE, TASK or CALENDAR");
        };
        let mut source = Source::All;
        let mut ops = Vec::new();
        loop {
            self.ws();
            if self.rest().is_empty() {
                break;
            }
            if self.kw("from") {
                source = self.source()?;
            } else if self.kw("where") {
                ops.push(Op::Where(self.expr()?));
            } else if self.kw("sort") {
                let mut keys = Vec::new();
                loop {
                    let e = self.expr()?;
                    let desc = if self.kw("descending") || self.kw("desc") {
                        true
                    } else {
                        let _ = self.kw("ascending") || self.kw("asc");
                        false
                    };
                    keys.push((e, desc));
                    if !self.eat(",") {
                        break;
                    }
                }
                ops.push(Op::Sort(keys));
            } else if self.kw("group") {
                if !self.kw("by") {
                    return self.err("expected BY");
                }
                let start = self.i;
                let e = self.expr()?;
                let text = self.s[start..self.i].trim().to_string();
                let name = self.name_after_as()?.unwrap_or(text);
                ops.push(Op::GroupBy(e, name));
            } else if self.kw("flatten") {
                let start = self.i;
                let e = self.expr()?;
                let text = self.s[start..self.i].trim().to_string();
                let name = self.name_after_as()?.unwrap_or(text);
                ops.push(Op::Flatten(e, name));
            } else if self.kw("limit") {
                self.ws();
                let n = self.rest().bytes().take_while(u8::is_ascii_digit).count();
                if n == 0 {
                    return self.err("expected a number after LIMIT");
                }
                ops.push(Op::Limit(self.rest()[..n].parse().unwrap()));
                self.i += n;
            } else {
                return self.err("unexpected text");
            }
        }
        Ok(Query { header, source, ops })
    }
}

pub fn parse_query(text: &str) -> PResult<Query> {
    P { s: text, i: 0 }.query()
}

/// A `FROM` source on its own (what `dv.pages("#tag")` receives); empty means all pages.
pub fn parse_source(text: &str) -> PResult<Source> {
    if text.trim().is_empty() {
        return Ok(Source::All);
    }
    let mut p = P { s: text, i: 0 };
    let src = p.source()?;
    p.ws();
    if !p.rest().is_empty() {
        return p.err("unexpected text");
    }
    Ok(src)
}

pub fn parse_expr(text: &str) -> PResult<Expr> {
    let mut p = P { s: text, i: 0 };
    let e = p.expr()?;
    p.ws();
    if !p.rest().is_empty() {
        return p.err("unexpected text");
    }
    Ok(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries() {
        let q = parse_query("TABLE file.mtime AS \"Modified\", status\nFROM #proj AND -\"Archive\"\nWHERE status != \"done\" and progresso > 50\nSORT file.mtime DESC\nLIMIT 10").unwrap();
        match &q.header {
            Header::Table { columns, .. } => {
                assert_eq!(columns.len(), 2);
                assert_eq!(columns[0].1, "Modified");
                assert_eq!(columns[1].1, "status");
            }
            _ => panic!(),
        }
        assert!(matches!(q.source, Source::And(_, _)));
        assert_eq!(q.ops.len(), 3);
        let q = parse_query("LIST WITHOUT ID file.link GROUP BY tipo").unwrap();
        assert!(matches!(q.header, Header::List { without_id: true, .. }));
        let q = parse_query("TASK FROM [[Home]] WHERE !completed AND due <= date(today) + dur(1 week)").unwrap();
        assert!(matches!(q.source, Source::LinkedFrom(_)));
    }

    #[test]
    fn expressions() {
        assert!(matches!(parse_expr("due-date").unwrap(), Expr::Field(f) if f == "due-date"));
        assert!(matches!(parse_expr("a - b").unwrap(), Expr::Binary(op, _, _) if op == "-"));
        assert!(matches!(parse_expr("map(rows, (r) => r.file.name)").unwrap(), Expr::Call(_, a) if matches!(a[1], Expr::Lambda(_, _))));
        assert!(matches!(parse_expr("date(2026-10-21)").unwrap(), Expr::Lit(Value::Date(_))));
        assert!(matches!(parse_expr("date(file.name)").unwrap(), Expr::Call(_, _)));
        assert!(parse_expr("this.file.tasks.text").is_ok());
        assert!(parse_expr("row[\"Due Date\"]").is_ok());
        assert!(parse_expr("{a: 1, \"b c\": [1, 2]}").is_ok());
    }
}
