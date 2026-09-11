//! LaTeX 명령 → 유니코드 기호 표.

/// 위 첨자용 글자.
pub fn superscript(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '⁰', '1' => '¹', '2' => '²', '3' => '³', '4' => '⁴',
        '5' => '⁵', '6' => '⁶', '7' => '⁷', '8' => '⁸', '9' => '⁹',
        '+' => '⁺', '-' => '⁻', '−' => '⁻', '=' => '⁼', '(' => '⁽', ')' => '⁾',
        'a' => 'ᵃ', 'b' => 'ᵇ', 'c' => 'ᶜ', 'd' => 'ᵈ', 'e' => 'ᵉ', 'f' => 'ᶠ',
        'g' => 'ᵍ', 'h' => 'ʰ', 'i' => 'ⁱ', 'j' => 'ʲ', 'k' => 'ᵏ', 'l' => 'ˡ',
        'm' => 'ᵐ', 'n' => 'ⁿ', 'o' => 'ᵒ', 'p' => 'ᵖ', 'r' => 'ʳ', 's' => 'ˢ',
        't' => 'ᵗ', 'u' => 'ᵘ', 'v' => 'ᵛ', 'w' => 'ʷ', 'x' => 'ˣ', 'y' => 'ʸ', 'z' => 'ᶻ',
        'A' => 'ᴬ', 'B' => 'ᴮ', 'D' => 'ᴰ', 'E' => 'ᴱ', 'G' => 'ᴳ', 'H' => 'ᴴ',
        'I' => 'ᴵ', 'J' => 'ᴶ', 'K' => 'ᴷ', 'L' => 'ᴸ', 'M' => 'ᴹ', 'N' => 'ᴺ',
        'O' => 'ᴼ', 'P' => 'ᴾ', 'R' => 'ᴿ', 'T' => 'ᵀ', 'U' => 'ᵁ', 'V' => 'ⱽ', 'W' => 'ᵂ',
        '∞' => '∞',
        ' ' => ' ',
        _ => return None,
    })
}

/// 아래 첨자용 글자.
pub fn subscript(ch: char) -> Option<char> {
    Some(match ch {
        '0' => '₀', '1' => '₁', '2' => '₂', '3' => '₃', '4' => '₄',
        '5' => '₅', '6' => '₆', '7' => '₇', '8' => '₈', '9' => '₉',
        '+' => '₊', '-' => '₋', '−' => '₋', '=' => '₌', '(' => '₍', ')' => '₎',
        'a' => 'ₐ', 'e' => 'ₑ', 'h' => 'ₕ', 'i' => 'ᵢ', 'j' => 'ⱼ', 'k' => 'ₖ',
        'l' => 'ₗ', 'm' => 'ₘ', 'n' => 'ₙ', 'o' => 'ₒ', 'p' => 'ₚ', 'r' => 'ᵣ',
        's' => 'ₛ', 't' => 'ₜ', 'u' => 'ᵤ', 'v' => 'ᵥ', 'x' => 'ₓ',
        ' ' => ' ',
        _ => return None,
    })
}

/// 문자열 전체를 위 첨자로 바꾼다. 하나라도 실패하면 None.
pub fn to_superscript(s: &str) -> Option<String> {
    s.chars().map(superscript).collect()
}

pub fn to_subscript(s: &str) -> Option<String> {
    s.chars().map(subscript).collect()
}

/// 굵은 수학 문자(`\mathbf`).
pub fn math_bold(ch: char) -> char {
    match ch {
        'A'..='Z' => char::from_u32(0x1D400 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
        'a'..='z' => char::from_u32(0x1D41A + (ch as u32 - 'a' as u32)).unwrap_or(ch),
        '0'..='9' => char::from_u32(0x1D7CE + (ch as u32 - '0' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

/// 칠판체(`\mathbb`).
pub fn math_bb(ch: char) -> char {
    match ch {
        'C' => 'ℂ', 'H' => 'ℍ', 'N' => 'ℕ', 'P' => 'ℙ', 'Q' => 'ℚ', 'R' => 'ℝ', 'Z' => 'ℤ',
        'A'..='Z' => char::from_u32(0x1D538 + (ch as u32 - 'A' as u32)).unwrap_or(ch),
        'a'..='z' => char::from_u32(0x1D552 + (ch as u32 - 'a' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

/// 필기체(`\mathcal`).
pub fn math_cal(ch: char) -> char {
    match ch {
        'B' => 'ℬ', 'E' => 'ℰ', 'F' => 'ℱ', 'H' => 'ℋ', 'I' => 'ℐ', 'L' => 'ℒ',
        'M' => 'ℳ', 'R' => 'ℛ', 'e' => 'ℯ', 'g' => 'ℊ', 'o' => 'ℴ',
        'A'..='Z' => char::from_u32(0x1D49C + (ch as u32 - 'A' as u32)).unwrap_or(ch),
        'a'..='z' => char::from_u32(0x1D4B6 + (ch as u32 - 'a' as u32)).unwrap_or(ch),
        _ => ch,
    }
}

/// 프락투어(`\mathfrak`)는 폰트 지원이 고르지 않아 그대로 둔다.
pub fn math_frak(ch: char) -> char {
    ch
}

/// 명령 이름(역슬래시 제외) → 기호.
pub fn symbol(name: &str) -> Option<&'static str> {
    Some(match name {
        // 그리스 소문자
        "alpha" => "α", "beta" => "β", "gamma" => "γ", "delta" => "δ",
        "epsilon" => "ε", "varepsilon" => "ε", "zeta" => "ζ", "eta" => "η",
        "theta" => "θ", "vartheta" => "ϑ", "iota" => "ι", "kappa" => "κ",
        "lambda" => "λ", "mu" => "μ", "nu" => "ν", "xi" => "ξ",
        "pi" => "π", "varpi" => "ϖ", "rho" => "ρ", "varrho" => "ϱ",
        "sigma" => "σ", "varsigma" => "ς", "tau" => "τ", "upsilon" => "υ",
        "phi" => "φ", "varphi" => "φ", "chi" => "χ", "psi" => "ψ", "omega" => "ω",
        // 그리스 대문자
        "Gamma" => "Γ", "Delta" => "Δ", "Theta" => "Θ", "Lambda" => "Λ",
        "Xi" => "Ξ", "Pi" => "Π", "Sigma" => "Σ", "Upsilon" => "Υ",
        "Phi" => "Φ", "Psi" => "Ψ", "Omega" => "Ω",
        // 연산자
        "times" => "×", "div" => "÷", "pm" => "±", "mp" => "∓",
        "cdot" => "·", "ast" => "∗", "star" => "⋆", "circ" => "∘", "bullet" => "•",
        "oplus" => "⊕", "ominus" => "⊖", "otimes" => "⊗", "odot" => "⊙",
        "cap" => "∩", "cup" => "∪", "sqcap" => "⊓", "sqcup" => "⊔",
        "wedge" => "∧", "land" => "∧", "vee" => "∨", "lor" => "∨", "neg" => "¬", "lnot" => "¬",
        "setminus" => "∖", "backslash" => "\\",
        // 관계
        "le" => "≤", "leq" => "≤", "ge" => "≥", "geq" => "≥",
        "ne" => "≠", "neq" => "≠", "equiv" => "≡", "sim" => "∼", "simeq" => "≃",
        "approx" => "≈", "cong" => "≅", "propto" => "∝", "asymp" => "≍",
        "ll" => "≪", "gg" => "≫", "prec" => "≺", "succ" => "≻",
        "subset" => "⊂", "supset" => "⊃", "subseteq" => "⊆", "supseteq" => "⊇",
        "in" => "∈", "ni" => "∋", "notin" => "∉",
        "perp" => "⊥", "parallel" => "∥", "mid" => "∣",
        "models" => "⊨", "vdash" => "⊢", "dashv" => "⊣",
        // 화살표
        "to" => "→", "rightarrow" => "→", "gets" => "←", "leftarrow" => "←",
        "leftrightarrow" => "↔", "uparrow" => "↑", "downarrow" => "↓",
        "Rightarrow" => "⇒", "Leftarrow" => "⇐", "Leftrightarrow" => "⇔",
        "mapsto" => "↦", "longrightarrow" => "⟶", "longleftarrow" => "⟵",
        "implies" => "⟹", "iff" => "⟺", "hookrightarrow" => "↪", "rightleftharpoons" => "⇌",
        "nearrow" => "↗", "searrow" => "↘", "swarrow" => "↙", "nwarrow" => "↖",
        // 기타 기호
        "infty" => "∞", "partial" => "∂", "nabla" => "∇", "forall" => "∀",
        "exists" => "∃", "nexists" => "∄", "emptyset" => "∅", "varnothing" => "∅",
        "aleph" => "ℵ", "hbar" => "ℏ", "ell" => "ℓ", "Re" => "ℜ", "Im" => "ℑ",
        "prime" => "′", "degree" => "°", "angle" => "∠", "triangle" => "△",
        "square" => "□", "diamond" => "◇", "checkmark" => "✓", "dagger" => "†",
        "therefore" => "∴", "because" => "∵", "surd" => "√",
        "ldots" => "…", "dots" => "…", "cdots" => "⋯", "vdots" => "⋮", "ddots" => "⋱",
        "lceil" => "⌈", "rceil" => "⌉", "lfloor" => "⌊", "rfloor" => "⌋",
        "langle" => "⟨", "rangle" => "⟩", "|" => "‖", "Vert" => "‖", "vert" => "│",
        "%" => "%", "$" => "$", "&" => "&", "#" => "#", "_" => "_", "{" => "{", "}" => "}",
        "sharp" => "♯", "flat" => "♭", "natural" => "♮",
        "leftrightarrows" => "⇆", "circlearrowleft" => "↺",
        _ => return None,
    })
}

/// 위·아래 한계를 붙이는 큰 연산자.
pub fn big_operator(name: &str) -> Option<&'static str> {
    Some(match name {
        "sum" => "∑",
        "prod" => "∏",
        "coprod" => "∐",
        "bigcup" => "⋃",
        "bigcap" => "⋂",
        "bigvee" => "⋁",
        "bigwedge" => "⋀",
        "bigoplus" => "⨁",
        "bigotimes" => "⨂",
        "bigodot" => "⨀",
        "lim" => "lim",
        "limsup" => "lim sup",
        "liminf" => "lim inf",
        "max" => "max",
        "min" => "min",
        "sup" => "sup",
        "inf" => "inf",
        "argmax" => "argmax",
        "argmin" => "argmin",
        _ => return None,
    })
}

/// 적분처럼 한계를 오른쪽에 붙이는 연산자.
pub fn integral(name: &str) -> Option<&'static str> {
    Some(match name {
        "int" => "∫",
        "iint" => "∬",
        "iiint" => "∭",
        "oint" => "∮",
        _ => return None,
    })
}

/// 로만체로 쓰는 함수 이름.
pub fn function_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "log" => "log", "ln" => "ln", "lg" => "lg", "exp" => "exp",
        "sin" => "sin", "cos" => "cos", "tan" => "tan", "cot" => "cot",
        "sec" => "sec", "csc" => "csc", "sinh" => "sinh", "cosh" => "cosh",
        "tanh" => "tanh", "arcsin" => "arcsin", "arccos" => "arccos", "arctan" => "arctan",
        "det" => "det", "dim" => "dim", "ker" => "ker", "deg" => "deg",
        "gcd" => "gcd", "mod" => "mod", "bmod" => "mod", "pmod" => "mod",
        "Pr" => "Pr", "tr" => "tr", "rank" => "rank",
        _ => return None,
    })
}

/// 강세 명령 → 얹을 기호.
pub fn accent_mark(name: &str) -> Option<char> {
    Some(match name {
        "overline" | "bar" => '─',
        "hat" | "widehat" => '^',
        "tilde" | "widetilde" => '~',
        "vec" => '→',
        "dot" => '·',
        "ddot" => '¨',
        "check" => 'ˇ',
        "breve" => '˘',
        "acute" => '´',
        "grave" => '`',
        _ => return None,
    })
}

/// 공백 명령 → 공백 수.
pub fn spacing(name: &str) -> Option<usize> {
    Some(match name {
        "," | ":" | ";" | "thinspace" | "medspace" | "thickspace" | "enspace" => 1,
        "!" | "negthinspace" => 0,
        "quad" => 2,
        "qquad" => 4,
        " " | "space" => 1,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_greek_and_relations() {
        assert_eq!(symbol("rho"), Some("ρ"));
        assert_eq!(symbol("le"), Some("≤"));
        assert_eq!(symbol("nope"), None);
    }

    #[test]
    fn script_conversion_is_all_or_nothing() {
        assert_eq!(to_superscript("2n"), Some("²ⁿ".to_string()));
        assert_eq!(to_subscript("total"), Some("ₜₒₜₐₗ".to_string()));
        assert_eq!(to_subscript("q"), None);
    }

    #[test]
    fn blackboard_and_calligraphic() {
        assert_eq!(math_bb('R'), 'ℝ');
        assert_eq!(math_cal('L'), 'ℒ');
    }
}
