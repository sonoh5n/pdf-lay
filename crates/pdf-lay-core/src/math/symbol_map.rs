//! Symbol mapping tables for math conversion.

use std::collections::{HashMap, HashSet};

/// Returns a mapping from symbol characters to their LaTeX commands.
/// Example: 'α' → "\\alpha", '∑' → "\\sum"
pub fn to_latex_map() -> HashMap<char, &'static str> {
    let mut m = HashMap::new();

    // Greek lowercase
    m.insert('α', "\\alpha");
    m.insert('β', "\\beta");
    m.insert('γ', "\\gamma");
    m.insert('δ', "\\delta");
    m.insert('ε', "\\epsilon");
    m.insert('ζ', "\\zeta");
    m.insert('η', "\\eta");
    m.insert('θ', "\\theta");
    m.insert('ι', "\\iota");
    m.insert('κ', "\\kappa");
    m.insert('λ', "\\lambda");
    m.insert('μ', "\\mu");
    m.insert('ν', "\\nu");
    m.insert('ξ', "\\xi");
    m.insert('π', "\\pi");
    m.insert('ρ', "\\rho");
    m.insert('σ', "\\sigma");
    m.insert('τ', "\\tau");
    m.insert('υ', "\\upsilon");
    m.insert('φ', "\\phi");
    m.insert('χ', "\\chi");
    m.insert('ψ', "\\psi");
    m.insert('ω', "\\omega");

    // Greek uppercase
    m.insert('Γ', "\\Gamma");
    m.insert('Δ', "\\Delta");
    m.insert('Θ', "\\Theta");
    m.insert('Λ', "\\Lambda");
    m.insert('Ξ', "\\Xi");
    m.insert('Π', "\\Pi");
    m.insert('Σ', "\\Sigma");
    m.insert('Φ', "\\Phi");
    m.insert('Ψ', "\\Psi");
    m.insert('Ω', "\\Omega");

    // Operators
    m.insert('±', "\\pm");
    m.insert('∓', "\\mp");
    m.insert('×', "\\times");
    m.insert('÷', "\\div");
    m.insert('·', "\\cdot");
    m.insert('∗', "\\ast");
    m.insert('∘', "\\circ");
    m.insert('∑', "\\sum");
    m.insert('∏', "\\prod");
    m.insert('∫', "\\int");
    m.insert('∂', "\\partial");
    m.insert('∇', "\\nabla");
    m.insert('√', "\\sqrt");

    // Relations
    m.insert('≤', "\\leq");
    m.insert('≥', "\\geq");
    m.insert('≠', "\\neq");
    m.insert('≈', "\\approx");
    m.insert('≡', "\\equiv");
    m.insert('∝', "\\propto");
    m.insert('∈', "\\in");
    m.insert('∉', "\\notin");
    m.insert('⊂', "\\subset");
    m.insert('⊃', "\\supset");
    m.insert('⊆', "\\subseteq");
    m.insert('⊇', "\\supseteq");
    m.insert('∪', "\\cup");
    m.insert('∩', "\\cap");

    // Arrows
    m.insert('→', "\\to");
    m.insert('←', "\\leftarrow");
    m.insert('↔', "\\leftrightarrow");
    m.insert('⇒', "\\Rightarrow");
    m.insert('⇐', "\\Leftarrow");
    m.insert('⇔', "\\Leftrightarrow");

    // Misc
    m.insert('∞', "\\infty");
    m.insert('∅', "\\emptyset");
    m.insert('∀', "\\forall");
    m.insert('∃', "\\exists");
    m.insert('¬', "\\neg");
    m.insert('∧', "\\land");
    m.insert('∨', "\\lor");

    m
}

/// Returns a mapping for Unicode superscript/subscript digits.
pub fn to_unicode_map() -> (HashMap<char, char>, HashMap<char, char>) {
    // superscript: '0'-'9' → '⁰','¹','²','³','⁴','⁵','⁶','⁷','⁸','⁹'
    // subscript: '0'-'9' → '₀','₁','₂','₃','₄','₅','₆','₇','₈','₉'
    // Also include some letter superscripts: 'n' → 'ⁿ', 'i' → 'ⁱ'
    let mut sup = HashMap::new();
    let mut sub = HashMap::new();

    let sup_digits = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    let sub_digits = ['₀', '₁', '₂', '₃', '₄', '₅', '₆', '₇', '₈', '₉'];

    for (i, d) in ('0'..='9').enumerate() {
        sup.insert(d, sup_digits[i]);
        sub.insert(d, sub_digits[i]);
    }

    // Additional superscript letters
    sup.insert('n', 'ⁿ');
    sup.insert('i', 'ⁱ');
    sup.insert('+', '⁺');
    sup.insert('-', '⁻');
    sup.insert('=', '⁼');
    sup.insert('(', '⁽');
    sup.insert(')', '⁾');

    // Additional subscript letters
    sub.insert('+', '₊');
    sub.insert('-', '₋');
    sub.insert('=', '₌');
    sub.insert('(', '₍');
    sub.insert(')', '₎');

    (sup, sub)
}

/// Returns the set of characters considered mathematical symbols for detection.
pub fn math_symbols() -> HashSet<char> {
    let latex_map = to_latex_map();
    let mut symbols: HashSet<char> = latex_map.keys().copied().collect();

    // Add more math-related characters not in LaTeX map
    for c in [
        '∀', '∃', '∄', '∅', '∆', '∇', '∈', '∉', '∋', '∌', '∎', '∏', '∐', '∑', '−', '∓', '∔', '∕',
        '∖', '∗', '∘', '∙', '√', '∛', '∜', '∝', '∞', '∟', '∠', '∡', '≤', '≥', '≦', '≧', '≨', '≩',
        '≪', '≫', '≬', '≭',
    ] {
        symbols.insert(c);
    }

    symbols
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latex_map_has_minimum_entries() {
        let m = to_latex_map();
        assert!(m.len() >= 30, "Expected 30+ entries, got {}", m.len());
        assert_eq!(m[&'α'], "\\alpha");
        assert_eq!(m[&'∑'], "\\sum");
    }

    #[test]
    fn test_unicode_superscript_digits() {
        let (sup, _sub) = to_unicode_map();
        assert_eq!(sup[&'0'], '⁰');
        assert_eq!(sup[&'2'], '²');
        assert_eq!(sup[&'9'], '⁹');
    }

    #[test]
    fn test_unicode_subscript_digits() {
        let (_sup, sub) = to_unicode_map();
        assert_eq!(sub[&'0'], '₀');
        assert_eq!(sub[&'2'], '₂');
    }

    #[test]
    fn test_math_symbols_includes_greek() {
        let s = math_symbols();
        assert!(s.contains(&'α'));
        assert!(s.contains(&'β'));
        assert!(s.contains(&'Σ'));
    }
}
