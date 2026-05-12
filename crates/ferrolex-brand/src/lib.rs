//! Lexios-powered brand validation runtime.
//!
//! Ferrolex does not define a competing brand guide format. This crate is the
//! runtime checking layer for Lexios Brand ID data.

use ferrolex_core::{Checker, Finding, Span, Token};

/// Placeholder for a compiled Lexios-derived brand rule set.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BrandRuleSet {
    rule_count: usize,
}

impl BrandRuleSet {
    /// Creates an empty brand rule set.
    #[must_use]
    pub const fn new() -> Self {
        Self { rule_count: 0 }
    }

    /// Returns the number of compiled brand rules.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.rule_count
    }

    /// Returns true when this rule set has no rules.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.rule_count == 0
    }
}

/// Brand checker using a Lexios-derived rule set.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BrandChecker {
    rules: BrandRuleSet,
}

impl BrandChecker {
    /// Creates a brand checker.
    #[must_use]
    pub const fn new(rules: BrandRuleSet) -> Self {
        Self { rules }
    }

    /// Returns the rule set used by this checker.
    #[must_use]
    pub const fn rules(&self) -> &BrandRuleSet {
        &self.rules
    }
}

impl Checker for BrandChecker {
    fn check(&self, _span: &Span, _tokens: &[Token]) -> Vec<Finding> {
        Vec::new()
    }
}
