//! TeX math as readable text: Greek letters, operators, arrows and relations
//! as their symbols, `^`/`_` as raised and lowered characters where Unicode
//! has them, `\frac{a}{b}` as a/b, `\sqrt{x}` as √x. Not a typesetter, but a
//! formula reads as one instead of as source.

fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ", "epsilon" => "ε", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η",
        "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ", "lambda" => "λ", "mu" => "μ", "nu" => "ν", "xi" => "ξ",
        "pi" => "π", "varpi" => "ϖ", "rho" => "ρ", "sigma" => "σ", "varsigma" => "ς", "tau" => "τ", "upsilon" => "υ", "phi" => "φ",
        "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ", "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        "sum" => "∑", "prod" => "∏", "int" => "∫", "iint" => "∬", "oint" => "∮", "partial" => "∂", "nabla" => "∇", "infty" => "∞",
        "cdot" => "·", "times" => "×", "div" => "÷", "pm" => "±", "mp" => "∓", "ast" => "∗", "circ" => "∘", "bullet" => "•",
        "leq" => "≤", "le" => "≤", "geq" => "≥", "ge" => "≥", "neq" => "≠", "ne" => "≠", "approx" => "≈", "equiv" => "≡", "sim" => "∼",
        "simeq" => "≃", "cong" => "≅", "propto" => "∝", "ll" => "≪", "gg" => "≫",
        "in" => "∈", "notin" => "∉", "ni" => "∋", "subset" => "⊂", "supset" => "⊃", "subseteq" => "⊆", "supseteq" => "⊇",
        "cup" => "∪", "cap" => "∩", "emptyset" => "∅", "varnothing" => "∅", "forall" => "∀", "exists" => "∃", "neg" => "¬",
        "land" => "∧", "wedge" => "∧", "lor" => "∨", "vee" => "∨", "oplus" => "⊕", "otimes" => "⊗",
        "to" => "→", "rightarrow" => "→", "leftarrow" => "←", "gets" => "←", "leftrightarrow" => "↔", "Rightarrow" => "⇒",
        "Leftarrow" => "⇐", "Leftrightarrow" => "⇔", "implies" => "⇒", "iff" => "⇔", "mapsto" => "↦", "uparrow" => "↑", "downarrow" => "↓",
        "ldots" => "…", "dots" => "…", "cdots" => "⋯", "vdots" => "⋮", "ddots" => "⋱", "prime" => "′", "degree" => "°",
        "angle" => "∠", "perp" => "⊥", "parallel" => "∥", "hbar" => "ℏ", "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ", "aleph" => "ℵ",
        "mathbb{R}" => "ℝ", "mathbb{N}" => "ℕ", "mathbb{Z}" => "ℤ", "mathbb{Q}" => "ℚ", "mathbb{C}" => "ℂ",
        "langle" => "⟨", "rangle" => "⟩", "lfloor" => "⌊", "rfloor" => "⌋", "lceil" => "⌈", "rceil" => "⌉", "|" => "‖",
        "quad" => "  ", "qquad" => "    ", "," => "\u{2009}", ";" => " ", ":" => " ", "!" => "", " " => " ",
        "{" => "{", "}" => "}", "%" => "%", "$" => "$", "&" => "&", "#" => "#", "_" => "_",
        "sin" => "sin", "cos" => "cos", "tan" => "tan", "log" => "log", "ln" => "ln", "exp" => "exp", "lim" => "lim", "max" => "max",
        "min" => "min", "det" => "det", "sup" => "sup", "inf" => "inf", "arg" => "arg",
        "left" | "right" | "big" | "Big" | "bigg" | "Bigg" | "displaystyle" | "textstyle" | "limits" | "nolimits" => "",
        _ => return None,
    })
}

fn sup(c: char) -> Option<char> {
    Some(match c {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴', '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾', 'n' => 'ⁿ', 'i' => 'ⁱ', 'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ',
        'd' => 'ᵈ', 'e' => 'ᵉ', 'f' => 'ᶠ', 'g' => 'ᵍ', 'h' => 'ʰ', 'j' => 'ʲ', 'k' => 'ᵏ', 'l' => 'ˡ', 'm' => 'ᵐ', 'o' => 'ᵒ',
        'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ', 't' => 'ᵗ', 'u' => 'ᵘ', 'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ',
        'T' => 'ᵀ', '′' => '′', '*' => '*',
        _ => return None,
    })
}

fn sub(c: char) -> Option<char> {
    Some(match c {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄', '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' => '₋', '=' => '₌', '(' => '₍', ')' => '₎', 'a' => 'ₐ', 'e' => 'ₑ', 'h' => 'ₕ', 'i' => 'ᵢ', 'j' => 'ⱼ',
        'k' => 'ₖ', 'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'o' => 'ₒ', 'p' => 'ₚ', 'r' => 'ᵣ', 's' => 'ₛ', 't' => 'ₜ', 'u' => 'ᵤ',
        'v' => 'ᵥ', 'x' => 'ₓ',
        _ => return None,
    })
}

struct P<'a> {
    s: &'a [char],
    i: usize,
}

impl P<'_> {
    /// One argument: a `{group}`, a `\command` or a single character.
    fn arg(&mut self) -> String {
        while self.i < self.s.len() && self.s[self.i] == ' ' {
            self.i += 1;
        }
        if self.i >= self.s.len() {
            return String::new();
        }
        if self.s[self.i] == '{' {
            self.i += 1;
            let start = self.i;
            let mut depth = 1;
            while self.i < self.s.len() {
                match self.s[self.i] {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                self.i += 1;
            }
            let inner: String = self.s[start..self.i.min(self.s.len())].iter().collect();
            self.i += 1;
            return convert(&inner);
        }
        if self.s[self.i] == '\\' {
            let start = self.i;
            self.i += 1;
            while self.i < self.s.len() && self.s[self.i].is_ascii_alphabetic() {
                self.i += 1;
            }
            if self.i == start + 1 && self.i < self.s.len() {
                self.i += 1;
            }
            let cmd: String = self.s[start..self.i].iter().collect();
            return convert(&cmd);
        }
        let c = self.s[self.i];
        self.i += 1;
        c.to_string()
    }
}

/// Raised or lowered text: Unicode characters when all of it has them,
/// else `^(…)` / `_(…)`.
fn script(text: &str, up: bool) -> String {
    let mapped: Option<String> = text.chars().map(|c| if up { sup(c) } else { sub(c) }).collect();
    match mapped {
        Some(m) => m,
        None if text.chars().count() == 1 => format!("{}{text}", if up { "^" } else { "_" }),
        None => format!("{}({text})", if up { "^" } else { "_" }),
    }
}

fn wrap(t: &str) -> String {
    if t.chars().count() <= 1 || t.chars().all(|c| c.is_alphanumeric()) { t.to_string() } else { format!("({t})") }
}

/// TeX source → readable Unicode text.
pub fn convert(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut p = P { s: &chars, i: 0 };
    let mut out = String::new();
    while p.i < chars.len() {
        let c = chars[p.i];
        match c {
            '\\' => {
                let start = p.i;
                p.i += 1;
                while p.i < chars.len() && chars[p.i].is_ascii_alphabetic() {
                    p.i += 1;
                }
                if p.i == start + 1 && p.i < chars.len() {
                    p.i += 1;
                }
                let name: String = chars[start + 1..p.i].iter().collect();
                match name.as_str() {
                    "frac" | "dfrac" | "tfrac" => {
                        let a = p.arg();
                        let b = p.arg();
                        out.push_str(&format!("{}/{}", wrap(&a), wrap(&b)));
                    }
                    "sqrt" => {
                        let a = p.arg();
                        out.push('√');
                        out.push_str(&wrap(&a));
                    }
                    "text" | "mathrm" | "mathit" | "mathbf" | "operatorname" | "textbf" | "textit" | "mathsf" | "mathcal" => out.push_str(&p.arg()),
                    "mathbb" => {
                        let a = p.arg();
                        out.push_str(symbol(&format!("mathbb{{{a}}}")).unwrap_or(&a));
                    }
                    "overline" | "bar" => {
                        let a = p.arg();
                        for ch in a.chars() {
                            out.push(ch);
                            out.push('\u{0305}');
                        }
                    }
                    "hat" | "widehat" => {
                        out.push_str(&p.arg());
                        out.push('\u{0302}');
                    }
                    "vec" => {
                        out.push_str(&p.arg());
                        out.push('\u{20D7}');
                    }
                    "dot" => {
                        out.push_str(&p.arg());
                        out.push('\u{0307}');
                    }
                    "tilde" => {
                        out.push_str(&p.arg());
                        out.push('\u{0303}');
                    }
                    "\\" => out.push('\n'),
                    "begin" | "end" => {
                        let _ = p.arg();
                    }
                    n => match symbol(n) {
                        Some(sym) => {
                            out.push_str(sym);
                            // a function name keeps a space before its argument
                            if n.chars().all(|c| c.is_ascii_lowercase()) && sym.chars().all(|c| c.is_ascii_lowercase()) && !sym.is_empty() && p.i < chars.len() && chars[p.i] != ' ' {
                                out.push(' ');
                            }
                        }
                        None => out.push_str(n),
                    },
                }
            }
            '^' | '_' => {
                p.i += 1;
                let a = p.arg();
                out.push_str(&script(&a, c == '^'));
            }
            '{' | '}' => p.i += 1,
            '&' => {
                out.push(' ');
                p.i += 1;
            }
            '~' => {
                out.push(' ');
                p.i += 1;
            }
            _ => {
                out.push(c);
                p.i += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formulas_read() {
        assert_eq!(convert(r"x^2 + y_1"), "x² + y₁");
        assert_eq!(convert(r"\frac{a}{b}"), "a/b");
        assert_eq!(convert(r"\frac{1}{x+1}"), "1/(x+1)");
        assert_eq!(convert(r"\alpha \leq \sqrt{\beta}"), "α ≤ √β");
        assert_eq!(convert(r"\int_0^1 x^2\,dx"), "∫₀¹ x²\u{2009}dx");
        assert_eq!(convert(r"e^{i\pi} = -1"), "e^(iπ) = -1");
        assert_eq!(convert(r"\sin x"), "sin x");
        assert_eq!(convert(r"\sin\theta"), "sin θ");
    }
}
