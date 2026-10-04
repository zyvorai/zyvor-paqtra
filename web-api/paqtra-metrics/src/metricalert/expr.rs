//! calc/warn/crit expressions: a small subset of Netdata's language.
//! Numbers, `$variables`, `+ - * / %`, comparisons, `&& || !` (also `and`,
//! `or`, `not`), the ternary `?:`, parentheses and `abs()/min()/max()`. Any
//! comparison involving NaN is false, so an expression over missing data
//! never raises an alert.

#[derive(Debug, Clone)]
enum Node {
    Num(f64),
    Var(String),
    Unary(char, Box<Node>),
    Binary(&'static str, Box<Node>, Box<Node>),
    Ternary(Box<Node>, Box<Node>, Box<Node>),
    Call(&'static str, Vec<Node>),
}

pub(crate) fn truth(v: f64) -> bool {
    !v.is_nan() && v != 0.0
}

fn b2f(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

impl Node {
    fn eval(&self, vars: &dyn Fn(&str) -> f64) -> f64 {
        match self {
            Node::Num(n) => *n,
            Node::Var(v) => vars(v),
            Node::Unary('!', x) => b2f(!truth(x.eval(vars))),
            Node::Unary(_, x) => -x.eval(vars),
            Node::Ternary(c, t, f) => {
                if truth(c.eval(vars)) {
                    t.eval(vars)
                } else {
                    f.eval(vars)
                }
            }
            Node::Call(f, a) => {
                let a: Vec<f64> = a.iter().map(|x| x.eval(vars)).collect();
                match *f {
                    "abs" => a[0].abs(),
                    "min" => a[0].min(a[1]),
                    _ => a[0].max(a[1]),
                }
            }
            Node::Binary(op, l, r) => {
                match *op {
                    "&&" => return b2f(truth(l.eval(vars)) && truth(r.eval(vars))),
                    "||" => return b2f(truth(l.eval(vars)) || truth(r.eval(vars))),
                    _ => {}
                }
                let (l, r) = (l.eval(vars), r.eval(vars));
                match *op {
                    "+" => return l + r,
                    "-" => return l - r,
                    "*" => return l * r,
                    "/" => return if r == 0.0 { f64::NAN } else { l / r },
                    "%" => return if r == 0.0 { f64::NAN } else { l % r },
                    _ => {}
                }
                if l.is_nan() || r.is_nan() {
                    return b2f(*op == "!=" && l.is_nan() != r.is_nan());
                }
                b2f(match *op {
                    ">" => l > r,
                    ">=" => l >= r,
                    "<" => l < r,
                    "<=" => l <= r,
                    "==" => l == r,
                    _ => l != r,
                })
            }
        }
    }
}

/// A compiled expression.
#[derive(Debug, Clone)]
pub struct Expr {
    src: String,
    root: Node,
}

impl Expr {
    /// Unknown variables should evaluate to NaN.
    pub fn eval(&self, vars: &dyn Fn(&str) -> f64) -> f64 {
        self.root.eval(vars)
    }

    pub fn source(&self) -> &str {
        &self.src
    }
}

/// Parses `src`. An empty source gives `None`.
pub fn compile(src: &str) -> Result<Option<Expr>, String> {
    let src = src.trim();
    if src.is_empty() {
        return Ok(None);
    }
    let toks = lex(src)?;
    let mut p = Parser {
        toks: &toks,
        pos: 0,
    };
    let root = p.expr(0).map_err(|e| format!("{src:?}: {e}"))?;
    if p.pos != toks.len() {
        return Err(format!("{src:?}: unexpected {:?}", toks[p.pos]));
    }
    Ok(Some(Expr {
        src: src.into(),
        root,
    }))
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn lex(s: &str) -> Result<Vec<String>, String> {
    let b: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '$' {
            let mut j = i + 1;
            if b.get(j) == Some(&'{') {
                let k = b[j..]
                    .iter()
                    .position(|&x| x == '}')
                    .ok_or("unterminated ${")?;
                out.push(format!("${}", b[j + 1..j + k].iter().collect::<String>()));
                i = j + k + 1;
                continue;
            }
            while j < b.len() && is_ident(b[j]) {
                j += 1;
            }
            if j == i + 1 {
                return Err(format!("empty variable at {i}"));
            }
            out.push(b[i..j].iter().collect());
            i = j;
        } else if c.is_ascii_digit() || c == '.' {
            let mut j = i;
            while j < b.len()
                && (b[j].is_ascii_digit()
                    || b[j] == '.'
                    || b[j] == 'e'
                    || b[j] == 'E'
                    || ((b[j] == '-' || b[j] == '+')
                        && j > i
                        && (b[j - 1] == 'e' || b[j - 1] == 'E')))
            {
                j += 1;
            }
            out.push(b[i..j].iter().collect());
            i = j;
        } else if c.is_alphabetic() {
            let mut j = i;
            while j < b.len() && is_ident(b[j]) {
                j += 1;
            }
            out.push(b[i..j].iter().collect::<String>().to_lowercase());
            i = j;
        } else {
            if i + 1 < b.len() {
                let two: String = b[i..i + 2].iter().collect();
                if [">=", "<=", "==", "!=", "&&", "||"].contains(&two.as_str()) {
                    out.push(two);
                    i += 2;
                    continue;
                }
            }
            if "+-*/%()<>!?:,".contains(c) {
                out.push(c.to_string());
                i += 1;
                continue;
            }
            return Err(format!("unexpected character {c:?}"));
        }
    }
    Ok(out)
}

struct Parser<'a> {
    toks: &'a [String],
    pos: usize,
}

const OPS: [&str; 14] = [
    "?", "||", "&&", "==", "!=", ">", ">=", "<", "<=", "+", "-", "*", "/", "%",
];

fn precedence(op: &str) -> Option<(&'static str, u8)> {
    let op = match op {
        "and" => "&&",
        "or" => "||",
        o => o,
    };
    let canon = OPS.iter().find(|o| **o == op)?;
    let p = match op {
        "?" => 1,
        "||" => 2,
        "&&" => 3,
        "==" | "!=" => 4,
        ">" | ">=" | "<" | "<=" => 5,
        "+" | "-" => 6,
        _ => 7,
    };
    Some((canon, p))
}

impl Parser<'_> {
    fn peek(&self) -> &str {
        self.toks.get(self.pos).map(String::as_str).unwrap_or("")
    }

    fn next(&mut self) -> String {
        let t = self.peek().to_string();
        self.pos += 1;
        t
    }

    fn expr(&mut self, min_prec: u8) -> Result<Node, String> {
        let mut left = self.unary()?;
        loop {
            let Some((op, prec)) = precedence(self.peek()) else {
                return Ok(left);
            };
            if prec <= min_prec {
                return Ok(left);
            }
            self.next();
            if op == "?" {
                let t = self.expr(0)?;
                if self.next() != ":" {
                    return Err("ternary without ':'".into());
                }
                let f = self.expr(prec - 1)?;
                left = Node::Ternary(Box::new(left), Box::new(t), Box::new(f));
                continue;
            }
            let right = self.expr(prec)?;
            left = Node::Binary(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Node, String> {
        let t = self.next();
        match t.as_str() {
            "" => Err("unexpected end of expression".into()),
            "-" => Ok(Node::Unary('-', Box::new(self.unary()?))),
            "!" | "not" => Ok(Node::Unary('!', Box::new(self.unary()?))),
            "+" => self.unary(),
            "(" => {
                let n = self.expr(0)?;
                if self.next() != ")" {
                    return Err("missing ')'".into());
                }
                Ok(n)
            }
            "abs" | "min" | "max" => {
                let f: &'static str = match t.as_str() {
                    "abs" => "abs",
                    "min" => "min",
                    _ => "max",
                };
                if self.next() != "(" {
                    return Err(format!("{f} needs '('"));
                }
                let mut args = Vec::new();
                loop {
                    args.push(self.expr(0)?);
                    match self.next().as_str() {
                        ")" => break,
                        "," => {}
                        _ => return Err(format!("bad argument list for {f}")),
                    }
                }
                let want = if f == "abs" { 1 } else { 2 };
                if args.len() != want {
                    return Err(format!("{f} takes {want} argument(s)"));
                }
                Ok(Node::Call(f, args))
            }
            "nan" => Ok(Node::Num(f64::NAN)),
            "inf" => Ok(Node::Num(f64::INFINITY)),
            v if v.starts_with('$') => Ok(Node::Var(v[1..].to_string())),
            v => v
                .parse()
                .map(Node::Num)
                .map_err(|_| format!("unexpected {v:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(src: &str, vars: &[(&str, f64)]) -> f64 {
        let e = compile(src).unwrap().unwrap();
        e.eval(&|n| {
            vars.iter()
                .find(|(k, _)| *k == n)
                .map_or(f64::NAN, |(_, v)| *v)
        })
    }

    #[test]
    fn arithmetic_precedence_and_functions() {
        assert_eq!(ev("1 + 2 * 3", &[]), 7.0);
        assert_eq!(ev("(1 + 2) * 3", &[]), 9.0);
        assert_eq!(ev("-2 + abs(-3) + max(1, 4) - min(2, 5)", &[]), 3.0);
        assert_eq!(ev("10 % 4", &[]), 2.0);
        assert!(ev("1 / 0", &[]).is_nan());
        assert_eq!(ev("1e3 + 2.5E-1", &[]), 1000.25);
    }

    #[test]
    fn hysteresis_ternary_and_nan() {
        let warn = "$this > (($status >= $WARNING) ? 75 : 85)";
        assert_eq!(
            ev(warn, &[("this", 80.0), ("status", 1.0), ("WARNING", 3.0)]),
            0.0
        );
        assert_eq!(
            ev(warn, &[("this", 80.0), ("status", 3.0), ("WARNING", 3.0)]),
            1.0
        );
        assert_eq!(ev("$missing > 0", &[]), 0.0);
        assert_eq!(ev("$missing != 1", &[]), 1.0);
        assert_eq!(
            ev(
                "${this} > 1 and not ($x == 2) or 0",
                &[("this", 2.0), ("x", 3.0)]
            ),
            1.0
        );
        assert_eq!(ev("$a ? 1 : $b ? 2 : 3", &[("a", 0.0), ("b", 1.0)]), 2.0);
    }

    #[test]
    fn errors() {
        assert!(compile("").unwrap().is_none());
        for bad in [
            "1 +",
            "(1",
            "$",
            "abs(1, 2)",
            "1 ? 2",
            "foo",
            "1 # 2",
            "${x",
        ] {
            assert!(compile(bad).is_err(), "{bad}");
        }
    }
}
