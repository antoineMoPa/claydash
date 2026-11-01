//! Scene definition language, usable by applications, command-line tools and agents.
//!
//! Parsing is kept separate from scene mutation so malformed input cannot edit a
//! document. Source positions are byte offsets into the original UTF-8 text.

#[derive(Clone, Debug, PartialEq)]
struct Token {
    pub kind: Kind,
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Ident(String),
    Number(f32),
    String(String),
    Symbol(char),
}

fn error(source: &str, offset: usize, message: &str) -> String {
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix.rsplit('\n').next().unwrap_or("").chars().count() + 1;
    format!("{line}:{column}: {message}")
}

fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut pos = 0;
    while pos < source.len() {
        let c = source[pos..].chars().next().unwrap();
        if c.is_whitespace() {
            pos += c.len_utf8();
            continue;
        }
        if c == '#' {
            pos += source[pos..].find('\n').unwrap_or(source.len() - pos);
            continue;
        }
        let start = pos;
        if c.is_ascii_alphabetic() || c == '_' {
            pos += 1;
            while pos < source.len()
                && (source.as_bytes()[pos].is_ascii_alphanumeric()
                    || source.as_bytes()[pos] == b'_')
            {
                pos += 1;
            }
            tokens.push(Token {
                kind: Kind::Ident(source[start..pos].to_owned()),
                offset: start,
            });
        } else if c.is_ascii_digit()
            || (c == '.'
                && source
                    .as_bytes()
                    .get(pos + 1)
                    .is_some_and(u8::is_ascii_digit))
        {
            pos += 1;
            while pos < source.len()
                && (source.as_bytes()[pos].is_ascii_digit() || source.as_bytes()[pos] == b'.')
            {
                pos += 1;
            }
            if matches!(source.as_bytes().get(pos), Some(b'e' | b'E')) {
                pos += 1;
                if matches!(source.as_bytes().get(pos), Some(b'+' | b'-')) {
                    pos += 1;
                }
                while pos < source.len() && source.as_bytes()[pos].is_ascii_digit() {
                    pos += 1;
                }
            }
            let value: f32 = source[start..pos]
                .parse()
                .map_err(|_| error(source, start, "invalid number"))?;
            if !value.is_finite() {
                return Err(error(source, start, "number must be finite"));
            }
            tokens.push(Token {
                kind: Kind::Number(value),
                offset: start,
            });
        } else if c == '"' {
            pos += 1;
            let mut value = String::new();
            let mut closed = false;
            while pos < source.len() {
                let ch = source[pos..].chars().next().unwrap();
                pos += ch.len_utf8();
                match ch {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => {
                        let escaped = source[pos..]
                            .chars()
                            .next()
                            .ok_or_else(|| error(source, start, "unterminated string"))?;
                        pos += escaped.len_utf8();
                        value.push(match escaped {
                            '"' => '"',
                            '\\' => '\\',
                            'n' => '\n',
                            't' => '\t',
                            _ => {
                                return Err(error(
                                    source,
                                    pos - escaped.len_utf8(),
                                    "invalid string escape",
                                ))
                            }
                        });
                    }
                    '\n' | '\r' => return Err(error(source, start, "unterminated string")),
                    _ => value.push(ch),
                }
            }
            if !closed {
                return Err(error(source, start, "unterminated string"));
            }
            tokens.push(Token {
                kind: Kind::String(value),
                offset: start,
            });
        } else if "(){}[],:;=+-*/".contains(c) {
            pos += c.len_utf8();
            tokens.push(Token {
                kind: Kind::Symbol(c),
                offset: start,
            });
        } else {
            return Err(error(source, start, "unexpected character"));
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_recipe_and_distinguishes_color_from_comments() {
        let tokens =
            tokenize("# comment\nbox(size: [width / 2, .5, 1e-2], color: \"#aabbcc\");").unwrap();
        assert!(tokens
            .iter()
            .any(|t| t.kind == Kind::String("#aabbcc".into())));
        assert!(tokens.iter().any(|t| t.kind == Kind::Number(0.01)));
        assert_eq!(tokens[0].offset, 10);
    }

    #[test]
    fn reports_locations_and_rejects_bad_literals() {
        assert!(tokenize("# café\n  1.2.3").unwrap_err().starts_with("2:3:"));
        assert!(tokenize("\"unterminated")
            .unwrap_err()
            .contains("unterminated"));
        assert!(tokenize("1e999").unwrap_err().contains("finite"));
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Expr {
    Number(f32),
    String(String),
    Name(String),
    Vector(Vec<Expr>),
    Unary(char, Box<Expr>),
    Binary(char, Box<Expr>, Box<Expr>),
    Call(String, Vec<(Option<String>, Expr)>),
}

#[derive(Clone, Debug, PartialEq)]
enum Statement {
    Definition {
        name: String,
        parameters: Vec<String>,
        body: Vec<Statement>,
        offset: usize,
    },
    Assignment {
        name: String,
        value: Expr,
        offset: usize,
    },
    Expression {
        value: Expr,
        offset: usize,
    },
}

fn parse(source: &str) -> Result<Vec<Statement>, String> {
    let tokens = tokenize(source)?;
    if tokens.len() > 100_000 {
        return Err("source exceeds 100000 tokens".into());
    }
    let mut nesting = 0usize;
    let mut unary_run = 0usize;
    for token in &tokens {
        match token.kind {
            Kind::Symbol('(' | '[' | '{') => {
                nesting += 1;
                if nesting > 128 {
                    return Err(error(source, token.offset, "nesting exceeds 128 levels"));
                }
            }
            Kind::Symbol(')' | ']' | '}') => nesting = nesting.saturating_sub(1),
            _ => {}
        }
        if token.kind == Kind::Symbol('-') {
            unary_run += 1;
        } else {
            unary_run = 0;
        }
        if unary_run > 128 {
            return Err(error(
                source,
                token.offset,
                "too many consecutive unary operators",
            ));
        }
    }
    let mut parser = Parser {
        source,
        tokens,
        cursor: 0,
    };
    let mut statements = Vec::new();
    while parser.cursor < parser.tokens.len() {
        statements.push(parser.statement()?);
    }
    Ok(statements)
}

struct Parser<'a> {
    source: &'a str,
    tokens: Vec<Token>,
    cursor: usize,
}
impl Parser<'_> {
    fn failure(&self, message: &str) -> String {
        error(
            self.source,
            self.tokens
                .get(self.cursor)
                .map_or(self.source.len(), |t| t.offset),
            message,
        )
    }
    fn symbol(&mut self, symbol: char) -> bool {
        if self
            .tokens
            .get(self.cursor)
            .is_some_and(|t| t.kind == Kind::Symbol(symbol))
        {
            self.cursor += 1;
            true
        } else {
            false
        }
    }
    fn require(&mut self, symbol: char) -> Result<(), String> {
        if self.symbol(symbol) {
            Ok(())
        } else {
            Err(self.failure(&format!("expected '{symbol}'")))
        }
    }
    fn ident(&mut self) -> Result<String, String> {
        if let Some(Token {
            kind: Kind::Ident(name),
            ..
        }) = self.tokens.get(self.cursor)
        {
            self.cursor += 1;
            Ok(name.clone())
        } else {
            Err(self.failure("expected a name"))
        }
    }
    fn keyword(&mut self, word: &str) -> bool {
        if self
            .tokens
            .get(self.cursor)
            .is_some_and(|t| matches!(&t.kind, Kind::Ident(name) if name == word))
        {
            self.cursor += 1;
            true
        } else {
            false
        }
    }
    fn statement(&mut self) -> Result<Statement, String> {
        let offset = self
            .tokens
            .get(self.cursor)
            .map_or(self.source.len(), |t| t.offset);
        if self.keyword("object") {
            let name = self.ident()?;
            self.require('(')?;
            self.require('{')?;
            let mut parameters = Vec::new();
            if !self.symbol('}') {
                loop {
                    let parameter = self.ident()?;
                    self.require(':')?;
                    if !self.keyword("number") {
                        return Err(self.failure("expected number parameter type"));
                    }
                    if parameters.contains(&parameter) {
                        return Err(self.failure("duplicate parameter"));
                    }
                    parameters.push(parameter);
                    if self.symbol('}') {
                        break;
                    }
                    self.require(',')?;
                }
            }
            self.require(')')?;
            self.require(':')?;
            let mut body = Vec::new();
            while !self.keyword("end") {
                if self.cursor == self.tokens.len() {
                    return Err(self.failure("expected end"));
                }
                if self
                    .tokens
                    .get(self.cursor)
                    .is_some_and(|t| matches!(&t.kind, Kind::Ident(name) if name == "object"))
                {
                    return Err(self.failure("object definitions must be top level"));
                }
                body.push(self.statement()?);
            }
            Ok(Statement::Definition {
                name,
                parameters,
                body,
                offset,
            })
        } else if matches!(
            self.tokens.get(self.cursor).map(|t| &t.kind),
            Some(Kind::Ident(_))
        ) && self
            .tokens
            .get(self.cursor + 1)
            .is_some_and(|t| t.kind == Kind::Symbol('='))
        {
            let name = self.ident()?;
            self.require('=')?;
            let value = self.expr(0)?;
            self.require(';')?;
            Ok(Statement::Assignment {
                name,
                value,
                offset,
            })
        } else {
            let value = self.expr(0)?;
            self.require(';')?;
            Ok(Statement::Expression { value, offset })
        }
    }
    fn expr(&mut self, min_precedence: u8) -> Result<Expr, String> {
        let mut left = if self.symbol('-') {
            Expr::Unary('-', Box::new(self.expr(3)?))
        } else if self.symbol('(') {
            let e = self.expr(0)?;
            self.require(')')?;
            e
        } else if self.symbol('[') {
            let mut values = Vec::new();
            if !self.symbol(']') {
                loop {
                    values.push(self.expr(0)?);
                    if self.symbol(']') {
                        break;
                    }
                    self.require(',')?;
                }
            }
            Expr::Vector(values)
        } else {
            match self.tokens.get(self.cursor).cloned() {
                Some(Token {
                    kind: Kind::Number(n),
                    ..
                }) => {
                    self.cursor += 1;
                    Expr::Number(n)
                }
                Some(Token {
                    kind: Kind::String(s),
                    ..
                }) => {
                    self.cursor += 1;
                    Expr::String(s)
                }
                Some(Token {
                    kind: Kind::Ident(name),
                    ..
                }) => {
                    self.cursor += 1;
                    if self.symbol('(') {
                        let mut args = Vec::new();
                        if !self.symbol(')') {
                            loop {
                                let key = if matches!(
                                    self.tokens.get(self.cursor).map(|t| &t.kind),
                                    Some(Kind::Ident(_))
                                ) && self
                                    .tokens
                                    .get(self.cursor + 1)
                                    .is_some_and(|t| t.kind == Kind::Symbol(':'))
                                {
                                    let key = self.ident()?;
                                    self.require(':')?;
                                    Some(key)
                                } else {
                                    None
                                };
                                args.push((key, self.expr(0)?));
                                if self.symbol(')') {
                                    break;
                                }
                                self.require(',')?;
                            }
                        }
                        Expr::Call(name, args)
                    } else {
                        Expr::Name(name)
                    }
                }
                _ => return Err(self.failure("expected an expression")),
            }
        };
        let mut operations = 0;
        loop {
            let (op, precedence) = match self.tokens.get(self.cursor).map(|t| &t.kind) {
                Some(Kind::Symbol(op @ ('+' | '-'))) => (*op, 1),
                Some(Kind::Symbol(op @ ('*' | '/'))) => (*op, 2),
                _ => break,
            };
            if precedence < min_precedence {
                break;
            }
            operations += 1;
            if operations > 128 {
                return Err(self.failure("expression has too many operators"));
            }
            self.cursor += 1;
            let right = self.expr(precedence + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }
}

#[cfg(test)]
mod parser_tests {
    use super::*;
    #[test]
    fn parses_parameterized_assembly() {
        let source = r##"object table({ width: number, height: number }):
            top = box(size: [width, 0.15, 1], at: [0, height, 0], color: "#a66b42");
            leg = box(size: [0.12, height, 0.12]);
            place(leg, at: [-width / 2 + 0.1, height / 2, -0.4]);
        end
        place(table(width: 2, height: 1), at: [0, 0, 0]);"##;
        let tree = parse(source).unwrap();
        assert_eq!(tree.len(), 2);
        assert!(
            matches!(&tree[0], Statement::Definition { parameters, body, .. } if parameters.len() == 2 && body.len() == 3)
        );
    }
    #[test]
    fn malformed_input_has_location() {
        assert!(parse("object a({ x: number }):\n box(;\nend")
            .unwrap_err()
            .starts_with("2:6:"));
        assert!(parse("object a({}):\n")
            .unwrap_err()
            .contains("expected end"));
    }
}

use crate::model::{PrimitiveKind, SdfObject, SdfParams};
use glam::{Quat, Vec3, Vec4};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone)]
struct Part {
    path: String,
    object: SdfObject,
}
#[derive(Clone)]
enum Value {
    Number(f32),
    Text(String),
    List(Vec<Value>),
    Bool(bool),
    Bend(crate::model::SheetBend),
    Parts(Vec<Part>),
}
type Definitions = HashMap<String, (Vec<String>, Vec<Statement>)>;

fn number(value: Value) -> Result<f32, String> {
    match value {
        Value::Number(n) if n.is_finite() => Ok(n),
        _ => Err("expected a finite number".into()),
    }
}
fn vector(value: Value) -> Result<Vec3, String> {
    match value {
        Value::List(v) if v.len() == 3 => {
            let values = v.into_iter().map(number).collect::<Result<Vec<_>, _>>()?;
            Ok(Vec3::new(values[0], values[1], values[2]))
        }
        _ => Err("expected a vector of three finite numbers".into()),
    }
}
fn arguments(
    args: &[(Option<String>, Expr)],
    env: &HashMap<String, Value>,
    defs: &Definitions,
    depth: usize,
    source: &str,
) -> Result<(Vec<Value>, BTreeMap<String, Value>), String> {
    let mut positional = Vec::new();
    let mut named = BTreeMap::new();
    for (key, expr) in args {
        let value = evaluate(expr, env, defs, depth, source)?;
        if let Some(key) = key {
            if named.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate argument '{key}'"));
            }
        } else {
            if !named.is_empty() {
                return Err("positional arguments must precede named arguments".into());
            }
            positional.push(value);
        }
    }
    Ok((positional, named))
}
fn take_vec(args: &mut BTreeMap<String, Value>, name: &str, default: Vec3) -> Result<Vec3, String> {
    args.remove(name).map(vector).unwrap_or(Ok(default))
}
fn apply_transform(
    parts: &mut [Part],
    at: Vec3,
    rotation: Vec3,
    scale: Vec3,
) -> Result<(), String> {
    if !at.is_finite() || !rotation.is_finite() || !scale.is_finite() || scale.min_element() <= 0.0
    {
        return Err("transform must be finite with positive scale".into());
    }
    let q = Quat::from_euler(
        glam::EulerRot::XYZ,
        rotation.x.to_radians(),
        rotation.y.to_radians(),
        rotation.z.to_radians(),
    );
    for part in parts {
        // A nonuniform parent scale followed by a rotated child introduces shear.
        if (scale.x - scale.y).abs() > 1e-6 || (scale.x - scale.z).abs() > 1e-6 {
            if part.object.transform.rotation != Quat::IDENTITY {
                return Err(
                    "nonuniform assembly scale with rotated parts would introduce shear".into(),
                );
            }
        }
        part.object.transform.translation = at + q * (scale * part.object.transform.translation);
        part.object.transform.rotation = q * part.object.transform.rotation;
        part.object.transform.scale *= scale;
    }
    Ok(())
}
fn color(value: Value) -> Result<Vec4, String> {
    let Value::Text(s) = value else {
        return Err("color must be a #rrggbb string".into());
    };
    if s.len() != 7 || !s.starts_with('#') || !s[1..].bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("color must be a #rrggbb string".into());
    }
    let n = u32::from_str_radix(&s[1..], 16).map_err(|_| "invalid color")?;
    Ok(Vec4::new(
        ((n >> 16) & 255) as f32 / 255.,
        ((n >> 8) & 255) as f32 / 255.,
        (n & 255) as f32 / 255.,
        1.,
    ))
}
fn primitive(name: &str) -> Option<PrimitiveKind> {
    match name {
        "box" => Some(PrimitiveKind::Box),
        "sphere" => Some(PrimitiveKind::Sphere),
        "cylinder" => Some(PrimitiveKind::Cylinder),
        "torus" => Some(PrimitiveKind::Torus),
        "polygon_prism" => Some(PrimitiveKind::PolygonPrism),
        "bezier_curve" => Some(PrimitiveKind::BezierCurve),
        "loft" => Some(PrimitiveKind::Loft),
        "sheet_panel" => Some(PrimitiveKind::SheetPanel),
        "text" => Some(PrimitiveKind::Text),
        _ => None,
    }
}
fn evaluate(
    expr: &Expr,
    env: &HashMap<String, Value>,
    defs: &Definitions,
    depth: usize,
    source: &str,
) -> Result<Value, String> {
    if depth > 32 {
        return Err("assembly nesting exceeds 32 levels or is recursive".into());
    }
    match expr {
        Expr::Number(n) => Ok(Value::Number(*n)),
        Expr::String(s) => Ok(Value::Text(s.clone())),
        Expr::Name(name) if name == "true" => Ok(Value::Bool(true)),
        Expr::Name(name) if name == "false" => Ok(Value::Bool(false)),
        Expr::Name(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown name '{name}'")),
        Expr::Vector(items) => Ok(Value::List(
            items
                .iter()
                .map(|e| evaluate(e, env, defs, depth, source))
                .collect::<Result<_, _>>()?,
        )),
        Expr::Unary('-', x) => Ok(Value::Number(-number(evaluate(
            x, env, defs, depth, source,
        )?)?)),
        Expr::Unary(_, _) => Err("unknown unary operator".into()),
        Expr::Binary(op, a, b) => {
            let a = number(evaluate(a, env, defs, depth, source)?)?;
            let b = number(evaluate(b, env, defs, depth, source)?)?;
            let n = match op {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                '/' if b != 0.0 => a / b,
                '/' => return Err("division by zero".into()),
                _ => return Err("unknown operator".into()),
            };
            number(Value::Number(n)).map(Value::Number)
        }
        Expr::Call(name, args) => {
            let (pos, mut named) = arguments(args, env, defs, depth, source)?;
            if name == "bend" {
                if !pos.is_empty() {
                    return Err("bend accepts named arguments only".into());
                }
                let mut required = |key: &str| -> Result<f32, String> {
                    number(
                        named
                            .remove(key)
                            .ok_or_else(|| format!("bend requires {key}"))?,
                    )
                };
                let start = required("start")?;
                let radius = required("radius")?;
                let angle = required("angle")?.to_radians();
                let direction = named
                    .remove("direction")
                    .map(number)
                    .transpose()?
                    .unwrap_or(0.)
                    .to_radians();
                if let Some(key) = named.keys().next() {
                    return Err(format!("unknown bend argument '{key}'"));
                }
                return Ok(Value::Bend(crate::model::SheetBend {
                    start,
                    radius,
                    angle,
                    direction,
                }));
            }
            if let Some(kind) = primitive(name) {
                if !pos.is_empty() {
                    return Err(format!("{name} accepts named arguments only"));
                }
                let mut object = SdfObject::create_kind(kind);
                object.softness = 0.;
                object.name = name.clone();
                if kind == PrimitiveKind::BezierCurve {
                    object.path_extrusion = Some(crate::model::PathExtrusion::default());
                }
                if let Some(value) = named.remove("color") {
                    object.color = color(value)?;
                    object.material.color = object.color;
                }
                if let Some(value) = named.remove("size") {
                    match &mut object.params {
                        SdfParams::BoxParams(p) => {
                            let size = vector(value)?;
                            if size.min_element() <= 0. {
                                return Err("size components must be positive".into());
                            }
                            p.box_q = size * 0.5;
                        }
                        SdfParams::TextParams(p) => {
                            p.size = number(value)?;
                        }
                        SdfParams::SheetPanelParams(p) => {
                            let Value::List(v) = value else {
                                return Err("sheet size requires two numbers".into());
                            };
                            if v.len() != 2 {
                                return Err("sheet size requires two numbers".into());
                            }
                            let v = v.into_iter().map(number).collect::<Result<Vec<_>, _>>()?;
                            p.size = glam::Vec2::new(v[0], v[1]);
                        }
                        _ => return Err(format!("size is not supported for {name}")),
                    }
                }
                if let Some(value) = named.remove("radius") {
                    let r = number(value)?;
                    if r <= 0. {
                        return Err("radius must be positive".into());
                    }
                    match &mut object.params {
                        SdfParams::SphereParams(p) => p.radius = r,
                        SdfParams::CylinderParams { radius, .. } => *radius = r,
                        SdfParams::BezierCurveParams(_) => {
                            object.path_extrusion.as_mut().unwrap().radius = r
                        }
                        _ => return Err(format!("radius is not supported for {name}")),
                    }
                }
                if let Some(value) = named.remove("height") {
                    let h = number(value)?;
                    if h <= 0. {
                        return Err("height must be positive".into());
                    }
                    match &mut object.params {
                        SdfParams::CylinderParams { half_height, .. } => *half_height = h * 0.5,
                        _ => return Err(format!("height is not supported for {name}")),
                    }
                }
                if let Some(value) = named.remove("corner_radius") {
                    match &mut object.params {
                        SdfParams::BoxParams(p) => p.corner_radius = number(value)?,
                        _ => return Err("corner_radius is supported on box".into()),
                    }
                }
                for field in ["major_radius", "minor_radius"] {
                    if let Some(value) = named.remove(field) {
                        let radius = number(value)?;
                        if radius <= 0.0 {
                            return Err(format!("{field} must be positive"));
                        }
                        match &mut object.params {
                            SdfParams::TorusParams {
                                major_radius,
                                minor_radius,
                            } => {
                                if field == "major_radius" {
                                    *major_radius = radius
                                } else {
                                    *minor_radius = radius
                                }
                            }
                            _ => return Err(format!("{field} is supported on torus")),
                        }
                    }
                }
                if let Some(value) = named.remove("text") {
                    match (&mut object.params, value) {
                        (SdfParams::TextParams(p), Value::Text(text)) => p.text = text,
                        _ => return Err("text requires a string on text".into()),
                    }
                }
                for field in ["depth", "tracking", "line_spacing"] {
                    if let Some(value) = named.remove(field) {
                        let value = number(value)?;
                        match &mut object.params {
                            SdfParams::TextParams(p) => match field {
                                "depth" => p.half_depth = value * 0.5,
                                "tracking" => p.tracking = value,
                                _ => p.line_spacing = value,
                            },
                            SdfParams::PolygonPrismParams(p) if field == "depth" && value > 0.0 => {
                                p.half_depth = value * 0.5
                            }
                            _ => return Err(format!("{field} is not supported for {name}")),
                        }
                    }
                }
                if let Some(value) = named.remove("vertices") {
                    match (&mut object.params, value) {
                        (SdfParams::PolygonPrismParams(p), Value::List(vertices)) => {
                            p.vertices = vertices
                                .into_iter()
                                .map(|v| match v {
                                    Value::List(v) if v.len() == 2 => {
                                        let values = v
                                            .into_iter()
                                            .map(number)
                                            .collect::<Result<Vec<_>, _>>()?;
                                        Ok(glam::Vec2::new(values[0], values[1]))
                                    }
                                    _ => Err("vertices must contain 2D points".to_string()),
                                })
                                .collect::<Result<_, _>>()?;
                            if !(3..=32).contains(&p.vertices.len()) {
                                return Err("polygon_prism requires 3–32 vertices".into());
                            }
                        }
                        _ => return Err("vertices is supported on polygon_prism".into()),
                    }
                }
                if let Some(value) = named.remove("thickness") {
                    match &mut object.params {
                        SdfParams::SheetPanelParams(p) => p.thickness = number(value)?,
                        _ => return Err(format!("thickness is not supported for {name}")),
                    }
                }
                if let Some(value) = named.remove("bends") {
                    match (&mut object.params, value) {
                        (SdfParams::SheetPanelParams(p), Value::List(v)) => {
                            p.bends = v
                                .into_iter()
                                .map(|v| match v {
                                    Value::Bend(b) => Ok(b),
                                    _ => Err("bends must contain bend(...) values".to_owned()),
                                })
                                .collect::<Result<_, _>>()?;
                        }
                        _ => return Err("bends requires a list on sheet_panel".into()),
                    }
                }
                if let Some(value) = named.remove("profile") {
                    match (object.path_extrusion.as_mut(), value) {
                        (Some(extrusion), Value::Text(profile)) if profile == "round" => {
                            extrusion.profile = crate::model::BezierProfile::Round
                        }
                        (Some(extrusion), Value::Text(profile)) if profile == "square" => {
                            extrusion.profile = crate::model::BezierProfile::Square
                        }
                        _ => {
                            return Err(
                                "profile requires \"round\" or \"square\" on bezier_curve".into()
                            )
                        }
                    }
                }
                if let Some(value) = named.remove("points") {
                    match (&mut object.params, value) {
                        (SdfParams::BezierCurveParams(p), Value::List(v)) => {
                            p.points = v.into_iter().map(vector).collect::<Result<_, _>>()?;
                            if p.points.len() < 4
                                || p.points.len() > 25
                                || (p.points.len() - 1) % 3 != 0
                            {
                                return Err(
                                    "bezier_curve requires 4, 7, ... 25 control points".into()
                                );
                            }
                        }
                        _ => return Err("points requires a list on bezier_curve".into()),
                    }
                }
                if let Some(value) = named.remove("closed") {
                    match (&mut object.params, value) {
                        (SdfParams::BezierCurveParams(p), Value::Bool(v)) => p.closed = v,
                        _ => return Err("closed requires a boolean on bezier_curve".into()),
                    }
                }
                if let SdfParams::SheetPanelParams(p) = &object.params {
                    p.validate()?;
                }
                match &object.params {
                    SdfParams::TextParams(p) => p.validate()?,
                    SdfParams::BoxParams(p) if p.corner_radius < 0.0 => {
                        return Err("corner_radius must be nonnegative".into())
                    }
                    _ => {}
                }
                let at = take_vec(&mut named, "at", Vec3::ZERO)?;
                let rot = take_vec(&mut named, "rotation", Vec3::ZERO)?;
                let scale = take_vec(&mut named, "scale", Vec3::ONE)?;
                if let Some(key) = named.keys().next() {
                    return Err(format!("unknown {name} argument '{key}'"));
                }
                let mut parts = vec![Part {
                    path: String::new(),
                    object,
                }];
                apply_transform(&mut parts, at, rot, scale)?;
                Ok(Value::Parts(parts))
            } else if let Some((params, body)) = defs.get(name) {
                if !pos.is_empty() {
                    return Err(format!("{name} accepts named arguments only"));
                }
                let mut local = HashMap::new();
                for param in params {
                    let v = named
                        .remove(param)
                        .ok_or_else(|| format!("missing parameter '{param}' for {name}"))?;
                    local.insert(param.clone(), Value::Number(number(v)?));
                }
                if let Some(key) = named.keys().next() {
                    return Err(format!("unknown parameter '{key}' for {name}"));
                }
                Ok(Value::Parts(run_body(
                    body,
                    &mut local,
                    defs,
                    depth + 1,
                    source,
                )?))
            } else {
                Err(format!("unknown function '{name}'"))
            }
        }
    }
}
fn run_body(
    body: &[Statement],
    env: &mut HashMap<String, Value>,
    defs: &Definitions,
    depth: usize,
    source: &str,
) -> Result<Vec<Part>, String> {
    let mut result = Vec::new();
    let mut declarations = Vec::new();
    let mut referenced = HashSet::new();
    for (index, stmt) in body.iter().enumerate() {
        let offset = match stmt {
            Statement::Definition { offset, .. }
            | Statement::Assignment { offset, .. }
            | Statement::Expression { offset, .. } => *offset,
        };
        let result_for_statement = (|| -> Result<(), String> {
            match stmt {
                Statement::Definition { .. } => {
                    return Err("object definitions must be top level".into())
                }
                Statement::Assignment { name, value, .. } => {
                    if env.contains_key(name) {
                        return Err(format!("duplicate name '{name}'"));
                    }
                    let v = evaluate(value, env, defs, depth, source)?;
                    env.insert(name.clone(), v);
                    declarations.push(name.clone());
                }
                Statement::Expression {
                    value: Expr::Call(name, args),
                    ..
                } if name == "place" => {
                    if let Some((None, Expr::Name(name))) = args.first() {
                        referenced.insert(name.clone());
                    }
                    let (mut pos, mut named) = arguments(args, env, defs, depth, source)?;
                    if pos.len() != 1 {
                        return Err("place requires one positional object".into());
                    }
                    let Value::Parts(mut parts) = pos.remove(0) else {
                        return Err("place requires an object or assembly".into());
                    };
                    let at = take_vec(&mut named, "at", Vec3::ZERO)?;
                    let rotation = take_vec(&mut named, "rotation", Vec3::ZERO)?;
                    let scale = take_vec(&mut named, "scale", Vec3::ONE)?;
                    if let Some(key) = named.keys().next() {
                        return Err(format!("unknown place argument '{key}'"));
                    }
                    apply_transform(&mut parts, at, rotation, scale)?;
                    for p in &mut parts {
                        p.path = format!("place{index}/{}", p.path);
                    }
                    result.extend(parts);
                }
                Statement::Expression { .. } => {
                    return Err("only place(...) may be used as a statement".into())
                }
            }
            Ok(())
        })();
        result_for_statement.map_err(|message| error(source, offset, &message))?;
        if result.len() > 10000 {
            return Err("scene exceeds 10000 objects".into());
        }
    }
    for name in declarations {
        if !referenced.contains(&name) {
            if let Some(Value::Parts(parts)) = env.get(&name) {
                for part in parts {
                    let mut p = part.clone();
                    p.path = format!("{name}/{}", p.path);
                    p.object.name = name.clone();
                    result.push(p);
                }
            }
        }
    }
    if result.len() > 10000 {
        return Err("scene exceeds 10000 objects".into());
    }
    Ok(result)
}

/// Compile a complete scene recipe into deterministic part paths and editable native objects.
/// This function does not access files, create a GPU, or mutate application state.
pub fn compile(source: &str) -> Result<Vec<(String, SdfObject)>, String> {
    if source.len() > 1_000_000 {
        return Err("source exceeds 1 MB".into());
    }
    let syntax = parse(source)?;
    let mut defs = Definitions::new();
    let mut body = Vec::new();
    for stmt in syntax {
        match stmt {
            Statement::Definition {
                name,
                parameters,
                body,
                offset,
            } => {
                if primitive(&name).is_some()
                    || defs.insert(name.clone(), (parameters, body)).is_some()
                {
                    return Err(error(
                        source,
                        offset,
                        &format!("duplicate or reserved definition '{name}'"),
                    ));
                }
            }
            other => body.push(other),
        }
    }
    let parts = run_body(&body, &mut HashMap::new(), &defs, 0, source)?;
    let mut ids = HashSet::new();
    let mut output = Vec::new();
    for mut part in parts {
        if !part.object.transform.translation.is_finite()
            || !part.object.transform.rotation.is_finite()
            || !part.object.transform.scale.is_finite()
            || part.object.transform.scale.min_element() <= 0.0
        {
            return Err(format!("invalid expanded transform at '{}'", part.path));
        }
        let digest = Sha256::digest(format!("claydash-scene-dsl-v1:{}", part.path).as_bytes());
        let mut bytes = [0; 16];
        bytes.copy_from_slice(&digest[..16]);
        bytes[6] = (bytes[6] & 15) | 0x50;
        bytes[8] = (bytes[8] & 63) | 0x80;
        part.object.uuid = uuid::Uuid::from_bytes(bytes);
        if !ids.insert(part.object.uuid) {
            return Err(format!("duplicate object path '{}'", part.path));
        }
        output.push((part.path, part.object));
    }
    if output
        .iter()
        .any(|(_, object)| matches!(object.params, SdfParams::TextParams(_)))
    {
        crate::model::validate_scene_text_budget(
            &output
                .iter()
                .map(|(_, object)| object.clone())
                .collect::<Vec<_>>(),
        )?;
    }
    Ok(output)
}

/// Axis-aligned world bounds of the generated geometry, including formed sheets and curve extrusion.
pub fn bounds(objects: &[SdfObject]) -> Result<(Vec3, Vec3), String> {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for object in objects {
        if let Some((lo, hi)) = crate::model::lattice_bounds(objects, object.uuid) {
            let matrix = crate::model::lattice_world_matrix(objects, object.uuid);
            for x in [lo.x, hi.x] {
                for y in [lo.y, hi.y] {
                    for z in [lo.z, hi.z] {
                        let point = matrix.transform_point3(Vec3::new(x, y, z));
                        min = min.min(point);
                        max = max.max(point);
                    }
                }
            }
        }
    }
    if !min.is_finite() || !max.is_finite() {
        return Err("objects have no finite render bounds".into());
    }
    Ok((min, max))
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    const TABLE: &str = r##"
object table({ width: number, height: number }):
 top = box(size: [width, 0.15, 1], at: [0, height, 0], color: "#a66b42");
 leg = box(size: [0.12, height, 0.12], color: "#70452e");
 place(leg, at: [-width / 2 + 0.1, height / 2, -0.4]);
 place(leg, at: [ width / 2 - 0.1, height / 2, -0.4]);
 place(leg, at: [-width / 2 + 0.1, height / 2,  0.4]);
 place(leg, at: [ width / 2 - 0.1, height / 2,  0.4]);
end
place(table(width: 2, height: 1), at: [5, 0, 0]);
"##;
    #[test]
    fn assemblies_expand_and_keep_ids_across_parameter_changes() {
        let a = compile(TABLE).unwrap();
        let b = compile(&TABLE.replace("width: 2, height: 1", "width: 3, height: 1")).unwrap();
        assert_eq!(a.len(), 5);
        assert_eq!(
            a.iter().map(|(p, o)| (p, o.uuid)).collect::<Vec<_>>(),
            b.iter().map(|(p, o)| (p, o.uuid)).collect::<Vec<_>>()
        );
        assert!(a.iter().all(|(_, o)| o.transform.translation.x > 3.));
        assert_eq!(a.iter().filter(|(_, o)| o.name == "top").count(), 1);
    }
    #[test]
    fn curves_and_sheet_bends_use_native_parameters() {
        let parts=compile("curve = bezier_curve(points: [[0,0,0], [1,0,0], [1,1,0], [2,1,0]], closed: false); panel = sheet_panel(size: [2,1], thickness: 0.04, bends: [bend(start: 0, radius: 0.3, angle: 45, direction: 0)]);").unwrap();
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().any(
            |(_, o)| matches!(&o.params,SdfParams::BezierCurveParams(p) if p.points.len()==4)
                && o.path_extrusion.is_some()
        ));
        assert!(parts.iter().any(|(_,o)|matches!(&o.params,SdfParams::SheetPanelParams(p) if (p.bends[0].angle-std::f32::consts::FRAC_PI_4).abs()<1e-6)));
        assert!(compile("a = bezier_curve(points: [[0,0,0]]);").is_err());
        assert!(
            compile("a = sheet_panel(bends: [bend(start: 0, radius: 0, angle: 45)]);").is_err()
        );
    }
    #[test]
    fn named_shape_parameters_map_to_native_geometry() {
        let objects=compile(r#"ring = torus(major_radius: 2, minor_radius: 0.2); label = text(text: "Hello", size: 1, depth: 0.4); prism = polygon_prism(vertices: [[0,0], [2,0], [0,1]], depth: 0.5);"#).unwrap();
        assert!(objects.iter().any(|(_,o)| matches!(&o.params, SdfParams::TorusParams {major_radius,minor_radius} if *major_radius==2. && *minor_radius==0.2)));
        assert!(objects.iter().any(|(_,o)| matches!(&o.params, SdfParams::TextParams(p) if p.text=="Hello" && p.size==1. && p.half_depth==0.2)));
        assert!(objects.iter().any(|(_,o)| matches!(&o.params, SdfParams::PolygonPrismParams(p) if p.vertices.len()==3 && p.half_depth==0.25)));
    }
    #[test]
    fn reusable_compiler_validates_geometry_without_an_application() {
        assert!(compile("part = box(corner_radius: -1);").is_err());
        assert!(compile("part = text(text: \"\");").is_err());
        let objects = compile("part = box(size: [2,1,1], at: [12,3,5]);")
            .unwrap()
            .into_iter()
            .map(|(_, object)| object)
            .collect::<Vec<_>>();
        let (min, max) = bounds(&objects).unwrap();
        assert_eq!((min + max) * 0.5, Vec3::new(12., 3., 5.));
        assert!(bounds(&[]).is_err());
    }
    #[test]
    fn recursive_and_bad_calls_fail() {
        assert!(compile("object loop({}): place(loop()); end place(loop());").is_err());
        assert!(compile("a = box(size: [1,1,1], mystery: 2);").is_err());
        assert!(compile("a = box(size: [1 / 0,1,1]);").is_err());
        assert!(compile("# bad\na = box(size: [0,1,1]);")
            .err()
            .unwrap()
            .starts_with("2:1:"));
    }
}
